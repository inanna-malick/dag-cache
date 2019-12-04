pub mod fs_store;
pub mod ipfs_store;
pub mod lib;
pub mod lru_cache;
pub mod runtime;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;

// dag node store (TODO: rename)
#[tonic::async_trait]
pub trait IPFSCapability
where
    Self: std::marker::Send,
{
    async fn get(&self, k: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError>;
    async fn put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError>;
}

// used to store key->hash mappings for CAS use
#[tonic::async_trait]
pub trait MutableHashStore
where
    Self: std::marker::Send,
{
    async fn get(&self, k: String) -> Result<Option<ipfs::IPFSHash>, DagCacheError>;
    async fn put(
        &self,
        k: String,
        hash: ipfs::IPFSHash,
    ) -> Result<Option<ipfs::IPFSHash>, DagCacheError>;
}

#[tonic::async_trait]
pub trait HasMutableHashStore
where
    Self: std::marker::Send,
{
    type Output: MutableHashStore + Sync;

    fn mhs_caps(&self) -> &Self::Output;

    async fn mhs_get(&self, k: String) -> Result<Option<ipfs::IPFSHash>, DagCacheError> {
        self.mhs_caps().get(k).await
    }
    async fn mhs_put(
        &self,
        k: String,
        hash: ipfs::IPFSHash,
    ) -> Result<Option<ipfs::IPFSHash>, DagCacheError> {
        self.mhs_caps().put(k, hash).await
    }
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


/// TODO: always one impl, consider dropping
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
