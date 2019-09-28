use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::HasCacheCap;
use crate::capabilities::HasIPFSCap;

pub struct Runtime {
    pub cache: Cache,
    pub ipfs_node: IPFSNode,
}

//todo: read up on generalized associated types
impl HasIPFSCap for Runtime {
    type Output = IPFSNode;
    fn ipfs_caps(&self) -> &IPFSNode { &self.ipfs_node }
}

impl HasCacheCap for Runtime {
    type Output = Cache;
    fn cache_caps(&self) -> &Cache { &self.cache }
}
