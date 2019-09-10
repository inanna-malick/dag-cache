use actix;
use actix_web::{http, web, App, HttpServer};
use futures::future;
use futures::future::Future;
use lru::LruCache;
use std::sync::Mutex;

use std::collections::VecDeque;

mod ipfs_types;
// use crate::ipfs_types;
mod encoding_types;
// use crate::encoding_types;
mod api_types;
use crate::api_types::ClientSideHash;
mod in_mem_types;
// use crate::in_mem_types;

mod ipfs_api;

use tracing::{info, span, Level};

type Cache = Mutex<LruCache<ipfs_types::IPFSHash, ipfs_types::DagNode>>;

struct State {
    cache: Cache,
    ipfs_node: ipfs_api::IPFSNode,
}

// TODO: rain says investigate stable deref (given that all refs here are immutable)
fn cache_get(mutex: &Cache, k: ipfs_types::IPFSHash) -> Option<ipfs_types::DagNode> {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    // let mv = cache.get(&k);
    // mv.cloned() // this feels weird? clone(d) is actually needed, right?
    let mv = cache.get(&k);
    mv.cloned() // this feels weird? clone(d) is actually needed, right?
}

fn cache_put(mutex: &Cache, k: ipfs_types::IPFSHash, v: ipfs_types::DagNode) {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    cache.put(k, v);
}

fn get(
    data: web::Data<State>,
    k: web::Path<(ipfs_types::IPFSHash)>,
) -> Box<dyn Future<Item = web::Json<api_types::get::Resp>, Error = api_types::DagCacheError>> {
    let span = span!(Level::TRACE, "dag cache get handler");
    let _enter = span.enter();
    info!("attempt cache get");
    let k = k.into_inner();
    match cache_get(&data.cache, k.clone()) {
        Some(dag_node) => {
            info!("cache hit");
            // see if have any of the referenced subnodes in the local cache
            let resp = extend(&data.cache, dag_node);
            Box::new(future::ok(web::Json(resp)))
        }
        None => {
            info!("cache miss");
            let f = data
                .ipfs_node
                .get(k.clone())
                .and_then(move |dag_node: ipfs_types::DagNode| {
                    info!("writing result of post cache miss lookup to cache");
                    cache_put(&data.cache, k.clone(), dag_node.clone());
                    // see if have any of the referenced subnodes in the local cache
                    let resp = extend(&data.cache, dag_node);
                    Ok(web::Json(resp))
                });
            Box::new(f)
        }
    }
}

// TODO: figure out traversal termination strategy - don't want to return whole cache in one resp (or do I?)
// NOTE: breadth first first, probably.. sounds good.
fn extend(cache: &Cache, node: ipfs_types::DagNode) -> api_types::get::Resp {
    let mut frontier = VecDeque::new();
    let mut res = Vec::new();

    for hp in node.links.iter() {
        // iter over ref
        frontier.push_back(hp.clone());
    }

    // explore the frontier of potentially cached hash pointers
    while let Some(hp) = frontier.pop_front() {
        // if a hash pointer is in the cache, grab the associated node and continue traversal
        if let Some(dn) = cache_get(cache, hp.hash.clone()) {
            // clone :(
            for hp in dn.links.iter() {
                // iter over ref
                frontier.push_back(hp.clone());
            }
            res.push(ipfs_types::DagNodeWithHash { hash: hp, node: dn });
        }
    }

    // NEL-like structure
    api_types::get::Resp {
        requested_node: node,
        extra_node_count: res.len(),
        extra_nodes: res,
    }
}

fn put(
    data: web::Data<State>,
    v: web::Json<ipfs_types::DagNode>,
) -> Box<dyn Future<Item = web::Json<ipfs_types::IPFSHash>, Error = api_types::DagCacheError>> {
    info!("dag cache put handler");
    let v = v.into_inner();

    let f = data
        .ipfs_node
        .put(v.clone())
        .and_then(move |hp: ipfs_types::IPFSHash| {
            cache_put(&data.cache, hp.clone(), v);
            Ok(web::Json(hp))
        });
    Box::new(f)
}

fn put_many(
    app_data: web::Data<State>,
    v: web::Json<api_types::bulk_put::Req>,
) -> Box<dyn Future<Item = web::Json<ipfs_types::IPFSHeader>, Error = api_types::DagCacheError>> {
    info!("dag cache put handler");
    let api_types::bulk_put::Req { entry_point, nodes } = v.into_inner();
    let (csh, dctp) = entry_point;

    let mut node_map = std::collections::HashMap::with_capacity(nodes.len());

    for (k, v) in nodes.into_iter() {
        node_map.insert(k, v);
    }

    let in_mem = in_mem_types::DagNode::build(dctp, &mut node_map)
        .expect("todo: handle malformed req case here"); // FIXME

    // FIXME: cause of future stack overflow? idk, could use custom future w/ state to avoid recursing on polls
    fn helper(
        app_data: web::Data<State>, // todo: just pass around arc, probably
        csh: ClientSideHash,
        x: in_mem_types::DagNode,
    ) -> Box<dyn Future<Item = ipfs_types::IPFSHeader, Error = api_types::DagCacheError>> {
        let in_mem_types::DagNode { data, links } = x;

        let app_data_prime = app_data.clone();

        let bar: Vec<_> = links
            .into_iter()
            .map({
                |x| match x {
                    in_mem_types::DagNodeLink::Local(hp, sn) => {
                        helper(app_data_prime.clone(), hp, *sn)
                    }
                    in_mem_types::DagNodeLink::Remote(nh) => Box::new(futures::future::ok(nh)),
                }
            })
            .collect();

        let foo = futures::future::join_all(bar);

        let f = foo.and_then(|links: Vec<ipfs_types::IPFSHeader>| {
            // might be a bit of an approximation, but w/e
            let size = data.0.len() as u64 + links.iter().map(|x| x.size).sum::<u64>();

            let dag_node = ipfs_types::DagNode {
                data: data,
                links: links,
            };

            app_data
                .ipfs_node
                .put(dag_node.clone())
                .and_then(move |hp: ipfs_types::IPFSHash| {
                    cache_put(&app_data.cache, hp.clone(), dag_node);
                    let hdr = ipfs_types::IPFSHeader {
                        name: csh.to_string(),
                        hash: hp,
                        size: size,
                    };
                    futures::future::ok(hdr)
                })
        });

        Box::new(f)
    };

    let f = helper(app_data, csh, in_mem).map(web::Json);

    Box::new(f)
    // let f = data
    //     .ipfs_node
    //     .put(v.clone())
    //     .and_then(move |hp: ipfs_types::IPFSHash| {
    //         cache_put(&data.cache, hp.clone(), v);
    //         Ok(web::Json(IPFSPutResp { hash: hp }))
    //     });
    // Box::new(f)
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
        ipfs_node: ipfs_api::IPFSNode::new(http::uri::Authority::from_static("localhost:5001")),
    };
    let data = web::Data::new(state);

    HttpServer::new(move || {
        println!("init app");
        App::new()
            .register_data(data.clone()) // <- register the created data (Arc) - keeps 1 reference to keep it alive, presumably
            .route("/get/{n}", web::get().to_async(get))
            .route("/object/put", web::post().to_async(put))
            .route("/objects/put", web::post().to_async(put_many))
    })
    .bind("127.0.0.1:8088")
    .expect("Can not bind to 127.0.0.1:8088")
    .start();

    // Run actix system (actually starts all async processes, presumably blocks(?))
    sys.run()
}
