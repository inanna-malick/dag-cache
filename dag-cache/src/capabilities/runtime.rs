// use crate::capabilities::ipfs_store::IPFSNode;
use crate::capabilities::fs_ipfs_store::FileSystemStore;
use crate::capabilities::lru_cache::Cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap};

pub struct RuntimeCaps {
    pub cache: Cache,
    // pub ipfs_node: IPFSNode, TODO: make it easier to switch..
    pub store: FileSystemStore,
}

//todo: read up on generalized associated types
impl HasIPFSCap for RuntimeCaps {
    type Output = FileSystemStore;
    fn ipfs_caps(&self) -> &Self::Output {
        &self.store
    }
}

impl HasCacheCap for RuntimeCaps {
    type Output = Cache;
    fn cache_caps(&self) -> &Self::Output {
        &self.cache
    }
}

// pub struct RuntimeCaps2 {
//     pub cache: Cache,
//     pub ipfs_node: IPFSNode, TODO: make it easier to switch..
// }

// //todo: read up on generalized associated types
// impl HasIPFSCap for RuntimeCaps {
//     type Output = IPFSNode;
//     fn ipfs_caps(&self) -> &IPFSNode {
//         &self.ipfs_node
//     }
// }

// impl HasCacheCap for RuntimeCaps {
//     type Output = Cache;
//     fn cache_caps(&self) -> &Cache {
//         &self.cache
//     }
// }
