use crate::capabilities::get_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use chashmap::CHashMap;
use dag_store_types::types::domain::{Hash, Node};
use dag_store_types::types::errors::DagCacheError;
use quick_js::Callback;
use std::collections::HashMap;
use std::sync::Arc;
use tokio;
use tokio::sync::mpsc;
use tracing::{error, info};

use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;

use std::convert::{TryInto};

use dag_store_types::types::domain::Id;

type GetNodesStream = Pin<Box<dyn Stream<Item = Result<Node, DagCacheError>> + Send + 'static>>;

struct IsInCache {
    cache: Arc<Cache>,
    header_map: HashMap<u32, Hash>,
}

// LMAO THIS FUCKING SUCKS lol, put it in a separate module and test the shit out of it
impl Callback<()> for IsInCache {
    fn argument_count(&self) -> usize {
        1
    }

    fn call(
        &self,
        args: Vec<quick_js::JsValue>,
    ) -> Result<Result<quick_js::JsValue, String>, quick_js::ValueError> {
        if args.len() != 1 {
            return Ok(Err(format!(
                "Invalid argument count: Expected {}, got {}",
                1,
                args.len()
            )));
        }
        let target_id: i32 = match args[0].clone().try_into() {
            Ok(x) => x,
            Err(e) => {
                return Ok(Err(format!(
                    "argument is not a valid id, must be an integer: {}",
                    e
                )))
            }
        };
        let target_id: u32 = match target_id.try_into() {
            Ok(x) => x,
            Err(e) => {
                return Ok(Err(format!(
                    "{} is not a valid id, must be a positive integer: {}",
                    target_id, e
                )))
            }
        };

        let target_hash: Hash = match self.header_map.get(&target_id).ok_or(format!(
            "invalid id, not found in node headers: {:?}",
            target_id
        )) {
            Ok(x) => x.clone(),
            Err(e) => return Ok(Err(e)),
        };

        Ok(Ok(quick_js::JsValue::Bool(
            self.cache.get(target_hash).is_some(),
        )))
    }
}

fn choose_child_nodes_to_traverse(js: String, node: &Node, cache: Arc<Cache>) -> Vec<Id> {
    use quick_js::{Context};

    // todo: config for this, I think. max memory and etc
    let context = Context::new().unwrap();

    context.set_global(
        "node_data",
        std::str::from_utf8(&node.data).expect("node is not valid utf8 string"),
    ).unwrap();

    let header_map: HashMap<u32, Hash> = node.links.iter().map(|h| (h.id.0, h.hash)).collect();
    context
        .add_callback("is_in_cache", IsInCache { cache, header_map })
        //  .add_callback("hash_for_id", |id: u32| a + b) TODO/FIXME: need conversion into JsValue
        .unwrap();

    context
        .eval_as::<Vec<i32>>(&js)
        .unwrap()
        .into_iter()
        // THIS IS FUCKING JANKY, WHY DOESN'T JAVASCRIPT SUPPORT U32
        .map(|i| Id(i.try_into().unwrap()))
        .collect()
}

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
    resp_chan: mpsc::Sender<Result<Node, DagCacheError>>, // used to send completed nodes (eagerly)
    to_populate: Arc<CHashMap<Hash, ()>>,                 // used to memoize async fetches
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
    resp_chan: mpsc::Sender<Result<Node, DagCacheError>>,
    to_populate: Arc<CHashMap<Hash, ()>>, // used to memoize async fetches
) {
    let res = get_and_cache(&store, &cache, hash).await;
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
                            &store,
                            &cache,
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
