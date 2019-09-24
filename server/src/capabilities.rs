use crate::graph_cache::{HasGraphCacheCap, GraphCache};
use crate::cache::Cached;
use crate::ipfs_api::{HasIPFSCap, IPFSNode};

pub struct Capabilities {
    graph_cache: Cached<IPFSNode>,
    ipfs_node: Cached<IPFSNode>,
}

impl Capabilities {
    pub fn new(graph_cache: GraphCache, ipfs_node: Cached<IPFSNode>) -> Capabilities {
        Capabilities { graph_cache, ipfs_node }
    }
}

//todo: read up on generalized associated types
impl HasIPFSCap for Capabilities {
    type Output = IPFSNode;
    fn ipfs_caps(&self) -> &IPFSNode {
        &self.ipfs_node
    }
}

impl HasGraphCacheCap for Capabilities {
    type Output = GraphCache;
    fn graph_cache_caps(&self) -> &Cache {
        &self.graph_cache
    }
}
