use crate::capabilities::cache::Cache;
use crate::capabilities::store::FileSystemStore;
use crate::server::app::Runtime;
use std::fs::File;
use std::io::prelude::*;
use std::sync::Arc;
use structopt::StructOpt;
use tracing_honeycomb::{new_blackhole_telemetry_layer, new_honeycomb_telemetry_layer};
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

    #[structopt(short = "f", long = "fs_path")]
    pub fs_path: String,

    #[structopt(short = "n", long = "max_cache_entries", default_value = "1024")]
    pub max_cache_entries: usize,

    #[structopt(short = "h", long = "honeycomb_key_file")]
    pub honeycomb_key_file: Option<String>,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        let store = Arc::new(FileSystemStore::new(self.fs_path));

        match self.honeycomb_key_file {
            Some(honeycomb_key_file) => {
                let mut file =
                    File::open(honeycomb_key_file).expect("failed opening honeycomb key file");
                let mut honeycomb_key = String::new();
                file.read_to_string(&mut honeycomb_key)
                    .expect("failed reading honeycomb key file");

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

                let telemetry_layer = new_honeycomb_telemetry_layer("dag-store", honeycomb_config);

                let subscriber =
                    telemetry_layer // publish to tracing
                        .and_then(tracing_subscriber::fmt::Layer::builder().finish()) // log to stdout
                        .and_then(LevelFilter::INFO) // omit low-level debug tracing (eg tokio executor)
                        .with_subscriber(registry::Registry::default()); // provide underlying span data store

                tracing::subscriber::set_global_default(subscriber)
                    .expect("setting global default failed");
            }
            None => {
                let telemetry_layer = new_blackhole_telemetry_layer();

                let subscriber =
                    telemetry_layer // publish to tracing
                        .and_then(tracing_subscriber::fmt::Layer::builder().finish()) // log to stdout
                        .and_then(LevelFilter::INFO) // omit low-level debug tracing (eg tokio executor)
                        .with_subscriber(registry::Registry::default()); // provide underlying span data store

                tracing::subscriber::set_global_default(subscriber)
                    .expect("setting global default failed");
            }
        };

        let cache = Arc::new(Cache::new(self.max_cache_entries));

        Runtime {
            cache: cache,
            mutable_hash_store: store.clone(),
            hashed_blob_store: store,
        }
    }
}
