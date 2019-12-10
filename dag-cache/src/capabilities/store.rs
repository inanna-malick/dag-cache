use crate::capabilities::{HashedBlobStore, MutableHashStore};
use blake2::{Blake2b, Digest};
use dag_cache_types::types::encodings;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash};
use serde_json;
use sled::Db;
use tracing::instrument;
use tracing::{info};

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
    fn get_mhs(&self, k: &str) -> Result<Option<IPFSHash>, DagCacheError> {
        match self.0.get(k) {
            Ok(x) => Ok(x.map(decode)),
            // TODO: actual handling for errors - time to revamp error schema?
            Err(e) => panic!("operational problem encountered: {}", e),
        }
    }

    // TODO: expose ONLY check and set, not put
    #[instrument(skip(self))]
    fn cas_mhs(
        &self,
        k: &str,
        previous_hash: Option<IPFSHash>,
        proposed_hash: IPFSHash,
    ) -> Result<(), DagCacheError> {
        println!("cas mhs!");

        info!("inside cas mhs");
        let cas_res =
            self.0
                .compare_and_swap(k, previous_hash.map(encode), Some(encode(proposed_hash))); // FIXME: refactor error structure
        info!("cas mhs res: {:?}", &cas_res);
        let cas_res = cas_res.unwrap();

        cas_res.map_err(
            |e: sled::CompareAndSwapError| DagCacheError::CASViolationError {
                actual_hash: e.current.map(decode),
            },
        )
    }
}

fn decode(hash: sled::IVec) -> IPFSHash {
    IPFSHash::from_raw(encodings::Base58::from_bytes(hash.to_vec()))
}

fn encode(hash: IPFSHash) -> Vec<u8> {
    let base58 = hash.0;
    let bytes = base58.0;
    bytes
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
    async fn get(&self, k: &str) -> Result<Option<IPFSHash>, DagCacheError> {
        self.get_mhs(k)
    }

    async fn cas(
        &self,
        k: &str,
        previous_hash: Option<IPFSHash>,
        proposed_hash: IPFSHash,
    ) -> Result<(), DagCacheError> {
        self.cas_mhs(k, previous_hash, proposed_hash)
    }
}
