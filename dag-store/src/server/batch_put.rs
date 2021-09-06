use crate::capabilities::put_and_cache;
use crate::capabilities::{Cache, HashedBlobStore, MutableHashStore};
use dag_store_types::types::{
    api::bulk_put,
    domain::{Hash, Header, Id, Node},
    errors::DagCacheError,
    validated_tree::ValidatedTree,
};
use std::sync::Arc;
use tokio;
use tracing::info;

// TODO: how to make this transactional while maintaining caps approach? ans: have an impl of the ipfsCap (TODO: rename to hash store)
// that is the _transaction-scoped_ tree - pretty sure this is supported.

pub async fn batch_put_cata_with_cas<'a>(
    mhs: &'a Arc<dyn MutableHashStore>,
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    tree: ValidatedTree,
    cas: Option<bulk_put::CAS>,
) -> Result<bulk_put::Resp, DagCacheError> {
    match cas {
        Some(cas) => {
            let cas_current_hash = mhs.get(&cas.cas_key).await?;
            if cas_current_hash == cas.required_previous_hash {
                info!("some cas, writing to store via cata");
                let res = batch_put_cata(store, cache, tree).await?;
                info!("some cas, got res: {:?}", &res);
                mhs.cas(
                    &cas.cas_key,
                    cas.required_previous_hash,
                    res.root_hash.clone(),
                )
                .await?;
                info!("some cas, wrote res hash to mhs");
                Ok(res)
            } else {
                info!("skipping cas op, provided prev hash was stale");
                Err(DagCacheError::CASViolationError {
                    actual_hash: cas_current_hash,
                })
            }
        }
        None => {
            let res = batch_put_cata(store, cache, tree).await?;
            Ok(res)
        }
    }
}

// catamorphism - a consuming change
// recursively publish DAG node tree to IPFS, starting with leaf nodes
pub async fn batch_put_cata<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    tree: ValidatedTree,
) -> Result<bulk_put::Resp, DagCacheError> {
    let focus = tree.root_node.clone();
    let tree = Arc::new(tree);
    // NOTE: should not need to clone here
    let (root_hash, additional_uploaded) =
        batch_put_worker(store.clone(), cache.clone(), tree, focus).await?; // TODO: don't panic on join error
    Ok(bulk_put::Resp {
        root_hash,
        additional_uploaded,
    })
}

