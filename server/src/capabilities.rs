use crate::cache::{Cache, CacheCapability};
use crate::ipfs_api::{IPFSNode, IPFSCapability};

pub struct Capabilities {
    cache: Cache,
    ipfs_node: IPFSNode,
}

impl Capabilities {
    pub fn new(cache: Cache, ipfs_node: IPFSNode) -> Capabilities {
        Capabilities{cache, ipfs_node}
    }
}

pub trait HasIPFSCap {
    type Output: IPFSCapability;

    fn ipfs_caps(&self) -> &Self::Output;
}

//todo: read up on generalized associated types
impl HasIPFSCap for Capabilities {
    type Output = IPFSNode;
    fn ipfs_caps(&self) -> &IPFSNode {
        &self.ipfs_node
    }
}

pub trait HasCacheCap {
    type Output: CacheCapability;

    fn cache_caps(&self) -> &Self::Output;
}


impl HasCacheCap for Capabilities {
    type Output = Cache;
    fn cache_caps(&self) -> &Cache {
        &self.cache
    }
}
