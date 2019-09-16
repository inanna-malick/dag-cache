use lru::LruCache;
use std::sync::Mutex;

use crate::ipfs_types::{DagNode, IPFSHash};

pub trait CacheCapability {
    fn get(&self, k: IPFSHash) -> Option<DagNode>;

    fn put(&self, k: IPFSHash, v: DagNode);
}

pub struct Cache(Mutex<LruCache<IPFSHash, DagNode>>);

impl Cache {
    pub fn new(cache: LruCache<IPFSHash, DagNode>) -> Self {
        Cache(Mutex::new(cache))
    }
}

impl CacheCapability for Cache {
    // TODO: rain says investigate stable deref (given that all refs here are immutable)
    fn get(&self, k: IPFSHash) -> Option<DagNode> {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut cache = self.0.lock().unwrap();
        let mv = cache.get(&k);
        mv.cloned() // this feels weird? clone(d) is actually needed, right?
    }

    fn put(&self, k: IPFSHash, v: DagNode) {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut cache = self.0.lock().unwrap();
        cache.put(k, v);
    }
}
