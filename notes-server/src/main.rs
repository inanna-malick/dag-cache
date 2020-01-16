#![deny(warnings)]

mod opts;

use dag_store_types::types::{
    api::{bulk_put, get, meta},
    domain::{self, Hash},
    grpc::{self, client::DagStoreClient},
};
use honeycomb_tracing::{TraceCtx, TraceId};
use opts::{Opt, Runtime};
use prost::Message;
use serde::Serialize;
use serde_json::json;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use structopt::StructOpt;
use tonic::metadata::MetadataValue;
use tracing::{error, info, instrument};
use warp::{reject, Filter};

use headers::HeaderMapExt;
use hyper::body::Chunk;

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

fn register_trace_root() {
    println!("register trace root");
    let trace_id = TraceId::generate();
    TraceCtx {
        trace_id,
        parent_span: None,
    }
    .record_on_current_span();
    println!("register trace root done");
}

fn add_tracing_to_meta<T>(request: &mut tonic::Request<T>) {
    let current_trace_ctx = TraceCtx::eval_current_trace_ctx();

    if let Some(ctx) = current_trace_ctx {
        let meta = request.metadata_mut();
        let ctx: grpc::TraceCtx = meta::trace_ctx_into_proto(ctx);
        let mut buf = vec![];
        ctx.encode(&mut buf)
            .expect("error writing proto msg to buffer");

        meta.insert_bin("trace-ctx-bin", MetadataValue::from_bytes(&buf));
    };
}

#[instrument]
async fn get_nodes(
    url: String,
    raw_hash: String,
) -> Result<notes_types::api::GetResp, Box<dyn std::error::Error + Send + Sync + 'static>> {
    register_trace_root();

    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    let hash = Hash::from_base58(&raw_hash).map_err(|e| Box::new(e))?;

    let mut request = tonic::Request::new(hash.into_proto());
    add_tracing_to_meta(&mut request);

    let response = client.get_node(request).await.map_err(|e| Box::new(e))?;

    let response = get::Resp::from_proto(response.into_inner()).map_err(|e| Box::new(e))?;

    let response = notes_types::api::GetResp::from_generic(response)?;
    Ok(response)
}

#[instrument]
async fn get_initial_state(
    url: String,
) -> Result<Option<domain::Hash>, Box<dyn std::error::Error + Send + Sync + 'static>> {
    println!("get initial state first line");
    register_trace_root();

    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    // TODO: validate base58 here
    // TODO: dedup cas hash
    let mut request = tonic::Request::new(grpc::GetHashForKeyReq {
        key: notes_types::api::CAS_KEY.to_string(),
    });
    add_tracing_to_meta(&mut request);

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
    register_trace_root();

    let put_req = put_req.into_generic()?;

    // TODO: better mgmt for grpc port/host
    let mut client = DagStoreClient::connect(url)
        .await
        .map_err(|e| Box::new(e))?;

    // TODO: validate base58 here
    let mut request = tonic::Request::new(put_req.into_proto());
    add_tracing_to_meta(&mut request);

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
                    let url = get_ctx().dag_store_url.to_string();
                    let res = get_nodes(url, raw_hash).await;

                    match res {
                        Ok(resp) => Ok(warp::reply::json(&resp)),
                        Err(e) => {
                            error!("err on getting nodes: {:?}", e);
                            Err(reject::custom::<Error>(Error(e)))
                        }
                    }
                }
            }
        });

    let index_route = warp::get().and(warp::path::end()).and_then(|| {
        async {
            let url = get_ctx().dag_store_url.to_string();
            let res = get_initial_state(url).await;

            match res {
                Ok(resp) => {
                    info!("initial state resp: {:?}", &resp);
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
                    error!("err on get initial state: {:?}", e);
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
                let url = get_ctx().dag_store_url.to_string();
                let res = put_nodes(url, put_req).await;

                match res {
                    Ok(resp) => Ok(warp::reply::json(&resp)),
                    Err(e) => {
                        error!("err on post: {:?}", e);
                        Err(reject::custom::<Error>(Error(e)))
                    }
                }
            }
        });

    // lmao, hardcoded - would be part of deployable, ideally
    // let static_route = warp::fs::dir("/home/pk/devd/target/deploy");
    let static_route = warp::get()
        .and(warp::path::param::<String>())
        .map(
            |path: String| match notes_frontend::get_static_asset(&path) {
                None => Trivial::not_found(),
                Some(blob) => {
                    let len = blob.len() as u64;
                    // TODO: arbitrary chunk size (1024), revisit later maybe (FIXME)
                    let stream = futures::stream::iter(
                        blob.chunks(1024)
                            // .map(bytes::Bytes::from_static)
                            .map(Chunk::from)
                            .map(|x| {
                                let res: Result<Chunk, Box<dyn std::error::Error + Send + Sync>> =
                                    Ok(x);
                                res
                            }),
                    );
                    let body = hyper::Body::wrap_stream(stream);

                    let mut resp = hyper::Response::new(body);

                    let mime = mime_guess::from_path(path).first_or_octet_stream();

                    resp.headers_mut().typed_insert(headers::ContentLength(len));
                    resp.headers_mut()
                        .typed_insert(headers::ContentType::from(mime));
                    resp.headers_mut()
                        .typed_insert(headers::AcceptRanges::bytes());

                    Trivial(resp)
                    // Ok(resp)
                }
            },
        );

    let routes = get_route.or(post_route).or(index_route).or(static_route);

    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), get_ctx().port);
    warp::serve(routes).run(socket).await;
}

