#![deny(warnings, rust_2018_idioms)]
mod capabilities;
mod lib;
mod opts;
mod server;
mod types;

use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::server::app;
use crate::types::grpc::server::IpfsCacheServer;
use futures::{Future, Stream};
use opts::Opt;
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;
use tower_hyper::server::{Http, Server};
use tracing::{error, info, span, Level};

fn main() {
    let opt = Opt::from_args();

    let bind_to = format!("127.0.0.1:{}", opt.port.clone());
    let caps = opt.into_runtime();

    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let span = span!(Level::TRACE, "app"); // todo: put app-level metadata here - port, any relevant config, etc
    let _enter = span.enter();

    info!("initializing server on {}", bind_to);
    serve(caps, bind_to)
}

fn serve<C: HasCacheCap + HasTelemetryCap + HasIPFSCap + Sync + Send + 'static>(
    caps: C,
    bind_to: String,
) {
    let app = app::Server {
        caps: Arc::new(caps),
    };
    let new_service = IpfsCacheServer::new(app);

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
