use crate::capabilities::lib::put_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap};
use dag_cache_types::types::api::bulk_put;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash, IPFSHeader};
use dag_cache_types::types::validated_tree::ValidatedTree;
use futures::future::FutureExt;
use futures::Future;
use std::sync::Arc;
use tokio;
use tokio::sync::oneshot;
use tracing::error;

// catamorphism - a consuming change
// recursively publish DAG node tree to IPFS, starting with leaf nodes
pub async fn ipfs_publish_cata<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: ValidatedTree,
) -> Result<(u64, IPFSHash), DagCacheError> {
    let focus = tree.root_node.clone();
    let tree = Arc::new(tree);
    ipfs_publish_cata_unsafe(caps, tree, focus).await
}

// unsafe b/c it can take any 'focus' ClientId and not just the root node of tree
async fn ipfs_publish_cata_unsafe<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: Arc<ValidatedTree>, // todo use async/await I guess, mb can avoid needing Arc? ugh
    node: bulk_put::DagNode,
) -> Result<(u64, IPFSHash), DagCacheError> {
    let (send, receive) = oneshot::channel();

    let f = ipfs_publish_worker(caps, tree, send, node);
    tokio::spawn(f);

    let recvd = receive.await;

    match recvd {
        Ok(x) => x,
        Err(e) => {
            let e = DagCacheError::UnexpectedError {
                // todo capture recv error
                msg: format!("one shot channel cancelled, {:?}", e),
            }; // one-shot channel cancelled
            Err(e)
        }
    }
}

async fn upload_link<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    x: bulk_put::DagNodeLink,
    tree: Arc<ValidatedTree>,
    caps: Arc<C>,
) -> Result<IPFSHeader, DagCacheError> {
    match x {
        bulk_put::DagNodeLink::Local(client_side_hash) => {
            // NOTE: could be more efficient by removing node from tree but would break
            // guarantees provided by ValidatedTree (by removing nodes)
            // NOTE: not possible, really - would need Mut access to the hashmap to do that

            // unhandled deref failure, known to be safe b/c of validated tree wrapper
            let node = tree.nodes[&client_side_hash].clone();

            // todo: figure out how to get this as 100% async
            let (size, hash) =
                ipfs_publish_cata_unsafe(caps.clone(), tree.clone(), node.clone()).await?;
            let hdr = IPFSHeader {
                name: client_side_hash.to_string(),
                size,
                hash,
            };
            Ok(hdr)
        }
        bulk_put::DagNodeLink::Remote(hdr) => Ok(hdr.clone()),
    }
}

