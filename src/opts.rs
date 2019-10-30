use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::runtime::{Runtime, RuntimeCaps};
use honeycomb_tracing::Telemetry;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "dag cache",
    about = "ipfs wrapper, provides bulk put and bulk get via LRU cache"
)]
pub struct Opt {
    #[structopt(short = "p", long = "port", default_value = "8088")]
    pub port: u64,

    #[structopt(long = "ipfs_host", default_value = "localhost")]
    ipfs_host: String,
    #[structopt(long = "ipfs_port", default_value = "5001")]
    ipfs_port: u64,

    #[structopt(short = "n", long = "max_cache_entries", default_value = "128")]
    // arbitrarily chosen number..
    max_cache_entries: usize,

    #[structopt(short = "h", long = "honeycomb_key")]
    honeycomb_key: String,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        let ipfs_node = format!("http://{}:{}", &self.ipfs_host, self.ipfs_port); // TODO: https...
        let ipfs_node = IPFSNode::new(reqwest::Url::parse(&ipfs_node).unwrap_or_else(|_| {
            panic!(
                "unable to parse provided IPFS host + port ({:?}) as URL",
                &ipfs_node
            )
        }));

        let honeycomb_config = libhoney::Config {
            options: libhoney::client::Options {
                api_key: self.honeycomb_key,
                dataset: "dag-cache".to_string(),
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options::default(),
        };

        let telemetry = Telemetry::new(honeycomb_config);

        let cache = Cache::new(self.max_cache_entries);

        let rt = RuntimeCaps { cache, ipfs_node };

        Runtime(telemetry, rt)
    }
}
