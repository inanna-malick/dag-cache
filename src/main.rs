use actix;
use actix_web::{http, web, App, HttpServer};
use futures::future;
use futures::future::Future;
use lru::LruCache;
use std::sync::Mutex;

use std::collections::VecDeque;

mod types;
use crate::types::{DagCacheError, DagNode, DagNodeGetResp, HashPointer, IPFSPutResp};

mod ipfs_io;

use tracing::{info, span, Level};

type Cache = Mutex<LruCache<HashPointer, DagNode>>;

struct State {
    cache: Cache,
    ipfs_node: ipfs_io::IPFSNode,
}

// TODO: rain says investigate stable deref (given that all refs here are immutable)
fn cache_get(mutex: &Cache, k: HashPointer) -> Option<DagNode> {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    // let mv = cache.get(&k);
    // mv.cloned() // this feels weird? clone(d) is actually needed, right?
    let mv = cache.get(&k);
    mv.cloned() // this feels weird? clone(d) is actually needed, right?
}

fn cache_put(mutex: &Cache, k: HashPointer, v: DagNode) {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    cache.put(k, v);
}

fn get(
    data: web::Data<State>,
    k: web::Path<(HashPointer)>,
) -> Box<dyn Future<Item = web::Json<DagNodeGetResp>, Error = DagCacheError>> {
    let span = span!(Level::TRACE, "dag cache get handler");
    let _enter = span.enter();
    info!("attempt cache get");
    let k = k.into_inner();
    match cache_get(&data.cache, k.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            // see if have any of the referenced subnodes in the local cache
            let resp = extend(&data.cache, (k, dag_node));
            Box::new(future::ok(web::Json(resp)))
        }
        None => {
            info!("cache miss");
            let f = data
                .ipfs_node
                .get(k.clone())
                .and_then(move |dag_node: DagNode| {
                    info!("writing result of post cache miss lookup to cache");
                    cache_put(&data.cache, k.clone(), dag_node.clone());
                    // see if have any of the referenced subnodes in the local cache
                    let resp = extend(&data.cache, (k, dag_node));
                    Ok(web::Json(resp))
                });
            Box::new(f)
        }
    }
}


// TODO: figure out traversal termination strategy - don't want to return whole cache in one resp (or do I?)
// NOTE: breadth first first, probably.. sounds good.
fn extend(cache: &Cache, node: (HashPointer, DagNode)) -> DagNodeGetResp {
    let mut frontier = VecDeque::new();
    let mut res = Vec::new();

    for hp in node.1.links.iter() {
        // iter over ref
        frontier.push_back(hp.hash.clone()); // clone :(
    }

    // explore the frontier of potentially cached hash pointers
    while let Some(hp) = frontier.pop_front() {
        // if a hash pointer is in the cache, grab the associated node and continue traversal
        if let Some(dn) = cache_get(cache, hp.clone()) {
            // clone :(
            for hp in dn.links.iter() {
                // iter over ref
                frontier.push_back(hp.hash.clone()); // clone :(
            }
            res.push((hp, dn));
        }
    }

    // NEL-like structure
    DagNodeGetResp {
        requested_node: node,
        extra_node_count: res.len(),
        extra_nodes: res,
    }
}

fn put(
    data: web::Data<State>,
    v: web::Json<DagNode>,
) -> Box<dyn Future<Item = web::Json<IPFSPutResp>, Error = DagCacheError>> {
    info!("dag cache put handler");
    let v = v.into_inner();

    let f = data
        .ipfs_node
        .put(v.clone())
        .and_then(move |hp: HashPointer| {
            cache_put(&data.cache, hp.clone(), v);
            Ok(web::Json(IPFSPutResp { hash: hp }))
        });
    Box::new(f)
}

fn main() -> Result<(), std::io::Error> {
    // PROBLEM: provisioning based on number of entities and _not_ number of bytes allocated total
    //          some dag nodes may be small and some may be large.
    let sys = actix::System::new("system"); // <- create Actix system

    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let cache = LruCache::new(32); // TODO: config, sensible defaults, etc

    let state = State {
        cache: Mutex::new(cache),
        ipfs_node: ipfs_io::IPFSNode::new(http::uri::Authority::from_static("localhost:5001")),
    };
    let data = web::Data::new(state);

    HttpServer::new(move || {
        println!("init app");
        App::new()
            .register_data(data.clone()) // <- register the created data
            .route("/get/{n}", web::get().to_async(get))
            .route("/put", web::post().to_async(put))
    })
    .bind("127.0.0.1:8088")
    .expect("Can not bind to 127.0.0.1:8088")
    .start();

    // Run actix system (actually starts all async processes, presumably blocks(?))
    sys.run()
}
