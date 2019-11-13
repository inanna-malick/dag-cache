// #![deny(warnings)]
mod capabilities;
mod generated_grpc_bindings;
mod opts;
mod server;
mod types;
mod utils;

use crate::generated_grpc_bindings::server::IpfsCacheServer;
use crate::server::app;
use honeycomb_tracing::TelemetrySubscriber;
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    // TODO: move addr parsing _into_ opts
    let bind_to = format!("127.0.0.1:{}", &opt.port);
    let (runtime_caps, honeycomb_config) = opt.into_runtime();

    let subscriber = TelemetrySubscriber::new("ipfs_dag_cache".to_string(), honeycomb_config);
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
