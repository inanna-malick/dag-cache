use actix_web::web;
use futures::future;
use futures::future::Future;

use std::collections::VecDeque;

use crate::api_types;
use crate::in_mem_types;
use crate::ipfs_types;

use crate::cache::HasCacheCap;
use crate::ipfs_api::HasIPFSCap;
use crate::lib::BoxFuture;
use tracing::{info, span, Level};

use crate::batch_upload;

pub fn get<C: 'static + HasIPFSCap + HasCacheCap>(
    caps: web::Data<C>,
    k: web::Path<(ipfs_types::IPFSHash)>,
) -> Box<dyn Future<Item = web::Json<api_types::get::Resp>, Error = api_types::DagCacheError>> {
    let caps = caps.into_inner();

    let span = span!(Level::TRACE, "dag cache get handler");
    let _enter = span.enter();
    info!("attempt cache get");
    let k = k.into_inner();
    match caps.cache_get(k.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            // see if have any of the referenced subnodes in the local cache
            let resp = extend(caps.as_ref(), dag_node);
            Box::new(future::ok(web::Json(resp)))
        }
        None => {
            info!("cache miss");
            let f =
                caps
                .ipfs_get(k.clone())
                    .and_then(move |dag_node: ipfs_types::DagNode| {
                        info!("writing result of post cache miss lookup to cache");
                        caps.cache_put(k.clone(), dag_node.clone());
                        // see if have any of the referenced subnodes in the local cache
                        let resp = extend(caps.as_ref(), dag_node);
                        Ok(web::Json(resp))
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
        extra_node_count: res.len(),
        extra_nodes: res,
    }
}

pub fn put<C: 'static + HasCacheCap + HasIPFSCap>(
    caps: web::Data<C>,
    node: web::Json<ipfs_types::DagNode>,
) -> Box<dyn Future<Item = web::Json<ipfs_types::IPFSHash>, Error = api_types::DagCacheError>> {
    info!("dag cache put handler");
    let node = node.into_inner();

    let f = caps
        .ipfs_put(node.clone())
        .and_then(move |hp: ipfs_types::IPFSHash| {
            caps.cache_put(hp.clone(), node);
            Ok(web::Json(hp))
        });
    Box::new(f)
}

pub fn put_many<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    // TODO: figure out exactly what sync does
    caps: web::Data<C>,
    req: web::Json<api_types::bulk_put::Req>,
) -> BoxFuture<web::Json<ipfs_types::IPFSHeader>, api_types::DagCacheError> {
    // let caps = caps.into_inner(); // just copy arc, lmao

    info!("dag cache put handler");
    let api_types::bulk_put::Req { entry_point, nodes } = req.into_inner();
    let (csh, dctp) = entry_point;

    let mut node_map = std::collections::HashMap::with_capacity(nodes.len());

    for (k, v) in nodes.into_iter() {
        node_map.insert(k, v);
    }

    let in_mem = in_mem_types::DagNode::build(dctp, &mut node_map)
        .expect("todo: handle malformed req case here"); // FIXME

    let f = batch_upload::ipfs_publish_cata(caps.into_inner(), csh, in_mem).map(web::Json);

    Box::new(f)
}
