use crate::capabilities::{Cache, HashedBlobStore};
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;
use std::sync::Arc;
use tracing::info;
use tracing::instrument;

#[instrument(skip(store, cache))]
pub async fn get_and_cache(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: ipfs::IPFSHash,
) -> Result<ipfs::DagNode, DagCacheError> {
    match cache.get(hash.clone()) {
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
pub async fn put_and_cache(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    node: ipfs::DagNode,
) -> Result<ipfs::IPFSHash, DagCacheError> {
    let hash = store.put(node.clone()).await?;

    cache.put(hash.clone(), node);

    Ok(hash)
}
