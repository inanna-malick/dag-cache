use futures::future;
use futures::future::Future;

use crate::api_types;
use crate::in_mem_types;
use crate::ipfs_types;

use std::sync::Arc;

use tokio;

use crate::api_types::ClientSideHash;
use crate::cache::HasCacheCap;
use crate::ipfs_api::HasIPFSCap;
use crate::ipfs_types::IPFSHeader;
use crate::in_mem_types::{DagTree, DagTreeLink};
use crate::lib::BoxFuture;
use std::convert::AsRef;
use tracing::info;

use futures::sync::oneshot;
use petgraph::graph;

use std::collections::HashMap;

enum DagNodeBody {
    NotYetUploaded,
    AlreadyUploaded,
}; // todo, actual body.

struct MerkleTree<H> {
    root: graph::NodeIndex<usize>,
    graph: graph::Graph<DagNodeBody, H, petgraph::Directed, usize>,
}

// // anamorphism (todo futu via cache, v1 goal is explicitly futu)
// // NOTE: could speed up hugely by static caching graph structure
// pub fn ipfs_fetch_ana<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
//     caps: Arc<C>,
//     hash: ClientSideHash,
//     root: IPFSHash,
// ) -> impl Future<Item = MerkleDag, Error = api_types::DagCacheError> + 'static + Send {
// } // NOTE: should be viable, best thing to do is mb just return petgraph graph for this instead of recursive data structure

