// #![deny(warnings)]
pub mod capabilities;
pub mod opts;
pub mod server;

use dag_store_types::types::grpc::server::DagStoreServer;
pub use opts::Opt;
use tonic::transport::Server;
use crate::server::app::Runtime;
use std::net::SocketAddr;

pub async fn run(runtime: Runtime, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + 'static>> {
    Server::builder()
        .add_service(DagStoreServer::new(runtime))
        .serve(addr)
        .await?;

    Ok(())
}
