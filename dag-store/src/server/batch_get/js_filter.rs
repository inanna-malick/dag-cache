use crate::server::batch_get::Cache;
use dag_store_types::types::domain::{Hash, Id, Node};
use quick_js::Callback;
use quick_js::Context;
use quick_js::JsValue;
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;

struct IsInCache {
    cache: Arc<Cache>,
    header_map: HashMap<u32, Hash>,
}

fn args_to_id(args: Vec<quick_js::JsValue>) -> Result<u32, String> {
    if args.len() != 1 {
        return Err(format!(
            "Invalid argument count: Expected {}, got {}",
            1,
            args.len()
        ));
    }
    let target_id:&str  = match &args[0] {
            JsValue::String(s) => s,
            x => {
                return Err(format!(
                    "argument is not a valid id, must be a string that can be converted into a positive integer: {:?}",
                    x
                ))
            }
        };
    let target_id: u32 = target_id.parse().map_err(|e| {
        format!(
            "{} is not a valid id, must be a positive integer: {}",
            target_id, e
        )
    })?;

    Ok(target_id)
}

// LMAO THIS FUCKING SUCKS lol, put it in a separate module and test the shit out of it
impl Callback<()> for IsInCache {
    fn argument_count(&self) -> usize {
        0
    }

    fn call(
        &self,
        args: Vec<quick_js::JsValue>,
    ) -> Result<Result<quick_js::JsValue, String>, quick_js::ValueError> {
        let target_id = match args_to_id(args) {
            Ok(x) => x,
            Err(e) => return Ok(Err(e)),
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
    // todo: config for this, I think. max memory and etc
    let context = Context::new().unwrap();

    context
        .set_global(
            "node_data",
            std::str::from_utf8(&node.data).expect("node is not valid utf8 string"),
        )
        .unwrap();

    let header_map: HashMap<u32, Hash> = node.headers.iter().map(|h| (h.id.0, h.hash)).collect();
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
