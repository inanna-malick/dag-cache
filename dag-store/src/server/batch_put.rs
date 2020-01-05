use crate::capabilities::put_and_cache;
use crate::capabilities::{Cache, HashedBlobStore, MutableHashStore};
use dag_store_types::types::{
    api::{bulk_put, ClientId},
    domain::{Hash, Header, Node},
    errors::DagCacheError,
    validated_tree::ValidatedTree,
};
use futures::future::FutureExt;
use futures::Future;
use std::sync::Arc;
use tokio;
use tokio::sync::oneshot;
use tracing::error;
use tracing::info;

// TODO: how to make this transactional while maintaining caps approach? ans: have an impl of the ipfsCap (TODO: rename to hash store)
// that is the _transaction-scoped_ tree - pretty sure this is supported. will likely need to move to dyn
// (fat pointers) to avoid excessive code gen but idk

pub async fn batch_put_cata_with_cas(
    mhs: Arc<dyn MutableHashStore>,
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: ValidatedTree,
    cas: Option<bulk_put::CAS>,
) -> Result<bulk_put::Resp, DagCacheError> {
    match cas {
        Some(cas) => {
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
        }
        None => {
            let res = batch_put_cata(store, cache, tree).await?;
            Ok(res)
        }
    }
}

// catamorphism - a consuming change
// recursively publish DAG node tree to IPFS, starting with leaf nodes
pub async fn batch_put_cata(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: ValidatedTree,
) -> Result<bulk_put::Resp, DagCacheError> {
    let focus = tree.root_node.clone();
    let tree = Arc::new(tree);
    let (_size, root_hash, additional_uploaded) =
        batch_put_cata_unsafe(store, cache, tree, focus).await?;
    Ok(bulk_put::Resp {
        root_hash,
        additional_uploaded,
    })
}

// unsafe b/c it can take any 'focus' ClientId and not just the root node of tree
async fn batch_put_cata_unsafe(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: Arc<ValidatedTree>, // todo use async/await I guess, mb can avoid needing Arc? ugh
    node: bulk_put::Node,
) -> Result<(u64, Hash, Vec<(ClientId, Hash)>), DagCacheError> {
    let (send, receive) = oneshot::channel();

    let f = batch_put_worker(store, cache, tree, send, node);
    tokio::spawn(f);

    let recvd = receive.await;

    match recvd {
        Ok(x) => x,
        Err(e) => {
            let e = DagCacheError::UnexpectedError(
                // todo capture recv error
                format!("one shot channel cancelled, {:?}", e),
            ); // one-shot channel cancelled
            Err(e)
        }
    }
}

async fn upload_link(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    x: bulk_put::NodeLink,
    tree: Arc<ValidatedTree>,
) -> Result<(Header, Vec<(ClientId, Hash)>), DagCacheError> {
    match x {
        bulk_put::NodeLink::Local(client_id) => {
            // NOTE: could be more efficient by removing node from tree but would break
            // guarantees provided by ValidatedTree (by removing nodes)
            // NOTE: not possible, really - would need Mut access to the hashmap to do that

            // unhandled deref failure, known to be safe b/c of validated tree wrapper
            let node = tree.nodes[&client_id].clone();

            let (size, hash, mut additional_uploaded) =
                batch_put_cata_unsafe(store, cache.clone(), tree.clone(), node.clone()).await?;
            let hdr = Header {
                name: client_id.to_string(),
                size,
                hash,
            };
            additional_uploaded.push((client_id, hdr.hash.clone()));
            Ok((hdr, additional_uploaded))
        }
        bulk_put::NodeLink::Remote(hdr) => Ok((hdr.clone(), Vec::new())),
    }
}

// needed to not have async cycle? idk lmao FIXME refactor
fn batch_put_worker(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: Arc<ValidatedTree>,
    chan: oneshot::Sender<Result<(u64, Hash, Vec<(ClientId, Hash)>), DagCacheError>>,
    // TODO: pass around pointers to node in stack frame (hm keys) instead of nodes
    // OR NOT: struct is quite small, even if the owned-by-it vec of u8/vec of links is big
    // TODO: ask rain
    node: bulk_put::Node,
) -> Box<dyn Future<Output = ()> + Unpin + Send> {
    let f = batch_put_worker_async(store, cache, tree, node)
        .map(move |res| {
            let chan_send_res = chan.send(res);
            if let Err(err) = chan_send_res {
                error!("failed oneshot channel send {:?}", err);
            };
        })
        .boxed();

    Box::new(f)
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
async fn batch_put_worker_async(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    tree: Arc<ValidatedTree>,
    // TODO: pass around pointers to node in stack frame (hm keys) instead of nodes
    // OR NOT: struct is quite small, even if the owned-by-it vec of u8/vec of links is big
    // TODO: ask rain
    node: bulk_put::Node,
) -> Result<(u64, Hash, Vec<(ClientId, Hash)>), DagCacheError> {
    let bulk_put::Node { data, links } = node;

    let size = data.0.len() as u64;

    let link_uploads: Vec<_> = links
        .into_iter()
        .map(|ln| upload_link(store.clone(), cache.clone(), ln, tree.clone()))
        .collect();

    let joined_link_uploads: Vec<Result<_, DagCacheError>> =
        futures::future::join_all(link_uploads).await;
    let links: Vec<_> = joined_link_uploads
        .into_iter()
        .collect::<Result<Vec<_>, DagCacheError>>()?;

    let additional_uploaded: Vec<(ClientId, Hash)> =
        links.iter().map(|x| x.1.clone()).flatten().collect();

    let links = links.into_iter().map(|x| x.0).collect();
    let dag_node = Node { data, links };

    // might be a bit of an approximation, but w/e
    let size = size + dag_node.links.iter().map(|x| x.size).sum::<u64>();

    let hash = put_and_cache(store, cache, dag_node).await?;
    Ok((size, hash, additional_uploaded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use dag_store_types::types::domain::{Hash, Node};
    use dag_store_types::types::encodings::{Base58, Base64};
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

        async fn put(&self, v: Node) -> Result<Hash, DagCacheError> {
            let mut map = self.0.lock().unwrap(); // fail on mutex poisoned

            // use map len (monotonic increasing) to provide unique hash ID
            let hash = Hash::from_raw(Base58::from_bytes(vec![map.len() as u8]));

            map.insert(hash.clone(), v);

            Ok(hash)
        }
    }

    // uses mock capabilities, does not require local fs state
    #[tokio::test]
    async fn test_batch_upload() {
        //build some client side 'hashes' - base58 of 1, 2, 3, 4
        let client_ids: Vec<ClientId> = (1..4).map(|x| ClientId::new(format!("{}", x))).collect();

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

        let _published = batch_put_cata(store.clone(), cache, validated_tree)
            .await
            .expect("publish cata error");

        let map = store.0.lock().unwrap();

        let uploaded_values: Vec<(Vec<ClientId>, Base64)> = map
            .values()
            .map(|Node { links, data }| {
                (
                    links
                        .iter()
                        .map(|x| ClientId::new(x.name.clone()))
                        .collect(),
                    data.clone(),
                )
            })
            .collect();

        assert!(&uploaded_values.contains(&(vec!(), t0.data))); // t1 uploaded
        assert!(&uploaded_values.contains(&(vec!(), t1.data))); // t2 uploaded
        assert!(&uploaded_values
            .contains(&(vec!(client_ids[0].clone(), client_ids[1].clone()), t2.data))); // t3 uploaded
        assert!(&uploaded_values.contains(&(vec!(client_ids[2].clone()), t3.data)));
        // t4 uploaded
    }
}
