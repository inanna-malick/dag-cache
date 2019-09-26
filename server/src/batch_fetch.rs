use futures::future::Future;
use futures::stream::Stream;

use crate::api_types;

use std::sync::Arc;

use tokio;

use crate::error_types::DagCacheError;
use crate::ipfs_api::HasIPFSCap;
use crate::ipfs_types::{DagNode, IPFSHash};
use std::convert::AsRef;
use tracing::info;

use crate::ipfs_types::IPFSHeader;
use crate::lib::BoxFuture;
use chashmap::CHashMap;
use futures::sink::Sink;
use futures::sync::mpsc;

// problem: can't meaningfully maintain validated graph structure while building tree via bulk fetch
// concept: just, like, whatever: run the algorithm, toss stuff into the map, log some weird error if it fails (?)
// concept: the above is fully mpsc compatible - maybe that's good? just dump everything into a map? lmao idk

// plan: (from talk w/ rain) (note: investigate async memo(ized) - mononoke uses it)
// use concurrent hash map to memoize. Write to map: future repr'ing result
// write future to hashmap iff doesn't contain already - chm supports this pattern (insert if not already exist, Fn () -> V)
// HashMap<IPFSHa

pub fn ipfs_fetch<C: 'static + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    hash: IPFSHash,
) -> impl Stream<Item = DagNode, Error = DagCacheError> + 'static + Send {
    let (send, receive) = mpsc::channel(128); // randomly chose this channel buffer size..
    let memoizer = Arc::new(CHashMap::new());

    ipfs_fetch_ana_internal(caps, hash, send, memoizer);

    receive.then( |res| match res {
        Ok(Ok(n)) => futures::future::ok(n),
        Ok(Err(e)) => futures::future::err(e),
        Err(()) => {
            panic!("mpsc receiver stream has error type (), did not expect to actually see error of said type")
        }
    })
}

// TODO: either abandon oneshot return channel or figure out useful metadata to collect.
// TODO: can abandon oneshot b/c if I pass an mpsc stream around it auto-closes when dropped (eg, when get tree completes)
// NOTE: does the return channel give me early failure (and thus, I think, cancellation? TODO: ask rain)
// anamorphism - an unfolding change
pub fn ipfs_fetch_ana_internal<C: 'static + HasIPFSCap + Sync + Send>(
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
fn ipfs_fetch_worker<C: 'static + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    hash: IPFSHash,
    resp_chan: mpsc::Sender<Result<DagNode, DagCacheError>>,
    to_populate: Arc<CHashMap<IPFSHash, ()>>, // used to memoize async fetches
) -> impl Future<Item = (), Error = ()> + 'static + Send {
    let resp_chan_2 = resp_chan.clone(); // FIXME: async/await...
    let caps2 = caps.clone();

    caps.as_ref()
        .ipfs_get(hash.clone())
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
