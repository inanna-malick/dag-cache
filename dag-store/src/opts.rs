use crate::capabilities::cache::Cache;
use crate::capabilities::store::FileSystemStore;
use crate::server::app::Runtime;
use std::num::NonZeroUsize;
use std::sync::Arc;
use structopt::StructOpt;


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
    pub max_cache_entries: NonZeroUsize,
}

impl Opt {
    /// parse opts into capabilities object, will panic if not configured correctly (TODO: FIXME)
    pub fn into_runtime(self) -> Runtime {
        let store = Arc::new(FileSystemStore::new(self.fs_path));


        tracing_subscriber::fmt::init();

        let cache = Arc::new(Cache::new(self.max_cache_entries));

        Runtime {
            cache: cache,
            hashed_blob_store: store,
        }
    }
}
