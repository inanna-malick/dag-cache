#![deny(warnings)]

mod opts;

use dag_cache_types::types::{
    api::{bulk_put, get},
    grpc::{self, client::DagStoreClient},
    domain,
};
use opts::{Runtime, Opt};
use serde::Serialize;
use serde_json::json;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use structopt::StructOpt;
use tracing::instrument;
use warp::{reject, Filter};

// TODO: struct w/ domain types & etc
#[derive(Debug)]
struct Error(Box<dyn std::error::Error + Send + Sync + 'static>);

impl reject::Reject for Error {}

/// A serialized message to report in JSON format.
#[derive(Serialize)]
struct ErrorMessage<'a> {
    code: u16,
    message: &'a str,
}

// used to provide shared runtime ctx - there's probably a better way to do this
static mut GLOBAL_CTX: Option<Arc<Runtime>> = None;

fn get_ctx() -> Arc<Runtime> {
    unsafe {
        match &GLOBAL_CTX {
            Some(x) => x.clone(),
            None => panic!("global ctx not set"),
        }
    }
}

#[instrument]
async fn get_nodes(
    url: String,
    raw_hash: String,
) -> Result<notes_types::api::GetResp, Box<dyn std::error::Error + Send + Sync + 'static>> {
    println!("parsed hash {} from path", raw_hash);

    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    // TODO: validate base58 here
    let mut request = tonic::Request::new(grpc::Hash { hash: raw_hash });

    let meta = request.metadata_mut();
    meta.insert("trace_id", "traceid-test-8".parse().unwrap());

    let response = client.get_node(request).await.map_err(|e| Box::new(e))?;

    let response = get::Resp::from_proto(response.into_inner()).map_err(|e| Box::new(e))?;

    let response = notes_types::api::GetResp::from_generic(response)?;
    Ok(response)
}

#[instrument]
async fn get_initial_state(
    url: String,
) -> Result<Option<domain::Hash>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    println!("fetching initialstate");

    // TODO: using hardcoded local path - moving to structopt args would be nice
    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    // TODO: validate base58 here
    // TODO: dedup cas hash
    let mut request = tonic::Request::new(grpc::GetHashForKeyReq {
        key: notes_types::api::CAS_KEY.to_string(),
    });

    let meta = request.metadata_mut();
    // TODO: propagate proto trace ctx here instead of just ID
    meta.insert("trace_id", "traceid-test-8".parse().unwrap());

    let response = client.get_hash_for_key(request).await?;
    let response = response
        .into_inner()
        .hash
        .map(|p| domain::Hash::from_proto(p))
        .transpose()
        .map_err(|e| Box::new(e))?;

    Ok(response)
}

