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
use crate::lib::BoxFuture;
use std::convert::AsRef;
use tracing::info;

use crate::in_mem_types::ValidatedTree;

use futures::sync::oneshot;

// catamorphism - a consuming change
// recursively publish DAG node tree to IPFS, starting with leaf nodes
pub fn ipfs_publish_cata<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: in_mem_types::ValidatedTree,
) -> impl Future<Item = IPFSHeader, Error = api_types::DagCacheError> + 'static + Send {
    // todo use async/await I guess, mb can avoid needing Arc? ugh
    let focus = tree.root.clone();
    let tree = Arc::new(tree);
    ipfs_publish_cata_unsafe(caps, tree, focus)
}

// unsafe b/c it can take any 'focus' ClientSideHash and not just the root node of tree
pub fn ipfs_publish_cata_unsafe<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    tree: Arc<in_mem_types::ValidatedTree>, // todo use async/await I guess, mb can avoid needing Arc? ugh
    focus: ClientSideHash,
) -> impl Future<Item = IPFSHeader, Error = api_types::DagCacheError> + 'static + Send {
    let (send, receive) = oneshot::channel();

    tokio::spawn(ipfs_publish_worker(caps, send, tree, focus));

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
    tree: Arc<ValidatedTree>,
    focus: ClientSideHash,
) -> impl Future<Item = (), Error = ()> + 'static + Send {
    // NOTE: could be more efficient by removing from node but would break guarantees
    // unhandled deref failure, known to be safe b/c of validated tree wrapper
    let api_types::bulk_put::DagNode { data, links } = tree.nodes[&focus].clone();

    let link_fetches: Vec<_> = links
        .into_iter()
        .map({
            |x| -> BoxFuture<IPFSHeader, api_types::DagCacheError> {
                match x {
                    api_types::bulk_put::DagNodeLink::Local(csh) => {
                        Box::new(ipfs_publish_cata_unsafe(caps.clone(), tree.clone(), csh.clone()))
                    }
                    api_types::bulk_put::DagNodeLink::Remote(nh) => {
                        Box::new(futures::future::ok(nh.clone())) // TODO: stable deref via frozen/elsa? yes.
                    }
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
                        caps.as_ref().cache_put(hp.clone(), dag_node.clone());
                        let hdr = IPFSHeader {
                            name: focus.to_string(),
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
    use crate::api_types::bulk_put;
    use crate::cache::{CacheCapability, HasCacheCap};
    use crate::encoding_types::{Base58, Base64};
    use crate::in_mem_types::ValidatedTree;
    use crate::ipfs_api::{HasIPFSCap, IPFSCapability};
    use crate::ipfs_types::{DagNode, IPFSHash};
    use crate::lib;
    use hashbrown::HashMap;
    use std::sync::Mutex;

    struct MockIPFS(Mutex<HashMap<IPFSHash, ipfs_types::DagNode>>);

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
            let mut map = self.0.lock().unwrap(); // fail on mutex poisoned

            // use map len (monotonic increasing) to provide unique hash ID
            let hash = IPFSHash::from_raw(Base58::from_bytes(vec!(map.len() as u8)));

            map.insert(hash.clone(), v);


            Box::new(futures::future::ok(hash))
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
        m.insert(client_hashes[3].clone(), t3.clone());

        let validated_tree = ValidatedTree::validate(client_hashes[3].clone(), m).expect("static test invalid");

        let mock_ipfs = MockIPFS(Mutex::new(HashMap::new()));
        let caps = std::sync::Arc::new(TestCaps(mock_ipfs, BlackHoleCache));
        let caps2 = caps.clone();

        let f = ipfs_publish_cata(caps, validated_tree)
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

                assert!(&uploaded_values.contains(&(vec!(), t0.data))); // t1 uploaded
                assert!(&uploaded_values.contains(&(vec!(), t1.data))); // t2 uploaded
                assert!(&uploaded_values.contains(&(
                    vec!(client_hashes[0].clone(), client_hashes[1].clone()),
                    t2.data
                ))); // t3 uploaded
                assert!(&uploaded_values.contains(&(vec!(client_hashes[2].clone()), t3.data))); // t4 uploaded
            });

        Box::new(f)
    }

}
