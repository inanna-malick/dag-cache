use crate::capabilities::{HashedBlobStore, MutableHashStore};
use blake2::{Blake2b, Digest};
use dag_cache_types::types::encodings;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash};
use serde_json;
use sled::Db;
use tracing::instrument;

/// store backed by local fs rocksdb
pub struct FileSystemStore(Db);

// TODO: better type names - IPFS -> Something Else (m)
fn mk_hash(v: &[u8]) -> IPFSHash {
    let mut hasher = Blake2b::new();
    hasher.input(v);
    let hash = hasher.result();
    IPFSHash::from_raw(encodings::Base58::from_bytes(hash.to_vec()))
}

// TODO: use tokio asnyc stuff for FS interaction - it was causing weird errors (not Sync, basically?)
impl FileSystemStore {
    pub fn new(path: String) -> Self {
        let db = Db::open(path).unwrap();
        FileSystemStore(db)
    }

    #[instrument(skip(self))]
    fn get_blob(&self, hash: IPFSHash) -> Result<DagNode, DagCacheError> {
        match self.0.get(format!("{}.blake2", hash)) {
            Ok(Some(value)) => {
                let node = serde_json::from_reader(value.as_ref())
                    .map_err(|e| DagCacheError::IPFSJsonError)?;
                Ok(node)
            }
            // TODO: actual handling for errors - time to revamp error schema?
            Ok(None) => panic!("value not found"),
            Err(e) => panic!("operational problem encountered: {}", e),
        }
    }

    #[instrument(skip(self, v))]
    fn put_blob(&self, v: DagNode) -> Result<IPFSHash, DagCacheError> {
        // TODO: serialize as proto now that I'm not interacting with IPFS! yay :)
        let bytes = serde_json::to_vec(&v).expect("json _serialize_ failed (should be impossible)");

        let hash = mk_hash(&bytes);

        self.0.insert(format!("{}.blake2", hash), bytes).unwrap(); // todo: expose error instead of panic

        Ok(hash)
    }

    #[instrument(skip(self))]
    fn get_mhs(&self, k: String) -> Result<Option<IPFSHash>, DagCacheError> {
        match self.0.get(k) {
            Ok(Some(value)) => Ok(Some(IPFSHash::from_raw(encodings::Base58::from_bytes(
                value.to_vec(),
            )))),
            // TODO: actual handling for errors - time to revamp error schema?
            Ok(None) => Ok(None),
            Err(e) => panic!("operational problem encountered: {}", e),
        }
    }

    #[instrument(skip(self, v))]
    fn put_mhs(&self, k: String, hash: IPFSHash) -> Result<Option<IPFSHash>, DagCacheError> {
        let base58 = hash.0;
        let bytes = base58.0;

        let prev = self.0.insert(k, bytes).unwrap(); // todo: expose error instead of panic
        let prev = prev.map(|p| IPFSHash::from_raw(encodings::Base58::from_bytes(p.to_vec())));

        Ok(prev)
    }
}

#[tonic::async_trait]
impl HashedBlobStore for FileSystemStore {
    async fn get(&self, hash: IPFSHash) -> Result<DagNode, DagCacheError> {
        self.get_blob(hash)
    }

    async fn put(&self, v: DagNode) -> Result<IPFSHash, DagCacheError> {
        self.put_blob(v)
    }
}

#[tonic::async_trait]
impl MutableHashStore for FileSystemStore {
    async fn get(&self, k: String) -> Result<Option<IPFSHash>, DagCacheError> {
        self.get_mhs(k)
    }

    async fn put(&self, k: String, hash: IPFSHash) -> Result<Option<IPFSHash>, DagCacheError> {
        self.put_mhs(k, hash)
    }
}
