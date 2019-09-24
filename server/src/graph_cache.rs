/// caches graph structure. never performs evictions.
use crate::ipfs_types::{IPFSHash};
use petgraph::graph::Graph;
use petgraph::{graph, Direction, Directed};
use petgraph::visit::EdgeRef;
use std::sync::Mutex;

// note: subnodes is partial, no fallback to network or w/e - really more like get_known_subnodes
pub trait GraphCacheCapability {
    fn get_subnodes(&self, k: IPFSHash) -> Vec<IPFSHash>;

    fn put_node(&self, node: IPFSHash, links: Vec<IPFSHash>);
}

pub trait HasGraphCacheCap {
    type Output: GraphCacheCapability;

    fn graph_cache_caps(&self) -> &Self::Output;

    fn graph_cache_get_subnodes(&self, k: IPFSHash) -> Vec<IPFSHash> {
        self.cache_caps().get_subnodes(k)
    }

    fn graph_cache_put_node(&self, k: IPFSHash, links: Vec<IPFSHash>) {
        self.cache_caps().put_node(k, links)
    }
}

// todo mb cache ipfs headers as edges..
pub struct GraphCache(Mutex<Graph<IPFSHash, (), Directed>>);
