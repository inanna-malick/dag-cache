// use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::fs_store::FileSystemStore;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasMutableHashStore};

pub struct RuntimeCaps {
    pub cache: Arc<dyn Cache>,
    pub store: Arc<dyn FileSystemStore>,
}

//todo: read up on generalized associated types
impl HasIPFSCap for RuntimeCaps {
    type Output = FileSystemStore;
    fn ipfs_caps(&self) -> &Self::Output {
        &self.store
    }
}

impl HasMutableHashStore for RuntimeCaps {
    type Output = FileSystemStore;
    fn mhs_caps(&self) -> &Self::Output {
        &self.store
    }
}

impl HasCacheCap for RuntimeCaps {
    type Output = Cache;
    fn cache_caps(&self) -> &Self::Output {
        &self.cache
    }
}
