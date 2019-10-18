use crate::capabilities::{HasCacheCap, HasIPFSCap};
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use tracing::info;
use tracing_futures::Instrument;

pub async fn get_and_cache<C: HasCacheCap + HasIPFSCap + Sync + 'static>(
    caps: &C,
    k: ipfs::IPFSHash,
) -> Result<ipfs::DagNode, DagCacheError> {
    let f = async {
        match caps.cache_get(k.clone()) {
            Some(dag_node) => {
                info!("cache hit");
                Ok(dag_node)
            }
            None => {
                info!("cache miss");

                let dag_node = caps.ipfs_get(k.clone()).await?;

                info!("writing result of post cache miss lookup to cache");
                caps.cache_put(k.clone(), dag_node.clone());

                Ok(dag_node)
            }
        }
    };

    f.instrument(tracing::info_span!("get-and-cache")).await
}

pub async fn put_and_cache<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: &C,
    node: ipfs::DagNode,
) -> Result<ipfs::IPFSHash, DagCacheError> {
    let f = async {
        let hash = caps.ipfs_put(node.clone()).await?;

        caps.cache_put(hash.clone(), node);

        Ok(hash)
    };

    f.instrument(tracing::info_span!("put and cache")).await
}
