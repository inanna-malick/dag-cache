mod capabilities;

mod api;
mod api_types;
mod batch_fetch;
mod batch_upload;
mod cache;
mod encoding_types;
mod error_types;
mod in_mem_types;
mod ipfs_api;
mod ipfs_types;
mod lib;
mod server;

use capabilities::Capabilities;
use ipfs_api::IPFSNode;
use std::sync::Arc;
use structopt::StructOpt;
use tracing::{info, span, Level};

// TODO ^ organize/clean inputs/use block

// TODO: provide simple naming standard for dag links - can probably gen somehow from generic Structs
// TODO: enforce (and parse) naming scheme for node pointers
// TODO: eg: 'parent: Commit' // (NOTE: will need to handle multiple mappings, eg: 'dir_entity_1: DirEntity' and etc)
// TODO: maybe also map entries

#[derive(Debug, StructOpt)]
#[structopt(
    name = "dag cache",
    about = "ipfs wrapper, provides bulk put and bulk get via LRU cache"
)]
struct Opt {
    #[structopt(short = "p", long = "port", default_value = "8088")]
    port: u64,

    #[structopt(long = "ipfs_host", default_value = "localhost")]
    ipfs_host: String,
    #[structopt(long = "ipfs_port", default_value = "5001")]
    ipfs_port: u64,

    #[structopt(short = "n", long = "max_cache_entries", default_value = "128")]
    // randomly chosen number..
    max_cache_entries: usize,
}

fn main() {
    let opt = Opt::from_args();

    let ipfs_node = format!("http://{}:{}", &opt.ipfs_host, opt.ipfs_port); // TODO: https...
    let ipfs_node = IPFSNode::new(reqwest::Url::parse(&ipfs_node).expect(&format!(
        "unable to parse provided IPFS host + port ({:?}) as URL",
        &ipfs_node
    )));
    // PROBLEM: provisioning based on number of entities and _not_ number of bytes allocated total
    //          some dag nodes may be small and some may be large.

    let bind_to = format!("127.0.0.1:{}", opt.port);

    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

    let caps = Arc::new(Capabilities::new(opt.max_cache_entries, ipfs_node));

    let span = span!(Level::TRACE, "app"); // todo: put app-level metadata here - port, any relevant config, etc
    let _enter = span.enter();

    server::serve(caps, bind_to)
}
