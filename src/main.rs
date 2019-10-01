#![deny(warnings, rust_2018_idioms)]

mod capabilities;
mod lib;
mod opts;
mod server;
mod types;

use opts::Opt;
use structopt::StructOpt;
use tracing::{info, span, Level};

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
    server::serve(caps, bind_to)
}
