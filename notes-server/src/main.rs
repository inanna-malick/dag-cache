// #![deny(warnings)]

use dag_cache::generated_grpc_bindings::{self as grpc, client::IpfsCacheClient};
use dag_cache::types::api::get;
use std::str::FromStr;
use warp::{reject, Filter, Rejection, Reply};
use futures::future::FutureExt;
use serde::{Serialize};

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

/// A newtype to enforce our maximum allowed seconds.
struct HashParam(String);

impl FromStr for HashParam {
    type Err = ();
    fn from_str(src: &str) -> Result<Self, Self::Err> {
        Ok(HashParam(src.to_string())) // todo: full base58 validation
    }
}

#[tokio::main]
async fn main() {
    let get_route = warp::path("node")
        .and(warp::path::param::<String>())
        .and_then(|raw_hash| {
            let f = async move {
                println!("parsed hash {} from path", raw_hash);

                let mut client = IpfsCacheClient::connect("http://localhost:8088").await.map_err(|e| Box::new(e))?;

                let request = tonic::Request::new(grpc::IpfsHash { hash: raw_hash });

                let response = client.get_node(request).await.map_err(|e| Box::new(e))?;

                let response = get::Resp::from_proto(response.into_inner()).map_err(|e| Box::new(e))?;

                let resp = warp::reply::json(&response);
                Ok::<_, Box<dyn std::error::Error + Send + Sync + 'static>>(resp)
            };

            f.map(|x| x.map_err(|e| reject::custom::<Error>(Error(e)) ))
        });

    // note: first path segment duplicated
    // let post_route = warp::post()
    //     .and(warp::path("node"))
    //     .and(warp::body::content_length_limit(1024 * 16)) // arbitrary?
    //     .and(warp::body::json())
    //     .and_then(|rate, mut employee: Employee| {

    //         employee.rate = rate;
    //         warp::reply::json(&employee)
    //     });

    // let routes = get_route.or(post_route);

    warp::serve(get_route).run(([127, 0, 0, 1], 3030)).await;
}
