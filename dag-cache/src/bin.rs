// #![deny(warnings)]
mod capabilities;
mod opts;
mod server;
mod utils;

use crate::server::app;
use dag_cache_types::types::grpc::server::IpfsCacheServer;
use honeycomb_tracing::TelemetrySubscriber;
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tonic::transport::Server;

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Layer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    // TODO: move addr parsing _into_ opts
    let bind_to = format!("127.0.0.1:{}", &opt.port);
    let (runtime_caps, honeycomb_config) = opt.into_runtime();

    let subscriber = TelemetrySubscriber::new("ipfs_dag_cache".to_string(), honeycomb_config);
    // filter out tracing noise
    let subscriber = LevelFilter::INFO.with_subscriber(subscriber);

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
