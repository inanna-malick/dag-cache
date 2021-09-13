use crate::capabilities::{HashedBlobStore, MutableHashStore};
use dag_store_types::types::domain::{Hash, Node};
use dag_store_types::types::errors::DagCacheError;
use prost::Message;
use tracing::instrument;

/// store backed by local fs sled db (embedded)
pub struct FileSystemStore(sled::Db);

impl FileSystemStore {
    pub fn new(path: String) -> Self {
        let db = sled::open(path).unwrap();
        FileSystemStore(db)
    }

    fn get_and_decode<X: Message + Default>(&self, k: &str) -> Result<Option<X>, DagCacheError> {
        let res = self.0.get(k).map_err(DagCacheError::unexpected)?;
        let proto: Option<X> = res
            .map(std::io::Cursor::new)
            .map(Message::decode)
            .transpose()
            .map_err(DagCacheError::unexpected)?;

        Ok(proto)
    }

    #[instrument(skip(self))]
    fn get_blob(&self, hash: Hash) -> Result<Node, DagCacheError> {
        let proto = self.get_and_decode(&hash.to_string_canonical())?;
        let proto = proto
            .ok_or_else(|| DagCacheError::UnexpectedError("broken link in sled db!".to_string()))?;
        let res = Node::from_proto(proto)?;
        Ok(res)
    }

    #[instrument(skip(self, v))]
    fn put_blob(&self, v: Node) -> Result<Hash, DagCacheError> {
        let hash = v.canonical_hash();

        let mut buf = vec![];
        v.into_proto()
            .encode(&mut buf)
            .map_err(DagCacheError::unexpected)?;

        self.0
            .insert(hash.to_string_canonical(), buf)
            .map_err(DagCacheError::unexpected)?;

        Ok(hash)
    }

    #[instrument(skip(self))]
    fn get_mhs(&self, k: &str) -> Result<Option<Hash>, DagCacheError> {
        let res = self.0.get(k).map_err(DagCacheError::unexpected)?;
        let res = res.map(decode);
        Ok(res)
    }

    #[instrument(skip(self))]
    fn cas_mhs(
        &self,
        k: &str,
        previous_hash: Option<Hash>,
        proposed_hash: Hash,
    ) -> Result<(), DagCacheError> {
        let cas_res =
            self.0
                .compare_and_swap(k, previous_hash.map(encode), Some(encode(proposed_hash)));
        let cas_res = cas_res.unwrap();

        cas_res.map_err(
            |e: sled::CompareAndSwapError| DagCacheError::CASViolationError {
                actual_hash: e.current.map(decode),
            },
        )
    }
}

fn decode(hash: sled::IVec) -> Hash {
    // FIXME/TODO: is this recoverable? not really, but I still don't like panic here
    Hash::from_bytes(&hash).expect("invalid bytes for hash in CAS store, panic")
}

fn encode(hash: Hash) -> Vec<u8> {
    hash.0.as_slice().to_vec()
}

#[tonic::async_trait]
impl HashedBlobStore for FileSystemStore {
    async fn get(&self, hash: Hash) -> Result<Node, DagCacheError> {
        self.get_blob(hash)
    }

    async fn put(&self, v: Node) -> Result<Hash, DagCacheError> {
        self.put_blob(v)
    }
}

#[tonic::async_trait]
impl MutableHashStore for FileSystemStore {
    async fn get(&self, k: &str) -> Result<Option<Hash>, DagCacheError> {
        self.get_mhs(k)
    }

    async fn cas(
        &self,
        k: &str,
        previous_hash: Option<Hash>,
        proposed_hash: Hash,
    ) -> Result<(), DagCacheError> {
        self.cas_mhs(k, previous_hash, proposed_hash)
    }
}