// needed to not have async cycle? idk lmao FIXME refactor
fn ipfs_publish_worker<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: Arc<ValidatedTree>,
    // TODO: pass around pointers to node in stack frame (hm keys) instead of nodes
    // OR NOT: struct is quite small, even if the owned-by-it vec of u8/vec of links is big
    // TODO: ask rain
    chan: oneshot::Sender<Result<(u64, IPFSHash), DagCacheError>>,
    node: bulk_put::DagNode,
) -> Box<dyn Future<Output = ()> + Unpin + Send> {
    let f = ipfs_publish_worker_async(caps, tree, node)
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
async fn ipfs_publish_worker_async<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: Arc<ValidatedTree>,
    // TODO: pass around pointers to node in stack frame (hm keys) instead of nodes
    // OR NOT: struct is quite small, even if the owned-by-it vec of u8/vec of links is big
    // TODO: ask rain
    node: bulk_put::DagNode,
) -> Result<(u64, IPFSHash), DagCacheError> {
    let bulk_put::DagNode { data, links } = node;

    let size = data.0.len() as u64;

    let link_uploads: Vec<_> = links
        .into_iter()
        .map(|ln| upload_link(ln, tree.clone(), caps.clone()))
        .collect();

    let joined_link_uploads: Vec<Result<IPFSHeader, DagCacheError>> =
        futures::future::join_all(link_uploads).await;
    let links: Vec<IPFSHeader> = joined_link_uploads
        .into_iter()
        .collect::<Result<Vec<IPFSHeader>, DagCacheError>>()?;

    let dag_node = DagNode { data, links };

    // might be a bit of an approximation, but w/e
    let size = size + dag_node.links.iter().map(|x| x.size).sum::<u64>();

    let hash = put_and_cache(caps.as_ref(), dag_node).await?;
    Ok((size, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CacheCapability, HasCacheCap, HasIPFSCap, IPFSCapability};
    use crate::lib;
    use async_trait::async_trait;
    use dag_cache_types::types::api::ClientId;
    use dag_cache_types::types::encodings::{Base58, Base64};
    use dag_cache_types::types::errors::DagCacheError;
    use dag_cache_types::types::ipfs::{DagNode, IPFSHash};
    use dag_cache_types::types::validated_tree::ValidatedTree;
    use hashbrown::HashMap;
    use std::sync::Mutex;

    struct MockIPFS(Mutex<HashMap<IPFSHash, DagNode>>);

    struct BlackHoleCache;
    impl CacheCapability for BlackHoleCache {
        fn get(&self, _k: IPFSHash) -> Option<DagNode> { None }

        fn put(&self, _k: IPFSHash, _v: DagNode) {}
    }

    // TODO: separate read/write caps to simplify writing this?
    #[async_trait]
    impl IPFSCapability for MockIPFS {
        async fn get(&self, k: IPFSHash) -> Result<DagNode, DagCacheError> {
            let map = self.0.lock().unwrap();
            let v = map.get(&k).unwrap(); // fail if not found in map
            Ok(v.clone())
        }

        async fn put(&self, v: DagNode) -> Result<IPFSHash, DagCacheError> {
            let mut map = self.0.lock().unwrap(); // fail on mutex poisoned

            // use map len (monotonic increasing) to provide unique hash ID
            let hash = IPFSHash::from_raw(Base58::from_bytes(vec![map.len() as u8]));

            map.insert(hash.clone(), v);

            Ok(hash)
        }
    }

    struct TestCaps(MockIPFS, BlackHoleCache);

    impl HasIPFSCap for TestCaps {
        type Output = MockIPFS;
        fn ipfs_caps(&self) -> &MockIPFS { &self.0 }
    }

    impl HasCacheCap for TestCaps {
        type Output = BlackHoleCache;
        fn cache_caps(&self) -> &BlackHoleCache { &self.1 }
    }

    // uses mock capabilities, does not require local ipfs daemon
    #[tokio::test]
    async fn test_batch_upload() {
        lib::init_test_env(); // tracing subscriber

        //build some client side 'hashes' - base58 of 1, 2, 3, 4
        let client_hashes: Vec<ClientId> = (1..=4)
            .map(|x| ClientId::new(Base58::from_bytes(vec![x])))
            .collect();

        let t0 = bulk_put::DagNode {
            links: vec![],
            data: Base64(vec![1, 3, 3, 7]),
        };

        let t1 = bulk_put::DagNode {
            links: vec![],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t2 = bulk_put::DagNode {
            links: vec![
                bulk_put::DagNodeLink::Local(client_hashes[0].clone()),
                bulk_put::DagNodeLink::Local(client_hashes[1].clone()),
            ],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t3 = bulk_put::DagNode {
            links: vec![bulk_put::DagNodeLink::Local(client_hashes[2].clone())],
            data: Base64(vec![0, 1, 1, 2, 3, 5]),
        };

        let mut m = HashMap::new();
        m.insert(client_hashes[0].clone(), t0.clone());
        m.insert(client_hashes[1].clone(), t1.clone());
        m.insert(client_hashes[2].clone(), t2.clone());

        let validated_tree = ValidatedTree::validate(t3.clone(), m).expect("static test invalid");

        let mock_ipfs = MockIPFS(Mutex::new(HashMap::new()));
        let caps = std::sync::Arc::new(TestCaps(mock_ipfs, BlackHoleCache));

        let _published = ipfs_publish_cata(caps.clone(), validated_tree)
            .await
            .expect("ipfs publish cata error");

        let map = (caps.0).0.lock().unwrap();

        let uploaded_values: Vec<(Vec<ClientId>, Base64)> = map
            .values()
            .map(|DagNode { links, data }| {
                (
                    links
                        .iter()
                        .map(|x| ClientId::new(Base58::from_string(&x.name).unwrap()))
                        .collect(),
                    data.clone(),
                )
            })
            .collect();

        assert!(&uploaded_values.contains(&(vec!(), t0.data))); // t1 uploaded
        assert!(&uploaded_values.contains(&(vec!(), t1.data))); // t2 uploaded
        assert!(&uploaded_values.contains(&(
            vec!(client_hashes[0].clone(), client_hashes[1].clone()),
            t2.data
        ))); // t3 uploaded
        assert!(&uploaded_values.contains(&(vec!(client_hashes[2].clone()), t3.data)));
        // t4 uploaded
    }
}
