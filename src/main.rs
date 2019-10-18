#![deny(warnings, rust_2018_idioms)]
mod capabilities;
mod lib;
mod opts;
mod server;
mod types;

use crate::capabilities::telemetry_subscriber::TelemetrySubscriber;
use crate::capabilities::runtime::Runtime;
use crate::server::app;
use crate::types::grpc::server::IpfsCacheServer;
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    // TODO: move addr parsing _into_ opts
    let bind_to = format!("127.0.0.1:{}", &opt.port);
    let Runtime(telemetry, runtime_caps) = opt.into_runtime();

    // TODO: figure out better (global? thread local? not a fcking mutex definitely) telemetry setup
    let subscriber = TelemetrySubscriber::new(telemetry);
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let app = app::CacheServer {
        caps: Arc::new(runtime_caps),
    };

    let addr = bind_to.parse().unwrap();

    Server::builder()
        .serve(addr, IpfsCacheServer::new(app))
        .await?;

    Ok(())
}
