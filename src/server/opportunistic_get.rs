use crate::capabilities::lib::get_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::types::api as api_types;
use crate::types::errors::DagCacheError;
use crate::types::ipfs as ipfs_types;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::info;
use tracing_futures::Instrument;

pub async fn get<C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Send + Sync>(
    caps: Arc<C>,
    k: ipfs_types::IPFSHash,
) -> Result<api_types::get::Resp, DagCacheError> {
    let f = async {
        let dag_node = get_and_cache(caps.clone(), k.clone()).await?;

        // use cache to extend DAG node by following links as long as they exist in-memory
        let extended = extend(caps.as_ref(), dag_node);

        Ok(extended)
    };

    f.instrument(tracing::info_span!("opportunistic-get")).await
}

// TODO: figure out traversal termination strategy - don't want to return whole cache in one resp
fn extend<C: 'static + HasCacheCap>(caps: &C, node: ipfs_types::DagNode) -> api_types::get::Resp {
    let mut frontier = VecDeque::new();
    let mut res = Vec::new();

    for hp in node.links.iter() {
        // iter over ref
        frontier.push_back(hp.clone());
    }

    // explore the frontier of potentially cached hash pointers
    while let Some(hp) = frontier.pop_front() {
        // if a hash pointer is in the cache, grab the associated node and continue traversal
        if let Some(dn) = caps.cache_get(hp.hash.clone()) {
            // clone :(
            for hp in dn.links.iter() {
                // iter over ref
                frontier.push_back(hp.clone());
            }
            info!(
                "add node with hash {:?} to opportunistic get result",
                hp.clone()
            );
            res.push(ipfs_types::DagNodeWithHeader {
                header: hp,
                node: dn,
            });
        }
    }

    api_types::get::Resp {
        requested_node: node,
        extra_node_count: res.len() as u64,
        extra_nodes: res,
    }
}
