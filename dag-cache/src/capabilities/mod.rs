pub mod fs_ipfs_store;
pub mod ipfs_store;
pub mod lib;
pub mod lru_cache;
pub mod runtime;

use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;

// remote node store
#[tonic::async_trait]
pub trait IPFSCapability
where
    Self: std::marker::Send,
{
    async fn get(&self, k: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError>;
    async fn put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError>;
}

#[tonic::async_trait]
pub trait HasIPFSCap
where
    Self: std::marker::Send,
{
    type Output: IPFSCapability + Sync;

    fn ipfs_caps(&self) -> &Self::Output;

    async fn ipfs_get(&self, k: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError> {
        self.ipfs_caps().get(k).await
    }

    async fn ipfs_put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError> {
        self.ipfs_caps().put(v).await
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

    fn cache_get(&self, k: ipfs::IPFSHash) -> Option<ipfs::DagNode> {
        self.cache_caps().get(k)
    }

    fn cache_put(&self, k: ipfs::IPFSHash, v: ipfs::DagNode) {
        self.cache_caps().put(k, v)
    }
}
