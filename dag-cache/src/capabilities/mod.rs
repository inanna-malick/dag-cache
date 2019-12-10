pub mod cache;
pub mod lib;
pub mod store;
pub use crate::capabilities::cache::Cache;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash};

// TODO: make actual hashing constant via fn on dag_cache, can simplify batch put & everything
// dag node store (TODO: rename)
#[tonic::async_trait]
pub trait HashedBlobStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: IPFSHash) -> Result<DagNode, DagCacheError>;
    async fn put(&self, v: DagNode) -> Result<IPFSHash, DagCacheError>;
}

// used to store key->hash mappings for CAS use
#[tonic::async_trait]
pub trait MutableHashStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: &str) -> Result<Option<IPFSHash>, DagCacheError>;
    async fn cas(
        &self,
        k: &str,
        previous_hash: Option<IPFSHash>,
        proposed_hash: IPFSHash,
    ) -> Result<(), DagCacheError>;
}
