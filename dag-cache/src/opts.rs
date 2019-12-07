use crate::capabilities::cache::Cache;
use crate::capabilities::store::FileSystemStore;
use crate::server::app::Runtime;
use honeycomb_tracing::TelemetryLayer;
use std::fs::File;
use std::io::prelude::*;
use std::sync::Arc;
use structopt::StructOpt;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::registry;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "dag cache",
    about = "ipfs wrapper, provides bulk put and bulk get via LRU cache"
)]
pub struct Opt {
    #[structopt(short = "p", long = "port", default_value = "8088")]
    pub port: u64,

    // #[structopt(long = "ipfs_host", default_value = "localhost")]
    // ipfs_host: String,
    // #[structopt(long = "ipfs_port", default_value = "5001")]
    // ipfs_port: u64,
    #[structopt(short = "f", long = "fs_path")]
    fs_path: String,

    #[structopt(short = "n", long = "max_cache_entries", default_value = "1024")]
    max_cache_entries: usize,

    #[structopt(short = "h", long = "honeycomb_key_file")]
    honeycomb_key_file: String,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        // let ipfs_node = format!("http://{}:{}", &self.ipfs_host, self.ipfs_port); // TODO: https...
        // let ipfs_node = IPFSNode::new(reqwest::Url::parse(&ipfs_node).unwrap_or_else(|_| {
        //     panic!(
        //         "unable to parse provided IPFS host + port ({:?}) as URL",
        //         &ipfs_node
        //     )
        // }));

        let store = Arc::new(FileSystemStore::new(self.fs_path));

        let mut file =
            File::open(self.honeycomb_key_file).expect("failed opening honeycomb key file");
        let mut honeycomb_key = String::new();
        file.read_to_string(&mut honeycomb_key)
            .expect("failed reading honeycomb key file");

        // NOTE: underlying lib is not really something I trust rn? just write my own queue + batch sender state machine...
        // TODO/FIXME/TODO/TODO: srsly, do this ^^
        let honeycomb_config = libhoney::Config {
            options: libhoney::client::Options {
                api_key: honeycomb_key,
                dataset: "dag-cache".to_string(), // todo rename
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options {
                max_batch_size: 1,
                ..libhoney::transmission::Options::default()
            },
        };
        let layer = TelemetryLayer::new("ipfs_dag_cache".to_string(), honeycomb_config)
            .and_then(tracing_subscriber::fmt::Layer::builder().finish())
            .and_then(LevelFilter::INFO);

        let subscriber = layer.with_subscriber(registry::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

        let cache = Arc::new(Cache::new(self.max_cache_entries));

        Runtime {
            cache: cache,
            mutable_hash_store: store.clone(),
            hashed_blob_store: store,
        }
    }
}
