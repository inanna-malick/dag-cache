use crate::capabilities::{Event, HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::lib::BoxFuture;
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use futures::compat::Future01CompatExt;
use futures01::future::Future;
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

            let dag_node = caps.ipfs_get(k.clone()).compat().await?;

            info!("writing result of post cache miss lookup to cache");
            caps.cache_put(k.clone(), dag_node.clone());
            caps.report_telemetry(Event::CachePut(k));

            Ok(dag_node)
        }
    }
}

pub fn put_and_cache<C: HasCacheCap + HasIPFSCap + HasTelemetryCap + Sync + Send + 'static>(
    caps: Arc<C>,
    node: ipfs::DagNode,
) -> BoxFuture<ipfs::IPFSHash, DagCacheError> {
    let f = caps.ipfs_put(node.clone()).map(move |hp: ipfs::IPFSHash| {
        caps.cache_put(hp.clone(), node);
        hp
    });

    Box::new(f)
}
