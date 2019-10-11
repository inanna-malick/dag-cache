use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::telemetry::Telemetry;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};

pub struct Runtime {
    pub telemetry: Telemetry,
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

impl HasTelemetryCap for Runtime {
    type Output = Telemetry;
    fn telemetry_caps(&self) -> &Telemetry { &self.telemetry }
}
