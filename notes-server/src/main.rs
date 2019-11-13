//! A basic example showing the request components

extern crate futures;
extern crate gotham;
#[macro_use]
extern crate gotham_derive;
extern crate hyper;
extern crate mime;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate tokio;

use futures::{stream, Future, Stream};
use std::time::{Duration, Instant};

use gotham::extractor::PathExtractor;

use futures::future::{FutureExt, TryFutureExt};
use gotham::handler::{HandlerError, HandlerFuture, IntoHandlerError};
use gotham::helpers::http::response::create_response;
use gotham::router::builder::DefineSingleRoute;
use gotham::router::builder::{build_simple_router, DrawRoutes};
use gotham::router::Router;
use gotham::state::{FromState, State};
use hyper::{Body, Response, StatusCode};
use tokio::timer::Delay;

use dag_cache::generated_grpc_bindings::{self as grpc, client::IpfsCacheClient};
use dag_cache::types::api::get;
use std::error::Error;
use futures::compat::Future01CompatExt;

#[derive(Deserialize, StateData, StaticResponseExtender)]
struct HashExtractor {
    raw_hash: String,
}

fn get_handler(mut state: State) -> Box<HandlerFuture> {
    // seems a bit odd, could use on any request w/o type error?
    let hash = HashExtractor::take_from(&mut state); // lmao ugh, TODO: new framework?

    let f = async move {
        println!("async block");

        // todo: cache, maybe per thread?
        let mut client = IpfsCacheClient::connect("http://localhost:8088")
            .await
            .map_err(|e| e.into_handler_error())?;

        println!("PRE grpc send");
        let request = tonic::Request::new(grpc::IpfsHash {
            hash: hash.raw_hash.clone(),
        });

        let response = client
            .get_node(request)
            .await
            .map_err(|e| e.into_handler_error())?;

        let response =
            get::Resp::from_proto(response.into_inner()).map_err(|e| e.into_handler_error())?;

        let vec = serde_json::to_vec(&response).map_err(|e| e.into_handler_error())?;

        Ok(vec)
    };

    let f = {
        use crate::hyper::rt::Future;

        f.boxed()
            .compat()
            .then(move |res: Result<Vec<u8>, HandlerError>| match res {
                Ok(bytes) => {
                    let resp = create_response(&state, StatusCode::OK, mime::APPLICATION_JSON, bytes);
                    Ok((state, resp))
                },
                Err(err) => {
                    println!("got err {:?}", &err);
                    Err((state, err))
                },
            })
    };

    Box::new(f)
}

/// Create a `Router`.
fn router() -> Router {
    build_simple_router(|route| {
        route
            .get("/node/:raw_hash")
            .with_path_extractor::<HashExtractor>()
            .to(get_handler);
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:7878";
    println!("Listening for requests at http://{}", addr);
    // let f = gotham::init_server(addr, router());
    // f.compat().await;

    gotham::start(addr, router());
    Ok(())
}



