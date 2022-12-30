use crate::capabilities::{HashedBlobStore};
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
