use crate::capabilities::cache::Cache;
use crate::capabilities::store::FileSystemStore;
use crate::server::app::Runtime;
use std::sync::Arc;
use structopt::StructOpt;
use tracing_jaeger::{
    new_opentelemetry_layer, new_blackhole_telemetry_layer
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::registry;
use tracing_subscriber::layer::Layer;

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

    #[structopt(short = "j", long = "jaeger-agent")]
    pub jaeger_agent: Option<String>,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        let store = Arc::new(FileSystemStore::new(self.fs_path));

        match self.jaeger_agent {
            Some(jaeger_agent) => {

                let exporter = opentelemetry_jaeger::Exporter::builder()
                    .with_agent_endpoint(jaeger_agent.parse().unwrap())
                    .with_process(opentelemetry_jaeger::Process {
                        service_name: "dag-store".to_string(),
                        tags: vec![],
                    })
                    .init()
                    .unwrap();

                let telemetry_layer = new_opentelemetry_layer(
                    "dag-store", // TODO: duplication of service name here
                    Box::new(exporter),
                    Default::default(),
                );

                let subscriber = registry::Registry::default() // provide underlying span data store
                    .with(LevelFilter::INFO) // filter out low-level debug tracing (eg tokio executor)
                    .with(tracing_subscriber::fmt::Layer::default()) // log to stdout
                    .with(telemetry_layer); // publish to jaeger

                tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

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
