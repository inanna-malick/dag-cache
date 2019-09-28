pub mod ipfs_store;
pub mod lru_cache;
pub mod runtime;

use crate::lib::BoxFuture;
use crate::types::errors::DagCacheError;
use crate::types::ipfs;

// remote node store
pub trait IPFSCapability {
    fn get(&self, k: ipfs::IPFSHash) -> BoxFuture<ipfs::DagNode, DagCacheError>;
    fn put(&self, v: ipfs::DagNode) -> BoxFuture<ipfs::IPFSHash, DagCacheError>;
}

pub trait HasIPFSCap {
    type Output: IPFSCapability;

    fn ipfs_caps(&self) -> &Self::Output;

    fn ipfs_get(&self, k: ipfs::IPFSHash) -> BoxFuture<ipfs::DagNode, DagCacheError> {
        self.ipfs_caps().get(k)
    }

    fn ipfs_put(&self, v: ipfs::DagNode) -> BoxFuture<ipfs::IPFSHash, DagCacheError> {
        self.ipfs_caps().put(v)
    }
}

/// process-local cache capability
pub trait CacheCapability {
    fn get(&self, k: ipfs::IPFSHash) -> Option<ipfs::DagNode>;

    fn put(&self, k: ipfs::IPFSHash, v: ipfs::DagNode);
}

pub trait HasCacheCap {
    type Output: CacheCapability;

    fn cache_caps(&self) -> &Self::Output;

    fn cache_get(&self, k: ipfs::IPFSHash) -> Option<ipfs::DagNode> { self.cache_caps().get(k) }

    fn cache_put(&self, k: ipfs::IPFSHash, v: ipfs::DagNode) { self.cache_caps().put(k, v) }
}
