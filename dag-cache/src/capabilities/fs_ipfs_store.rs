use crate::capabilities::IPFSCapability;
use blake2::{Blake2b, Digest};
use dag_cache_types::types::encodings;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash};
use serde_json;
use std::fs::File;
use std::io::prelude::*;
use tracing::instrument;

pub struct FileSystemStore(pub String); //base path

// TODO: better type names - IPFS -> Something Else (m)
fn mk_hash(v: &[u8]) -> IPFSHash {
    let mut hasher = Blake2b::new();
    hasher.input(v);
    let hash = hasher.result();
    IPFSHash::from_raw(encodings::Base58::from_bytes(hash.to_vec()))
}

// TODO: use tokio asnyc stuff for FS interaction - it was causing weird errors (not Sync, basically?)
impl FileSystemStore {
    fn path_for(&self, h: &IPFSHash) -> String {
        format!("{}/{}.blake2", self.0, h)
    }

    #[instrument(skip(self))]
    fn get_(&self, hash: IPFSHash) -> Result<DagNode, DagCacheError> {
        let file = File::open(self.path_for(&hash)).map_err(|e| {
            DagCacheError::UnexpectedError {
                // TODO: better
                msg: format!("file IO error (on open) {}", e),
            }
        })?;

        let node: DagNode =
            serde_json::from_reader(file).map_err(|e| DagCacheError::IPFSJsonError)?;

        Ok(node)
    }

    #[instrument(skip(self, v))]
    fn put_(&self, v: DagNode) -> Result<IPFSHash, DagCacheError> {
        // TODO: serialize as proto now that I'm not interacting with IPFS! yay :)
        let bytes = serde_json::to_vec(&v).expect("json _serialize_ failed (should be impossible)");

        let hash = mk_hash(&bytes);

        let mut file = File::create(self.path_for(&hash)).map_err(|e| {
            DagCacheError::UnexpectedError {
                // TODO: better
                msg: format!("file IO error {}", e),
            }
        })?;
        file.write_all(&bytes).map_err(|e| {
            DagCacheError::UnexpectedError {
                // TODO: better
                msg: format!("file IO error {}", e),
            }
        })?;

        Ok(hash)
    }
}

#[tonic::async_trait]
impl IPFSCapability for FileSystemStore {
    async fn get(&self, hash: IPFSHash) -> Result<DagNode, DagCacheError> {
        self.get_(hash)
    }

    async fn put(&self, v: DagNode) -> Result<IPFSHash, DagCacheError> {
        self.put_(v)
    }
}
