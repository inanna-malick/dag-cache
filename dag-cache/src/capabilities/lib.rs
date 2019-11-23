use crate::capabilities::{HasCacheCap, HasIPFSCap};
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;
use tracing::info;
use tracing::instrument;

#[instrument(skip(caps))]
pub async fn get_and_cache<C: HasCacheCap + HasIPFSCap + Send + Sync + 'static>(
    caps: &C,
    hash: ipfs::IPFSHash,
) -> Result<ipfs::DagNode, DagCacheError> {
    match caps.cache_get(hash.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            Ok(dag_node)
        }
        None => {
            info!("cache miss");

            let dag_node = caps.ipfs_get(hash.clone()).await?;

            info!("writing result of post cache miss lookup to cache");
            caps.cache_put(hash.clone(), dag_node.clone());

            Ok(dag_node)
        }
    }
}

#[instrument(skip(caps, node))]
pub async fn put_and_cache<C: HasCacheCap + HasIPFSCap + Send + Sync + 'static>(
    caps: &C,
    node: ipfs::DagNode,
) -> Result<ipfs::IPFSHash, DagCacheError> {
    let hash = caps.ipfs_put(node.clone()).await?;

    caps.cache_put(hash.clone(), node);

    Ok(hash)
}
