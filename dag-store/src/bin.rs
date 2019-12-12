// #![deny(warnings)]
mod capabilities;
mod opts;
mod server;
mod utils;

use dag_store_types::types::grpc::server::DagStoreServer;
use opts::Opt;
use structopt::StructOpt;
use tonic::transport::Server;

// TODO: debug this - not publishing to honeycomb from docker compose!?!>
// TODO: confirm everything works in small example

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();

    // TODO: move addr parsing _into_ opts
    let bind_to = format!("0.0.0.0:{}", &opt.port);
    let runtime = opt.into_runtime();

    let addr = bind_to.parse().unwrap();

    Server::builder()
        .add_service(DagStoreServer::new(runtime))
        .serve(addr)
        .await?;

    Ok(())
}
