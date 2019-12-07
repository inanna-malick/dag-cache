pub mod cache;
pub mod lib;
pub mod store;
pub use crate::capabilities::cache::Cache;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;

// dag node store (TODO: rename)
#[tonic::async_trait]
pub trait HashedBlobStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError>;
    async fn put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError>;
}

// used to store key->hash mappings for CAS use
#[tonic::async_trait]
pub trait MutableHashStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: String) -> Result<Option<ipfs::IPFSHash>, DagCacheError>;
    async fn put(
        &self,
        k: String,
        hash: ipfs::IPFSHash,
    ) -> Result<Option<ipfs::IPFSHash>, DagCacheError>;
}