fn upload_link<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    x: bulk_put::NodeLink,
    tree: Arc<ValidatedTree>,
) -> tokio::task::JoinHandle<Result<(Header, Vec<(Id, Hash)>), DagCacheError>> {
    let store = store.clone();
    let cache = cache.clone();
    tokio::spawn(async move {
        match x {
            bulk_put::NodeLink::Local(id) => {
                // NOTE: could be more efficient by removing node from tree but would break
                // guarantees provided by ValidatedTree (by removing nodes)
                // NOTE: not possible, really - would need Mut access to the hashmap to do that

                // unhandled deref failure, known to be safe b/c of validated tree wrapper
                let node = tree.nodes[&id].clone();

                let (hash, mut additional_uploaded) =
                    batch_put_worker(store.clone(), cache.clone(), tree.clone(), node.clone())
                        .await?;
                let hdr = Header { id, hash };
                additional_uploaded.push((id, hdr.hash.clone()));
                Ok((hdr, additional_uploaded))
            }
            bulk_put::NodeLink::Remote(hdr) => Ok((hdr.clone(), Vec::new())),
        }
    })
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
async fn batch_put_worker(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: Arc<ValidatedTree>,
    // TODO: pass around pointers to node in stack frame (hm keys) instead of nodes
    // OR NOT: struct is quite small, even if the owned-by-it vec of u8/vec of links is big
    // TODO: ask rain
    node: bulk_put::Node,
) -> Result<(Hash, Vec<(Id, Hash)>), DagCacheError> {
    let bulk_put::Node { data, links } = node;

    // let link_uploads: Vec<tokio::task::JoinHandle<>> = links
    let link_uploads: Vec<
        tokio::task::JoinHandle<Result<(Header, Vec<(Id, Hash)>), DagCacheError>>,
    > = links
        .into_iter()
        .map(|ln| upload_link(&store, &cache, ln, tree.clone()))
        .collect();

    let joined_link_uploads: Vec<Result<Result<_, DagCacheError>, tokio::task::JoinError>> =
        futures::future::join_all(link_uploads).await;
    let links: Vec<_> = joined_link_uploads
        .into_iter()
        .map(|x| x.unwrap()) // TODO: figure out how to handle join errors
        .collect::<Result<Vec<_>, DagCacheError>>()?;

    let additional_uploaded: Vec<(Id, Hash)> =
        links.iter().map(|x| x.1.clone()).flatten().collect();

    let links = links.into_iter().map(|x| x.0).collect();
    let dag_node = Node { data, links };

    let hash = put_and_cache(&store, &cache, dag_node).await?;
    Ok((hash, additional_uploaded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use dag_store_types::types::domain::{Hash, Node};
    use dag_store_types::types::encodings::Base64;
    use dag_store_types::types::errors::DagCacheError;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockStore(Mutex<HashMap<Hash, Node>>);

    // TODO: separate read/write caps to simplify writing this?
    #[async_trait]
    impl HashedBlobStore for MockStore {
        async fn get(&self, k: Hash) -> Result<Node, DagCacheError> {
            let map = self.0.lock().unwrap();
            let v = map.get(&k).unwrap(); // fail if not found in map
            Ok(v.clone())
        }

        async fn put(&self, node: Node) -> Result<Hash, DagCacheError> {
            let mut map = self.0.lock().unwrap(); // fail on mutex poisoned

            // use map len (monotonic increasing) to provide unique hash ID
            let hash = node.canonical_hash();

            map.insert(hash.clone(), node);

            Ok(hash)
        }
    }

    // uses mock capabilities, does not require local fs state
    #[tokio::test]
    async fn test_batch_upload() {
        //build some client side 'hashes' - base58 of 1, 2, 3, 4
        let client_ids: Vec<Id> = (1..4).map(|x| Id(x)).collect();

        let t0 = bulk_put::Node {
            links: vec![],
            data: Base64(vec![1, 3, 3, 7]),
        };

        let t1 = bulk_put::Node {
            links: vec![],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t2 = bulk_put::Node {
            links: vec![
                bulk_put::NodeLink::Local(client_ids[0].clone()),
                bulk_put::NodeLink::Local(client_ids[1].clone()),
            ],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t3 = bulk_put::Node {
            links: vec![bulk_put::NodeLink::Local(client_ids[2].clone())],
            data: Base64(vec![0, 1, 1, 2, 3, 5]),
        };

        let mut m = HashMap::new();
        m.insert(client_ids[0].clone(), t0.clone());
        m.insert(client_ids[1].clone(), t1.clone());
        m.insert(client_ids[2].clone(), t2.clone());

        let validated_tree = ValidatedTree::validate(t3.clone(), m).expect("static test invalid");

        let store = Arc::new(MockStore(Mutex::new(HashMap::new())));

        let cache = Arc::new(Cache::new(16));

        fn shim(x: Arc<MockStore>) -> Arc<dyn HashedBlobStore> {
            x
        }

        // this needs an Arc<dyn HashedBlobStore>
        let _published = batch_put_cata(&shim(store.clone()), &cache, validated_tree)
            .await
            .expect("publish cata error");

        // this pins it, to, specifically, mockstore
        let map = store.0.lock().unwrap();

        let uploaded_values: Vec<(Vec<Id>, Base64)> = map
            .values()
            .map(|Node { links, data }| (links.iter().map(|x| Id(x.id.0)).collect(), data.clone()))
            .collect();

        assert!(&uploaded_values.contains(&(vec!(), t0.data))); // t1 uploaded
        assert!(&uploaded_values.contains(&(vec!(), t1.data))); // t2 uploaded
        assert!(&uploaded_values
            .contains(&(vec!(client_ids[0].clone(), client_ids[1].clone()), t2.data))); // t3 uploaded
        assert!(&uploaded_values.contains(&(vec!(client_ids[2].clone()), t3.data)));
        // t4 uploaded
    }
}
