use actix;
use actix_web::{web, App, HttpServer};
use lru::LruCache;

mod capabilities;

mod api_types;
mod encoding_types;
mod in_mem_types;
mod ipfs_api;
mod ipfs_types;
mod lib;

use cache::Cache;
use capabilities::Capabilities;
use ipfs_api::IPFSNode;

mod api;
mod cache;

use tracing::{info, span, Level};

fn main() -> Result<(), std::io::Error> {
    // PROBLEM: provisioning based on number of entities and _not_ number of bytes allocated total
    //          some dag nodes may be small and some may be large.
    let sys = actix::System::new("system"); // <- create Actix system

    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let cache = Cache::new(LruCache::new(32)); // TODO: config, sensible defaults, etc
    let ipfs_node = IPFSNode::new(
        reqwest::Url::parse("http://localhost:5001").expect("parsing static string failed? fix it"),
    );

    let caps = web::Data::new(Capabilities::new(cache, ipfs_node));

    let span = span!(Level::TRACE, "app"); // todo: put app-level metadata here - port, any relevant config, etc
    let _enter = span.enter();

    HttpServer::new(move || {
        info!("initialize app");
        App::new()
            .register_data(caps.clone()) // <- register the created data (Arc) - keeps 1 reference to keep it alive, presumably
            .route(
                "/object/get/{n}",
                web::get().to_async(api::get::<Capabilities>),
            )
            .route(
                "/object/put",
                web::post().to_async(api::put::<Capabilities>),
            )
            .route(
                "/objects/put",
                web::post().to_async(api::put_many::<Capabilities>),
            )
    })
    .bind("127.0.0.1:8088")
    .expect("Can not bind to 127.0.0.1:8088")
    .start();

    // Run actix system (actually starts all async processes, presumably blocks(?))
    sys.run()
}
