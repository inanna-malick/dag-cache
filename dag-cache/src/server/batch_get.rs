use crate::capabilities::lib::get_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use chashmap::CHashMap;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs::{DagNode, IPFSHash};
use futures::channel::mpsc;
use futures::sink::SinkExt;
use std::sync::Arc;
use tokio;
use tracing::{error, info};

pub fn ipfs_fetch(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: IPFSHash,
) -> mpsc::Receiver<Result<DagNode, DagCacheError>> {
    info!("starting recursive fetch for root hash {:?}", &hash);
    let (send, receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    ipfs_fetch_ana_internal(store, cache, hash, send, memoizer);

    receive
}

// anamorphism - an unfolding change
fn ipfs_fetch_ana_internal(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: IPFSHash,
    resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>, // used to send completed nodes (eagerly)
    to_populate: Arc<CHashMap<IPFSHash, ()>>,                // used to memoize async fetches
) {
    let hash2 = hash.clone();
    to_populate.clone().upsert(
        hash,
        || {
            tokio::spawn(async move {
                ipfs_fetch_worker(store, cache, hash2, resp_chan, to_populate).await
            });
        },
        |()| (),
    );
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
async fn ipfs_fetch_worker(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: IPFSHash,
    mut resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>,
    to_populate: Arc<CHashMap<IPFSHash, ()>>, // used to memoize async fetches
) {
    let res = get_and_cache(store.clone(), cache.clone(), hash.clone()).await;
    match res {
        Ok(node) => {
            let links = node.links.clone();
            // this way will only recurse on & traverse links if writing to channel doesn't fail
            // short circuit if failure
            // NOTE: should have some way to signal that this is an error instead of just failing?
            //       but the channel's broken so I can't.
            let sr = resp_chan.send(Ok(node)).await;

            // todo: weird type errors (async/await?), refactor later
            match sr {
                Ok(()) => {
                    for link in links.into_iter() {
                        ipfs_fetch_ana_internal(
                            store.clone(),
                            cache.clone(),
                            link.hash,
                            resp_chan.clone(),
                            to_populate.clone(),
                        );
                    }
                }
                Err(e) => {
                    error!("failed sending resp via mpsc due to {:?}", e);
                }
            }
        }
        Err(e) => {
            let sr = resp_chan.send(Err(e)).await;
            if let Err(e) = sr {
                error!("failed sending resp via mpsc due to {:?}", e);
            };
        }
    }
}
