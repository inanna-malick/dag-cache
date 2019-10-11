use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap, Event};
use crate::lib::BoxFuture;
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use futures::future::Future;
use std::sync::Arc;
use tracing::info;


pub fn get_and_cache<C: HasCacheCap + HasIPFSCap + HasTelemetryCap + Sync + Send + 'static>(
    caps: Arc<C>,
    k: ipfs::IPFSHash,
) -> BoxFuture<ipfs::DagNode, DagCacheError> {

    match caps.cache_get(k.clone()) {
        Some(dag_node) => {
            info!("cache hit"); // todo: move this event-logging code to telemetry capability
            // todo: unify w/ tracing span/event code via suscriber
            caps.report_telemetry(Event::CacheHit(k));
            Box::new(futures::future::ok(dag_node))
        }
        None => {
            info!("cache miss"); // todo: move this event-logging code to telemetry capability
            // todo: unify w/ tracing span/event code via suscriber
            caps.report_telemetry(Event::CacheMiss(k.clone()));
            let f = caps
                .ipfs_get(k.clone())
                .map(move |dag_node: ipfs::DagNode| {
                    info!("writing result of post cache miss lookup to cache");
                    caps.cache_put(k, dag_node.clone());
                    dag_node
                });
            Box::new(f)
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
