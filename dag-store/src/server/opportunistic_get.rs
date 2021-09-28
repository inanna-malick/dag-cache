use crate::capabilities::get_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use dag_store_types::types::api;
use dag_store_types::types::domain::{Hash, Node, NodeWithHeader};
use dag_store_types::types::errors::DagCacheError;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::info;
use tracing::instrument;

#[instrument(skip(store, cache, k))]
pub async fn get<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    k: Hash,
) -> Result<api::get::Resp, DagCacheError> {
    let dag_node = get_and_cache(store, cache, k).await?;

    // use cache to extend DAG node by following links as long as they exist in-memory
    let extended = extend(cache, dag_node, 4);

    Ok(extended)
}

// TODO: response size-based traversal termination strategy - eg pack to next X
fn extend<'a>(cache: &'a Arc<Cache>, requested_node: Node, max_nodes: usize) -> api::get::Resp {
    let mut frontier = VecDeque::new();
    let mut extra_nodes = Vec::new();

    for hp in requested_node.links.iter() {
        if frontier.len() + extra_nodes.len() < max_nodes -1 {
            frontier.push_back(hp.clone());
        }
    }

    // explore the frontier of potentially cached hash pointers
    while let Some(header) = frontier.pop_front() {
        // if a hash pointer is in the cache, grab the associated node and continue traversal
        if let Some(node) = cache.get(&header.hash) {
            // clone :(
            for hp in node.links.iter() {
                if frontier.len() + extra_nodes.len() < max_nodes -1 {
                    frontier.push_back(hp.clone());
                }
            }
            info!(
                "add node with hash {:?} to opportunistic get result",
                header
            );
            extra_nodes.push(NodeWithHeader {
                header,
                node,
            });
        }
    }

    api::get::Resp {
        requested_node,
        extra_nodes,
    }
}