// FIXME: should be omitable
struct Trivial(hyper::Response<hyper::Body>);

impl Trivial {
    fn not_found() -> Self {
        let r = hyper::Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body(hyper::Body::empty())
            .unwrap(); // ASSERTION: builder will never fail

        Trivial(r)
    }
}

impl warp::Reply for Trivial {
    fn into_response(self) -> warp::reply::Response {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_store_types::types::validated_tree::ValidatedTree_;
    use notes_types::notes::*;
    use std::collections::HashMap;

    use honeycomb_tracing::TelemetryLayer;
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::Layer;
    use tracing_subscriber::registry;

    // use std::net::SocketAddr;
    use dag_store::capabilities::cache::Cache;
    use dag_store::capabilities::store::FileSystemStore;
    use std::sync::Arc;

    pub fn init_test_env() {
        let layer = TelemetryLayer::new_blackhole()
            .and_then(tracing_subscriber::fmt::Layer::builder().finish())
            .and_then(LevelFilter::INFO);

        let subscriber = layer.with_subscriber(registry::Registry::default());

        // attempt to set, failure means already set (other test suite, likely)
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

    fn spawn_dag_store(port: u16) -> tempdir::TempDir {
        let tmp_dir = tempdir::TempDir::new("dag-store-test").unwrap();
        let fs_path = tmp_dir.path().to_str().unwrap().to_string();
        let store = Arc::new(FileSystemStore::new(fs_path));

        let cache = Arc::new(Cache::new(64));

        let runtime = dag_store::server::app::Runtime {
            cache: cache,
            mutable_hash_store: store.clone(),
            hashed_blob_store: store,
        };

        let bind_to = format!("0.0.0.0:{}", &port);
        let addr = bind_to.parse().unwrap();

        tokio::spawn(async move {
            dag_store::run(runtime, addr).await.unwrap();
            ()
        });

        // return guard
        tmp_dir
    }

    #[tokio::test]
    async fn test_batch_upload() {
        init_test_env();

        let dag_store_port = 6666;
        let tmp_dir = spawn_dag_store(dag_store_port);

        // TODO: test env might have to be manual - how to express test dep on other bin in project?

        let dag_store_url = format!("http://localhost:{}", dag_store_port);

        // - get state, no hash.
        let state = get_initial_state(dag_store_url.to_string()).await.unwrap();
        assert_eq!(state, None);

        let node1 = notes_types::notes::Node {
            parent: None,
            children: vec![NodeRef::Modified(NodeId(1))],
            header: "hdr".to_string(),
            body: "body".to_string(),
        };

        let node2 = notes_types::notes::Node {
            parent: Some(NodeId::root()),
            children: Vec::new(),
            header: "hdr 2".to_string(),
            body: "body 2".to_string(),
        };

        let mut extra_nodes = HashMap::new();
        extra_nodes.insert(NodeId(1), node2);

        let tree = ValidatedTree_::validate_(node1.clone(), extra_nodes, |n| {
            n.children.clone().into_iter().filter_map(|x| match x {
                NodeRef::Modified(x) => Some(x),
                _ => None,
            })
        })
        .expect("failure validating tree while building put request");

        let put_req = notes_types::api::PutReq {
            tree,
            cas_hash: None,
        };

        // - push small tree with hash + no CAS hash
        let hash = put_nodes(dag_store_url.to_string(), put_req)
            .await
            .unwrap()
            .root_hash;

        let state = get_initial_state(dag_store_url.to_string()).await.unwrap();
        assert_eq!(state, Some(hash.clone()));

        // - get tree, recursive expansion of same (NOTE: only one layer currently)
        let get_resp = get_nodes(dag_store_url.to_string(), hash.to_string())
            .await
            .unwrap();

        // TODO: test that root node _and_ extra nodes come back through
        // test round trip
        assert_eq!(
            get_resp.requested_node.map(|n| n.0),
            node1.map(|n| n.node_id())
        );

        drop(tmp_dir);
    }
}
