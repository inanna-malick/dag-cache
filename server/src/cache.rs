use crate::ipfs_types::{DagNode, IPFSHash};
use crate::lib::BoxFuture;
use crate::api_types::DagCacheError;
use crate::ipfs_api::IPFSCapability;
use lru::LruCache;
use std::sync::Mutex;

// for adding caching to an IPFS capability
pub struct Cached<X>(Mutex<LruCache<IPFSHash, DagNode>>, X);

impl<X> Cached<X> {
    pub fn new(cache: LruCache<IPFSHash, DagNode>, x: X) -> Self {
        Cached(Mutex::new(cache), x)
    }
}

impl<X: IPFSCapability> IPFSCapability for Cached<X> {
    fn get(&self, k: IPFSHash) -> BoxFuture<DagNode, DagCacheError> {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut cache = self.0.lock().unwrap();
        match cache.get(&k) {
            Some(v) => {
                // TODO: rain says investigate stable deref (given that all refs here are immutable)
                futures::future::ok(v.cloned())
            }
            None => self.1.get(k),
        }
    }

    fn put(&self, node: DagNode) -> BoxFuture<IPFSHash, DagCacheError> {
        let node2 = node.clone();
        let f = self.1.put(node).map(|hash| {
            // succeed or die. failure is unrecoverable (mutex poisoned)
            let mut cache = self.0.lock().unwrap();
            cache.put(hash.clone(), node2);
            hash
        });

        Box::new(f)
    }
}
