use crate::capabilities::{HasCacheCap, HasIPFSCap};
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use std::sync::Arc;
use tracing::info;

pub async fn get_and_cache<
    C: HasCacheCap + HasIPFSCap + Sync + 'static,
>(
    caps: Arc<C>,
    k: ipfs::IPFSHash,
) -> Result<ipfs::DagNode, DagCacheError> {
    match caps.cache_get(k.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            Ok(dag_node)
        }
        None => {
            info!("cache miss");

            let dag_node = caps.ipfs_get(k.clone()).await?;

            info!("writing result of post cache miss lookup to cache");
            caps.cache_put(k.clone(), dag_node.clone());

            Ok(dag_node)
        }
    }
}

pub async fn put_and_cache<
    C: HasCacheCap + HasIPFSCap + Sync + Send + 'static,
>(
    caps: Arc<C>,
    node: ipfs::DagNode,
) -> Result<ipfs::IPFSHash, DagCacheError> {
    let hash = caps.ipfs_put(node.clone()).await?;

    caps.cache_put(hash.clone(), node);

    Ok(hash)
}
