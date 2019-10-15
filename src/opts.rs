use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::runtime::Runtime;
use crate::capabilities::telemetry::Telemetry;
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
    pub ipfs_host: String,
    #[structopt(long = "ipfs_port", default_value = "5001")]
    pub ipfs_port: u64,

    #[structopt(short = "n", long = "max_cache_entries", default_value = "128")]
    // arbitrarily chosen number..
    pub max_cache_entries: usize,

    #[structopt(short = "h", long = "honeycomb_key")]
    pub honeycomb_key: String,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        let ipfs_node = format!("http://{}:{}", &self.ipfs_host, self.ipfs_port); // TODO: https...
        let ipfs_node = IPFSNode::new(reqwest::Url::parse(&ipfs_node).expect(&format!(
            "unable to parse provided IPFS host + port ({:?}) as URL",
            &ipfs_node
        )));

        let telemetry = Telemetry::new(self.honeycomb_key);

        let cache = Cache::new(self.max_cache_entries);

        Runtime {
            telemetry,
            cache,
            ipfs_node,
        }
    }
}
