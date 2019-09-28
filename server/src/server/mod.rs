pub mod app;
pub mod batch_fetch;
pub mod batch_upload;
pub mod opportunistic_get;

use crate::capabilities::HasCacheCap;
use crate::capabilities::HasIPFSCap;
use crate::types::grpc::server;
use futures::{Future, Stream};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_hyper::server::{Http, Server};
use tracing::{error, info};

pub fn serve<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(caps: C, bind_to: String) {
    let app = app::App {
        caps: Arc::new(caps),
    };
    let new_service = server::IpfsCacheServer::new(app);

    let mut server = Server::new(new_service);
    let http = Http::new().http2_only(true).clone();

    let addr = bind_to.parse().unwrap();
    let bind = TcpListener::bind(&addr).expect("bind");

    info!("listining on {:?}", addr);

    let serve = bind
        .incoming()
        .for_each(move |sock| {
            if let Err(e) = sock.set_nodelay(true) {
                return Err(e);
            }

            let serve = server.serve_with(sock, http.clone());
            tokio::spawn(serve.map_err(|e| error!("h2 error: {:?}", e)));

            Ok(())
        })
        .map_err(|e| error!("accept error: {}", e));

    tokio::run(serve);
}
