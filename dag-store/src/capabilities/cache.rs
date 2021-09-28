use dag_store_types::types::domain::{Hash, Node};
use lru::LruCache;
use std::sync::Mutex;

pub struct Cache(pub Mutex<LruCache<Hash, Node>>);

impl Cache {
    pub fn new(max_cache_entries: usize) -> Self {
        let cache = LruCache::new(max_cache_entries);
        // TODO: use RW lock instead, probably
        Cache(Mutex::new(cache))
    }

    // TODO: rain says investigate stable deref (given that all refs here are immutable)
    pub fn get(&self, k: &Hash) -> Option<Node> {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut cache = self.0.lock().unwrap();
        let mv = cache.get(k);
        mv.cloned() // this feels weird? clone(d) is actually needed, right?
    }

    pub fn put(&self, k: Hash, v: Node) {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut cache = self.0.lock().unwrap();
        cache.put(k, v);
    }
}
