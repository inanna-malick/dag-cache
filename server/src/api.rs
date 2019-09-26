use crate::api_types;
use crate::batch_upload;
use crate::cache::HasCacheCap;
use crate::error_types::DagCacheError;
use crate::in_mem_types::ValidatedTree;
use crate::ipfs_api::HasIPFSCap;
use crate::ipfs_types;
use crate::lib::BoxFuture;
use futures::future;
use futures::future::Future;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::{info, span, Level};

pub fn get<C: 'static + HasIPFSCap + HasCacheCap + Send + Sync>(
    caps: Arc<C>,
    k: ipfs_types::IPFSHash,
) -> BoxFuture<api_types::get::Resp, DagCacheError> {
    let span = span!(Level::TRACE, "dag cache get handler");
    let _enter = span.enter();
    info!("attempt cache get");
    match caps.cache_get(k.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            // see if have any of the referenced subnodes in the local cache
            let resp = extend(caps.as_ref(), dag_node);
            Box::new(future::ok(resp))
        }
        None => {
            info!("cache miss");
            let f = caps
                .ipfs_get(k.clone())
                .and_then(move |dag_node: ipfs_types::DagNode| {
                    info!("writing result of post cache miss lookup to cache");
                    caps.cache_put(k.clone(), dag_node.clone());
                    // see if have any of the referenced subnodes in the local cache
                    let resp = extend(caps.as_ref(), dag_node);
                    Ok(resp)
                });
            Box::new(f)
        }
    }
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

pub fn put<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    node: ipfs_types::DagNode,
) -> BoxFuture<ipfs_types::IPFSHash, DagCacheError> {
    info!("dag cache put handler");

    let f = caps
        .ipfs_put(node.clone())
        .and_then(move |hp: ipfs_types::IPFSHash| {
            caps.cache_put(hp.clone(), node);
            Ok(hp)
        });
    Box::new(f)
}

pub fn put_many<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    req: api_types::bulk_put::Req,
) -> BoxFuture<ipfs_types::IPFSHash, DagCacheError> {
    info!("dag cache put handler");
    let api_types::bulk_put::Req { root_node, nodes } = req;

    let mut node_map = hashbrown::HashMap::with_capacity(nodes.len());

    for api_types::bulk_put::DagNodeWithHash { hash, node } in nodes.into_iter() {
        node_map.insert(hash, node);
    }

    let in_mem =
        ValidatedTree::validate(root_node, node_map).expect("todo: handle malformed req case here"); // FIXME

    let f = batch_upload::ipfs_publish_cata(caps, in_mem).map(|(_size, hash)| hash);

    Box::new(f)
}
