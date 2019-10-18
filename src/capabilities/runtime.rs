use crate::capabilities::telemetry::Telemetry;
use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap};

pub struct Runtime(pub Telemetry, pub RuntimeCaps);

pub struct RuntimeCaps {
    pub cache: Cache,
    pub ipfs_node: IPFSNode,
}

//todo: read up on generalized associated types
impl HasIPFSCap for RuntimeCaps {
    type Output = IPFSNode;
    fn ipfs_caps(&self) -> &IPFSNode { &self.ipfs_node }
}

impl HasCacheCap for RuntimeCaps {
    type Output = Cache;
    fn cache_caps(&self) -> &Cache { &self.cache }
}