// catamorphism - a consuming change
// recursively publish DAG node tree to IPFS, starting with leaf nodes
pub fn ipfs_publish_cata<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    hash: ClientSideHash,
    tree: MerkleTree<ClientSideHash>,
) -> impl Future<Item = IPFSHeader, Error = api_types::DagCacheError> + 'static + Send {
    let (send, receive) = oneshot::channel();

    tokio::spawn(ipfs_publish_worker(caps, send, hash, tree));

    receive
        .map_err(|_| api_types::DagCacheError::UnexpectedError {
            msg: "one shot channel cancelled".to_string(),
        }) // one-shot channel cancelled
        .then(move |x| x)
        .and_then(|res| match res {
            Ok(res) => future::ok(res),
            Err(err) => future::err(err),
        })
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
fn ipfs_publish_worker<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    chan: oneshot::Sender<Result<IPFSHeader, api_types::DagCacheError>>,
    hash: ClientSideHash,
    node: MerkleTree<ClientSideHash>,
) -> impl Future<Item = (), Error = ()> + 'static + Send {
    let MerkleTree { root, graph } = node;

    let link_fetches: Vec<_> = links
        .into_iter()
        .map({
            |x| -> BoxFuture<IPFSHeader, api_types::DagCacheError> {
                match x {
                    DagTreeLink::Local(hp, sn) => {
                        let g = MerkleGraph{ root, graph}; // todo need some way to avoid repeating work here for some node
                        Box::new(ipfs_publish_cata(caps.clone(), hp, *sn))
                    }
                    DagTreeLink::Remote(nh) => Box::new(futures::future::ok(nh)),
                }
            }
        })
        .collect();

    let joined_link_fetches = futures::future::join_all(link_fetches);

    joined_link_fetches
        .and_then(|links: Vec<IPFSHeader>| {
            // might be a bit of an approximation, but w/e
            let size = data.0.len() as u64 + links.iter().map(|x| x.size).sum::<u64>();

            let dag_node = ipfs_types::DagNode { data, links };

            caps.as_ref()
                .ipfs_put(dag_node.clone())
                .then(move |res| match res {
                    Ok(hp) => {
                        caps.as_ref().cache_put(hp.clone(), dag_node);
                        let hdr = IPFSHeader {
                            name: hash.to_string(),
                            hash: hp,
                            size: size,
                        };

                        let chan_send_res = chan.send(Ok(hdr));
                        if let Err(err) = chan_send_res {
                            info!("failed oneshot channel send {:?}", err);
                        };
                        futures::future::ok(())
                    }
                    Err(err) => {
                        let chan_send_res = chan.send(Err(err));
                        if let Err(err) = chan_send_res {
                            info!("failed oneshot channel send {:?}", err);
                        };
                        futures::future::ok(())
                    }
                })
        })
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_types::DagCacheError;
    use crate::cache::{CacheCapability, HasCacheCap};
    use crate::encoding_types::{Base58, Base64};
    use crate::DagTreeLink::Local;
    use crate::ipfs_api::{HasIPFSCap, IPFSCapability};
    use crate::ipfs_types::{DagNode, IPFSHash};
    use crate::lib;
    use rand;
    use rand::Rng;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockIPFS(Mutex<HashMap<IPFSHash, DagNode>>);

    struct BlackHoleCache;
    impl CacheCapability for BlackHoleCache {
        fn get(&self, _k: IPFSHash) -> Option<DagNode> {
            None
        }

        fn put(&self, _k: IPFSHash, _v: DagNode) {}
    }

    // TODO: separate read/write caps to simplify writing this?
    // TODO: would probably be easier to mock out if this was a closure instead of a trait
    impl IPFSCapability for MockIPFS {
        fn get(&self, k: IPFSHash) -> BoxFuture<DagNode, DagCacheError> {
            let map = self.0.lock().unwrap();
            let v = map.get(&k).unwrap(); // fail if not found in map
            Box::new(futures::future::ok(v.clone()))
        }

        fn put(&self, v: DagNode) -> BoxFuture<IPFSHash, DagCacheError> {
            let mut random_bytes = vec![];

            // not hitting the actual IPFS daemon API here so hashes don't need to be even a little bit valid
            let mut rng = rand::thread_rng(); // faster if cached locally
            for _ in 0..64 {
                random_bytes.push(rng.gen())
            }

            let random_hash = IPFSHash::from_raw(Base58::from_bytes(random_bytes));

            let mut map = self.0.lock().unwrap();
            map.insert(random_hash.clone(), v);

            Box::new(futures::future::ok(random_hash))
        }
    }

    struct TestCaps(MockIPFS, BlackHoleCache);

    impl HasIPFSCap for TestCaps {
        type Output = MockIPFS;
        fn ipfs_caps(&self) -> &MockIPFS {
            &self.0
        }
    }

    impl HasCacheCap for TestCaps {
        type Output = BlackHoleCache;
        fn cache_caps(&self) -> &BlackHoleCache {
            &self.1
        }
    }

    #[test]
    fn test_batch_upload() {
        lib::run_test(test_batch_upload_worker)
    }

    // uses mock capabilities, does not require local ipfs daemon
    fn test_batch_upload_worker() -> BoxFuture<(), String> {
        //build some client side 'hashes' - base58 of 1, 2, 3, 4
        let client_hashes: Vec<ClientSideHash> = (1..=4)
            .map(|x| ClientSideHash::new(Base58::from_bytes(vec![x])))
            .collect();

        let t1 = DagTree {
            links: vec![],
            data: Base64(vec![1, 3, 3, 7]),
        };

        let t2 = DagTree {
            links: vec![],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t3 = DagTree {
            links: vec![
                Local(client_hashes[0].clone(), Box::new(t1.clone())),
                Local(client_hashes[1].clone(), Box::new(t2.clone())),
            ],
            data: Base64(vec![3, 1, 4, 1, 5]),
        };

        let t4 = DagTree {
            links: vec![Local(client_hashes[2].clone(), Box::new(t3.clone()))],
            data: Base64(vec![0, 1, 1, 2, 3, 5]),
        };

        let mock_ipfs = MockIPFS(Mutex::new(HashMap::new()));
        let caps = std::sync::Arc::new(TestCaps(mock_ipfs, BlackHoleCache));
        let caps2 = caps.clone();

        let f = ipfs_publish_cata(caps, client_hashes[3].clone(), t4.clone())
            .map_err(|e| format!("ipfs publish cata error: {:?}", e))
            .map(move |_| {
                let map = (caps2.0).0.lock().unwrap();

                let uploaded_values: Vec<(Vec<ClientSideHash>, Base64)> = map
                    .values()
                    .map(|DagNode { links, data }| {
                        (
                            links
                                .iter()
                                .map(|x| ClientSideHash::new(Base58::from_string(&x.name).unwrap()))
                                .collect(),
                            data.clone(),
                        )
                    })
                    .collect();

                assert!(&uploaded_values.contains(&(vec!(), t1.data))); // t1 uploaded
                assert!(&uploaded_values.contains(&(vec!(), t2.data))); // t2 uploaded
                assert!(&uploaded_values.contains(&(
                    vec!(client_hashes[0].clone(), client_hashes[1].clone()),
                    t3.data
                ))); // t3 uploaded
                assert!(&uploaded_values.contains(&(vec!(client_hashes[2].clone()), t4.data))); // t4 uploaded
            });

        Box::new(f)
    }

}
