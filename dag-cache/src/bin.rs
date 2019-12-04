// #![deny(warnings)]
mod capabilities;
mod opts;
mod server;
mod utils;

use crate::server::app;
use dag_cache_types::types::grpc::server::IpfsCacheServer;
use honeycomb_tracing::TelemetryLayer;
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tonic::transport::Server;

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::registry;

// TODO: debug this - not publishing to honeycomb from docker compose!?!>
// TODO: confirm everything works in small example

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    // TODO: move addr parsing _into_ opts
    let bind_to = format!("0.0.0.0:{}", &opt.port);
    let (runtime_caps, honeycomb_config) = opt.into_runtime();

    let layer = TelemetryLayer::new("ipfs_dag_cache".to_string(), honeycomb_config)
        .and_then(tracing_subscriber::fmt::Layer::builder().finish())
        .and_then(LevelFilter::INFO);

    let subscriber = layer.with_subscriber(registry::Registry::default());

    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let app = app::CacheServer {
        caps: Arc::new(runtime_caps),
    };

    let addr = bind_to.parse().unwrap();

    Server::builder()
        .add_service(IpfsCacheServer::new(app))
        .serve(addr)
        .await?;

    Ok(())
}
