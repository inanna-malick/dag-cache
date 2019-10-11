use crate::capabilities::{Event, HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use std::sync::Arc;
use tracing::info;

pub async fn get_and_cache<
    C: HasCacheCap + HasIPFSCap + HasTelemetryCap + Sync + Send + 'static,
>(
    caps: Arc<C>,
    k: ipfs::IPFSHash,
) -> Result<ipfs::DagNode, DagCacheError> {
    match caps.cache_get(k.clone()) {
        Some(dag_node) => {
            info!("cache hit"); // todo: move this event-logging code to telemetry capability
                                // todo: unify w/ tracing span/event code via suscriber
            caps.report_telemetry(Event::CacheHit(k));
            Ok(dag_node)
        }
        None => {
            info!("cache miss"); // todo: move this event-logging code to telemetry capability
                                 // todo: unify w/ tracing span/event code via suscriber
            caps.report_telemetry(Event::CacheMiss(k.clone()));

            let dag_node = caps.ipfs_get(k.clone()).await?;

            info!("writing result of post cache miss lookup to cache");
            caps.cache_put(k.clone(), dag_node.clone());
            caps.report_telemetry(Event::CachePut(k));

            Ok(dag_node)
        }
    }
}

pub async fn put_and_cache<
    C: HasCacheCap + HasIPFSCap + HasTelemetryCap + Sync + Send + 'static,
>(
    caps: Arc<C>,
    node: ipfs::DagNode,
) -> Result<ipfs::IPFSHash, DagCacheError> {
    let hash = caps.ipfs_put(node.clone()).await?;

    caps.cache_put(hash.clone(), node);

    Ok(hash)
}
