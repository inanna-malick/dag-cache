use crate::capabilities::get_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use chashmap::CHashMap;
use dag_store_types::types::domain::{Hash, Node};
use dag_store_types::types::errors::DagCacheError;
use futures::channel::mpsc;
use futures::sink::SinkExt;
use std::sync::Arc;
use tokio;
use tracing::{error, info};

pub fn batch_get(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: Hash,
) -> mpsc::Receiver<Result<Node, DagCacheError>> {
    info!("starting recursive fetch for root hash {:?}", &hash);
    let (send, receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    batch_get_ana_internal(store, cache, hash, send, memoizer);

    receive
}

// anamorphism - an unfolding change
fn batch_get_ana_internal(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: Hash,
    resp_chan: mpsc::Sender<Result<Node, DagCacheError>>, // used to send completed nodes (eagerly)
    to_populate: Arc<CHashMap<Hash, ()>>,                 // used to memoize async fetches
) {
    let hash2 = hash.clone();
    to_populate.clone().upsert(
        hash,
        || {
            tokio::spawn(async move {
                batch_get_worker(store, cache, hash2, resp_chan, to_populate).await
            });
        },
        |()| (),
    );
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
async fn batch_get_worker(
    store: Arc<dyn HashedBlobStore>,
    cache: Arc<Cache>,
    hash: Hash,
    mut resp_chan: mpsc::Sender<Result<Node, DagCacheError>>,
    to_populate: Arc<CHashMap<Hash, ()>>, // used to memoize async fetches
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
                        batch_get_ana_internal(
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
