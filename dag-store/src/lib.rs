#![deny(warnings)]
pub mod capabilities;
pub mod opts;
pub mod server;

use crate::server::app::Runtime;
use dag_store_types::types::grpc::dag_store_server::DagStoreServer;
pub use opts::Opt;
use std::net::SocketAddr;
use tonic::transport::Server;

pub async fn run(
    runtime: Runtime,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + 'static>> {
    Server::builder()
        .add_service(DagStoreServer::new(runtime).accept_gzip().send_gzip())
        .serve(addr)
        .await?;

    Ok(())
}
