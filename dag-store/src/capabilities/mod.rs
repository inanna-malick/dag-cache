pub mod cache;
pub mod store;
pub use crate::capabilities::cache::Cache;
use dag_store_types::types::domain::{Hash, Node};
use dag_store_types::types::errors::DagCacheError;
use std::sync::Arc;
use tracing::info;
use tracing::instrument;

// TODO: make actual hashing constant via fn on dag_cache, can simplify batch put & everything
// dag node store (TODO: rename)
#[tonic::async_trait]
pub trait HashedBlobStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: Hash) -> Result<Node, DagCacheError>;
    async fn put(&self, v: Node) -> Result<Hash, DagCacheError>;
}

// used to store key->hash mappings for CAS use
#[tonic::async_trait]
pub trait MutableHashStore
where
    Self: Send + Sync,
{
    async fn get(&self, k: &str) -> Result<Option<Hash>, DagCacheError>;
    async fn cas(
        &self,
        k: &str,
        previous_hash: Option<Hash>,
        proposed_hash: Hash,
    ) -> Result<(), DagCacheError>;
}

#[instrument(skip(store, cache))]
pub async fn get_and_cache<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    hash: Hash,
) -> Result<Node, DagCacheError> {
    match cache.get(&hash) {
        Some(dag_node) => {
            info!("cache hit");
            Ok(dag_node)
        }
        None => {
            info!("cache miss");

            let dag_node = store.get(hash.clone()).await?;

            info!("writing result of post cache miss lookup to cache");
            cache.put(hash.clone(), dag_node.clone());

            Ok(dag_node)
        }
    }
}

#[instrument(skip(store, cache, node))]
pub async fn put_and_cache<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    node: Node,
) -> Result<Hash, DagCacheError> {
    let hash = store.put(node.clone()).await?;

    cache.put(hash.clone(), node);

    Ok(hash)
}
