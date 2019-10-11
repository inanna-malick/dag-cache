use crate::capabilities::lib::get_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::types::errors::DagCacheError;
use crate::types::ipfs::{DagNode, IPFSHash};
use chashmap::CHashMap;
use futures::channel::mpsc;
use futures::future::{FutureExt, TryFutureExt};
use futures::sink::SinkExt;
use futures::stream::Stream;
use std::sync::Arc;
use tokio;
use tracing::info;

// TODO: add fn that does get-and-cache, req's both caps
pub fn ipfs_fetch<C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Sync + Send>(
    caps: Arc<C>,
    hash: IPFSHash,
) -> impl Stream<Item = Result<DagNode, DagCacheError>> + 'static + Send {
    info!("starting recursive fetch for root hash {:?}", &hash);
    let (send, receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    ipfs_fetch_ana_internal(caps, hash, send, memoizer);

    receive
}

// anamorphism - an unfolding change
fn ipfs_fetch_ana_internal<
    C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Sync + Send,
>(
    caps: Arc<C>,
    hash: IPFSHash,
    resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>, // used to send completed nodes (eagerly)
    to_populate: Arc<CHashMap<IPFSHash, ()>>,                // used to memoize async fetches
) {
    let hash2 = hash.clone();
    to_populate.upsert(
        hash,
        || {
            tokio::spawn(
                ipfs_fetch_worker(caps, hash2, resp_chan, to_populate.clone())
                    .boxed()
                    .compat(),
            );
            ()
        },
        |()| (),
    );
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
async fn ipfs_fetch_worker<
    C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Sync + Send,
>(
    caps: Arc<C>,
    hash: IPFSHash,
    mut resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>,
    to_populate: Arc<CHashMap<IPFSHash, ()>>, // used to memoize async fetches
) -> Result<(), ()> {
    // let resp_chan_2 = resp_chan.clone(); // FIXME: async/await...
    // let caps2 = caps.clone();

    let res = get_and_cache(caps.clone(), hash.clone()).await;
    match res {
        Ok(node) => {
            let links = node.links.clone();
            // todo: caching should be baked into ipfs cap instead of being managed like this
            // caps.as_ref().cache_put(hash.clone(), node);
            // this way will only recurse on & traverse links if writing to channel doesn't fail
            // short circuit if failure
            // NOTE: should have some way to signal that this is an error instead of just failing?
            //       but the channel's broken so I can't.
            let sr: Result<(), ()> = resp_chan.send(Ok(node)).await.map_err(|_| ());

            // todo: weird type errors (async/await?), refactor later
            match sr {
                Ok(()) => {
                    for link in links.into_iter() {
                        ipfs_fetch_ana_internal(
                            caps.clone(),
                            link.hash,
                            resp_chan.clone(),
                            to_populate.clone(),
                        );
                    }
                    Ok(())
                }
                Err(()) => Err(()),
            }
        }
        Err(e) => {
            let sr: Result<(), ()> = resp_chan.send(Err(e)).await.map_err(|_| ()); // what err type to use? idk
            sr
        }
    }
}
