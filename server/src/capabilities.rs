use crate::cache::{Cache, HasCacheCap};
use crate::ipfs_api::{HasIPFSCap, IPFSNode};
use lru::LruCache;

pub struct Capabilities {
    cache: Cache,
    ipfs_node: IPFSNode,
}

impl Capabilities {
    pub fn new(max_cache_entries: usize, ipfs_node: IPFSNode) -> Capabilities {
        let cache = Cache::new(LruCache::new(max_cache_entries));
        Capabilities { cache, ipfs_node }
    }
}

//todo: read up on generalized associated types
impl HasIPFSCap for Capabilities {
    type Output = IPFSNode;
    fn ipfs_caps(&self) -> &IPFSNode {
        &self.ipfs_node
    }
}

impl HasCacheCap for Capabilities {
    type Output = Cache;
    fn cache_caps(&self) -> &Cache {
        &self.cache
    }
}
