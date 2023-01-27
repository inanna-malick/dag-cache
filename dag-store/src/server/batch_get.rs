mod js_filter;

use crate::capabilities::get_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use chashmap::CHashMap;
use dag_store_types::types::domain::{GetNodesResp, Hash, Node, NodeWithHash};
use dag_store_types::types::errors::DagCacheError;
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use tokio;
use tokio::sync::mpsc;
use tracing::{error, info};

type GetNodesStream =
    Pin<Box<dyn Stream<Item = Result<GetNodesResp, DagCacheError>> + Send + 'static>>;

pub fn batch_get<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    hash: Hash,
) -> GetNodesStream {
    info!("starting recursive fetch for root hash {:?}", &hash);
    let (send, mut receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    batch_get_ana_internal(store, cache, hash, send, memoizer);

    let stream = async_stream::stream! {
        while let Some(item) = receive.recv().await {
            yield item;
        }
    };

    stream.boxed()
}

// anamorphism - an unfolding change
fn batch_get_ana_internal<'a>(
    store: &'a Arc<dyn HashedBlobStore>,
    cache: &'a Arc<Cache>,
    hash: Hash,
    resp_chan: mpsc::Sender<Result<GetNodesResp, DagCacheError>>, // used to send completed nodes (eagerly)
    to_populate: Arc<CHashMap<Hash, ()>>,                         // used to memoize async fetches
) {
    let store = store.clone();
    let cache = cache.clone();
    to_populate.clone().upsert(
        hash,
        || {
            tokio::spawn(async move {
                batch_get_worker(store, cache, hash, resp_chan, to_populate).await
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
    resp_chan: mpsc::Sender<Result<GetNodesResp, DagCacheError>>,
    to_populate: Arc<CHashMap<Hash, ()>>, // used to memoize async fetches
) {
    let res = get_and_cache(&store, &cache, hash).await;
    match res {
        Ok(node) => {
            let links = node.headers.clone();
            // this way will only recurse on & traverse links if writing to channel doesn't fail
            // short circuit if failure
            // NOTE: should have some way to signal that this is an error instead of just failing?
            //       but the channel's broken so I can't.
            let sr = resp_chan
                .send(Ok(GetNodesResp::Node(NodeWithHash { hash, node })))
                .await;

            // todo: weird type errors (async/await?), refactor later
            match sr {
                Ok(()) => {
                    for link in links.into_iter() {
                        // TODO: metadata filtering here
                        if false {
                            if let Err(e) = resp_chan
                                .send(Ok(GetNodesResp::ChoseNotToExplore(link)))
                                .await
                            {
                                error!("failed sending 'chose not to traverse' resp via mpsc due to {:?}", e);
                            }
                        } else {
                            batch_get_ana_internal(
                                &store,
                                &cache,
                                link.hash,
                                resp_chan.clone(),
                                to_populate.clone(),
                            );
                        }
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
