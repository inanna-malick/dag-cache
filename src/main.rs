use actix;
use actix_web::{http, web, App, HttpServer};
use futures::future;
use futures::future::Future;
use lru::LruCache;
use std::sync::Mutex;

mod types;
use crate::types::{DagCacheError, DagNode, HashPointer, IPFSPutResp};

mod ipfs_io;

struct State {
    cache: Mutex<LruCache<HashPointer, DagNode>>,
    ipfs_node: ipfs_io::IPFSNode,
}

fn cache_get(mutex: &Mutex<LruCache<HashPointer, DagNode>>, k: HashPointer) -> Option<DagNode> {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    let mv = cache.get(&k);
    mv.cloned() // this feels weird? clone(d) is actually needed, right?
}

fn cache_put(mutex: &Mutex<LruCache<HashPointer, DagNode>>, k: HashPointer, v: DagNode) {
    // succeed or die. failure is unrecoverable (mutex poisoned)
    let mut cache = mutex.lock().unwrap();
    cache.put(k, v);
}

fn get(
    data: web::Data<State>,
    k: web::Path<(HashPointer)>,
) -> Box<dyn Future<Item = web::Json<DagNode>, Error = DagCacheError>> {
    let k = k.into_inner();
    match cache_get(&data.cache, k.clone()) {
        Some(res) => {
            Box::new(future::ok(web::Json(res.clone()))) // probably bad (clone is mb not needed?)
        }
        None => {
            let f = data
                .ipfs_node
                .get(k.clone())
                .and_then(move |dag_node: DagNode| {
                    cache_put(&data.cache, k, dag_node.clone());
                    Ok(web::Json(dag_node))
                });
            Box::new(f)
        }
    }
}

fn put(
    data: web::Data<State>,
    v: web::Json<DagNode>,
) -> Box<dyn Future<Item = web::Json<IPFSPutResp>, Error = DagCacheError>> {
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

fn main() {
    // PROBLEM: provisioning based on number of entities and _not_ number of bytes allocated total
    //          some dag nodes may be small and some may be large.
    let sys = actix::System::new("system"); // <- create Actix system

    let cache = LruCache::new(2);

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

    // Run actix system (actually starts all async processes)
    let _ = sys.run();
}
