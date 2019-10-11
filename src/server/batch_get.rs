use crate::capabilities::lib::get_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::lib::BoxFuture;
use crate::types::errors::DagCacheError;
use crate::types::ipfs::{DagNode, IPFSHash, IPFSHeader};
use chashmap::CHashMap;
use futures::future::Future;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::sync::mpsc;
use std::sync::Arc;
use tokio;
use tracing::info;

// TODO: add fn that does get-and-cache, req's both caps
pub fn ipfs_fetch<C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Sync + Send>(
    caps: Arc<C>,
    hash: IPFSHash,
) -> impl Stream<Item = DagNode, Error = DagCacheError> + 'static + Send {
    info!("starting recursive fetch for root hash {:?}", &hash);
    let (send, receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    ipfs_fetch_ana_internal(caps, hash, send, memoizer);

    receive.then( |res| match res {
        Ok(Ok(n)) => futures::future::ok(n),
        Ok(Err(e)) => futures::future::err(e),
        Err(()) => {
            // this should never happen...
            let msg = format!("mpsc receiver stream has error type (), did not expect to actually see error of said type");
            futures::future::err(DagCacheError::UnexpectedError { msg })
        }
    })
}

// TODO: either abandon oneshot return channel or figure out useful metadata to collect.
// TODO: can abandon oneshot b/c if I pass an mpsc stream around it auto-closes when dropped (eg, when get tree completes)
// NOTE: does the return channel give me early failure (and thus, I think, cancellation? TODO: ask rain)
// anamorphism - an unfolding change
pub fn ipfs_fetch_ana_internal<
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
            tokio::spawn(ipfs_fetch_worker(
                caps,
                hash2,
                resp_chan,
                to_populate.clone(),
            ));
            ()
        },
        |()| (),
    );
}

// worker thread - uses one-shot channel to return result to avoid unbounded stack growth
fn ipfs_fetch_worker<C: 'static + HasIPFSCap + HasTelemetryCap + HasCacheCap + Sync + Send>(
    caps: Arc<C>,
    hash: IPFSHash,
    resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>,
    to_populate: Arc<CHashMap<IPFSHash, ()>>, // used to memoize async fetches
) -> impl Future<Item = (), Error = ()> + 'static + Send {
    let resp_chan_2 = resp_chan.clone(); // FIXME: async/await...
    let caps2 = caps.clone();

    get_and_cache(caps, hash.clone())
        .then(move |res| -> BoxFuture<Vec<IPFSHeader>, ()> {
            match res {
                Ok(node) => {
                    let links = node.links.clone();
                    // todo: caching should be baked into ipfs cap instead of being managed like this
                    // caps.as_ref().cache_put(hash.clone(), node);
                    // this way will only recurse on & traverse links if writing to channel doesn't fail
                    let f = resp_chan.send(Ok(node)).map(|_| links).map_err(|_| {
                        // idk what to do if sending fails here, so just err ()
                        ()
                    });
                    Box::new(f)
                }
                Err(e) => {
                    let f = resp_chan
                        .send(Err(e)) // if send to chan fails, lmao, idk - maybe add extra logging?
                        .then(|_| futures::future::err(()));
                    Box::new(f)
                }
            }
        })
        .map(move |links| {
            for link in links.into_iter() {
                ipfs_fetch_ana_internal(
                    caps2.clone(),
                    link.hash,
                    resp_chan_2.clone(),
                    to_populate.clone(),
                )
            }
            ()
        })
        .map_err(|_| ()) // TODO: log or etc here, mb
}
