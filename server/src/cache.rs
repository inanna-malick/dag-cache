use crate::ipfs_types::{DagNode, IPFSHash};
use lru::LruCache;
use std::sync::Mutex;

pub trait CacheCapability {
    fn get(&self, k: IPFSHash) -> Option<DagNode>;

    fn put(&self, k: IPFSHash, v: DagNode);

    // fn depth_first_search<
    //     X,
    //     F: Fn(X, IPFSHash, &DagNode) -> (X, DFSControl),
    //     I: IntoIterator<Item = IPFSHash>,
    // >(
    //     &self,
    //     start: IPFSHash,
    //     seed: X,
    //     f: F,
    // ) -> X;
}

pub trait HasCacheCap {
    type Output: CacheCapability;

    fn cache_caps(&self) -> &Self::Output;

    fn cache_get(&self, k: IPFSHash) -> Option<DagNode> {
        self.cache_caps().get(k)
    }

    fn cache_put(&self, k: IPFSHash, v: DagNode) {
        self.cache_caps().put(k, v)
    }
}

pub struct Cache(Mutex<LruCache<IPFSHash, DagNode>>);

impl Cache {
    pub fn new(cache: LruCache<IPFSHash, DagNode>) -> Self {
        Cache(Mutex::new(cache))
    }
}

pub enum DFSControl {
    Continue,
    Break,
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

    // fn depth_first_search<
    //         X,
    //     F: Fn(X, IPFSHash, &DagNode) -> (X, DFSControl),
    //     I: IntoIterator<Item = IPFSHash>,
    //     >(
    //     &self,
    //     start: IPFSHash,
    //     seed: X,
    //     f: F,
    // ) -> X {
    //     let mut cache = self.0.lock().unwrap();

    //     let mut seed = seed; // better names for both of these pls
    //     let mut start = start;

    //     while let Some(node) = cache.get(&start) {
    //         let (next_seed, control) = f(seed, start, node);
    //         seed = next_seed;
    //         match control {
    //             Continue => {}
    //             Break => { break; }
    //         }
    //     }

    //     seed // result of fold - needs better name..
    // }
}