#[instrument]
async fn put_nodes(
    url: String,
    put_req: notes_types::api::PutReq,
) -> Result<bulk_put::Resp, Box<dyn std::error::Error + Send + Sync + 'static>> {
    println!("got req {:?} body", put_req);

    let put_req = put_req.into_generic()?;

    // TODO: better mgmt for grpc port/host
    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    // TODO: validate base58 here
    let mut request = tonic::Request::new(put_req.into_proto());

    let meta = request.metadata_mut();
    // TODO: figure out better encoding, 3 u64/string id fields
    meta.insert("trace_id", "traceid-test-8".parse().unwrap());

    let response = client.put_nodes(request).await.map_err(|e| Box::new(e))?;

    // NOTE: no need to use specific repr, hash and client id are generic enough
    let response = bulk_put::Resp::from_proto(response.into_inner()).map_err(|e| Box::new(e))?;

    Ok(response)
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let runtime = opt.into_runtime();
    unsafe {
        GLOBAL_CTX = Some(Arc::new(runtime));
    }

    let get_route = warp::path("node")
        .and(warp::path::param::<String>())
        // .end()
        .and_then({
            |raw_hash: String| {
                async move {
                    let url = get_ctx().dag_cache_url.to_string();
                    let res = get_nodes(url, raw_hash).await;

                    match res {
                        Ok(resp) => {
                            println!(" get route resp: {:?}", &resp);
                            // lmao how does this fail? seems to happen on
                            Ok(warp::reply::json(&resp))
                        }
                        Err(e) => {
                            println!("err on get: {:?}", e);
                            Err(reject::custom::<Error>(Error(e)))
                        }
                    }
                }
            }
        });

    let index_route = warp::get().and(warp::path::end()).and_then(|| {
        async {
            let url = get_ctx().dag_cache_url.to_string();
            let res = get_initial_state(url).await;

            match res {
                Ok(resp) => {
                    println!("initial state resp: {:?}", &resp);
                    let t = match resp {
                        Some(h) => crate::opts::WithTemplate {
                            name: "index.html",
                            value: json!({ "initial_hash": format!("{}", h) }),
                        },
                        None => crate::opts::WithTemplate {
                            name: "index.html",
                            value: json!({"initial_hash" : ""}),
                        },
                    };
                    Ok(get_ctx().render(t))
                }
                Err(e) => {
                    println!("err on get initial state: {:?}", e);
                    Err(reject::custom::<Error>(Error(e)))
                }
            }
        }
    });

    let post_route = warp::post()
        .and(warp::path("nodes"))
        .and(warp::body::content_length_limit(1024 * 16)) // arbitrary?
        .and(warp::body::json())
        // .end()
        .and_then(|put_req: notes_types::api::PutReq| {
            async move {
                let url = get_ctx().dag_cache_url.to_string();
                let res = put_nodes(url, put_req).await;

                match res {
                    Ok(resp) => Ok(warp::reply::json(&resp)),
                    Err(e) => {
                        println!("err on get: {:?}", e);
                        Err(reject::custom::<Error>(Error(e)))
                    }
                }
            }
        });

    // lmao, hardcoded - would be part of deployable, ideally
    // let static_route = warp::fs::dir("/home/pk/dev/dag-cache/notes-frontend/target/deploy");
    let static_route = warp::fs::dir("/static");

    let routes = get_route.or(post_route).or(index_route).or(static_route);

    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), get_ctx().port);
    warp::serve(routes).run(socket).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // uses mock capabilities, does not require local ipfs daemon
    #[tokio::test]
    async fn test_batch_upload() {
        // TODO: test env might have to be manual - how to express test dep on other bin in project?

        let url = "http://localhost:8088";

        // - get state, no hash.
        let state = get_initial_state(url.to_string()).await.unwrap();
        assert_eq!(state, None);

        let node = notes_types::notes::Node {
            parent: None, // _not_ T, constant type. NOTE: enforces that this is a TREE and not a DAG
            children: Vec::new(),
            header: "hdr".to_string(),
            body: "body".to_string(),
        };

        let put_req = notes_types::api::PutReq {
            head_node: node,
            extra_nodes: HashMap::new(),
            cas_hash: None,
        };

        // - push small tree with hash + no CAS hash
        let hash = put_nodes(url.to_string(), put_req).await.unwrap().root_hash;

        // FIXME: FIXME: FIXME:
        // FAILS HERE: state is just None instead of expected CAS (oh right, didn't impl that, did i?)
        // - get state, hash of small tree
        let state = get_initial_state(url.to_string()).await.unwrap();
        assert_eq!(state, Some(hash.clone()));

        // - get tree, recursive expansion of same (NOTE: only one layer currently)
        let get_resp = get_nodes(url.to_string(), hash.to_string()).await.unwrap();

        let expected_node = notes_types::notes::Node {
            parent: None, // _not_ T, constant type. NOTE: enforces that this is a TREE and not a DAG
            children: Vec::new(),
            header: "hdr".to_string(),
            body: "body".to_string(),
        };

        // test round trip
        assert_eq!(get_resp.requested_node, expected_node);
    }
}
