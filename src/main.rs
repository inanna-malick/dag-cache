#![deny(warnings, rust_2018_idioms)]
#![feature(type_alias_impl_trait)]
mod capabilities;
mod lib;
mod opts;
mod server;
mod types;

use crate::capabilities::telemetry::Telemetry;
use crate::capabilities::telemetry_subscriber::TelemetrySubscriber;
use crate::server::app;
use crate::types::grpc::server::IpfsCacheServer;
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    let bind_to = format!("127.0.0.1:{}", opt.port.clone());
    let hk_key = opt.honeycomb_key.clone();
    let caps = opt.into_runtime();

    // initialize and register event/span logging subscriber
    // let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();

    // TODO: figure out better (global? thread local? not a fcking mutex definitely) telemetry setup
    let subscriber = TelemetrySubscriber::new(Telemetry::new(hk_key));
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let app = app::CacheServer {
        caps: Arc::new(caps),
    };

    let addr = bind_to.parse().unwrap();

    Server::builder()
        .serve(addr, IpfsCacheServer::new(app))
        .await?;

    Ok(())
}
