// #![deny(warnings, rust_2018_idioms)]

use crate::server::ipfscache::{server, BulkPutReq, GetResp, IpfsHash, IpfsHeader, IpfsNode};

use crate::api;
use crate::api_types;
use crate::batch_fetch;
use crate::cache::HasCacheCap;
use crate::error_types::DagCacheError;
use crate::ipfs_api::HasIPFSCap;
use crate::ipfs_types;
use crate::lib::BoxFuture;

use futures::sync::mpsc;
use futures::{future, Future, Sink, Stream};
use log::error;
// use std::collections::HashMap;
// use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
// use std::time::Instant;
use tokio::net::TcpListener;
use tower_grpc::{Request, Response, Streaming};
use tower_hyper::server::{Http, Server};

// question not the gprc macro magic (I sadly have no idea what this does)
pub mod ipfscache {
    include!(concat!(env!("OUT_DIR"), "/ipfscache.rs"));
}

struct App<C>(Arc<C>); // TODO: wrapper around arc of caps

impl<C> Clone for App<C> {
    fn clone(&self) -> Self {
        App(self.0.clone()) // not sure why the derived example was trying to clone C instead of the Arc
    }
}

impl<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static> ipfscache::server::IpfsCache for App<C> {
    type GetNodeFuture = BoxFuture<Response<GetResp>, tower_grpc::Status>;

    // note: trait requires mut here? ideally would allow non-mut as impl
    fn get_node(&mut self, request: Request<IpfsHash>) -> Self::GetNodeFuture {
        let domain_hash = ipfs_types::IPFSHash::from_proto(request.into_inner());
        let f = api::get(self.0.clone(), domain_hash)
            .map(|get_resp| {
                let proto_get_resp = get_resp.into_proto();
                Response::new(proto_get_resp)
            })
            .map_err(|domain_err| domain_err.into_status());
        Box::new(f)
    }

    type GetNodesStream = Box<dyn Stream<Item = IpfsNode, Error = tower_grpc::Status> + Send>;
    type GetNodesFuture = BoxFuture<Response<Self::GetNodesStream>, tower_grpc::Status>;

    fn get_nodes(&mut self, request: Request<IpfsHash>) -> Self::GetNodesFuture {
        let domain_hash = ipfs_types::IPFSHash::from_proto(request.into_inner());
        let s = batch_fetch::ipfs_fetch(self.0.clone(), domain_hash)
            .map(|n| n.into_proto())
            .map_err(|domain_err| domain_err.into_status());
        let resp: Response<Self::GetNodesStream> = Response::new(Box::new(s));
        Box::new(futures::future::ok(resp))
    }

    type PutNodeFuture = BoxFuture<Response<IpfsHash>, tower_grpc::Status>;

    fn put_node(&mut self, request: Request<IpfsNode>) -> Self::PutNodeFuture {
        match ipfs_types::DagNode::from_proto(request.into_inner()) {
            Ok(domain_node) => {
                let f = api::put(self.0.clone(), domain_node)
                    .map(|hash| {
                        let proto_hash = hash.into_proto();
                        Response::new(proto_hash)
                    })
                    .map_err(|domain_err| domain_err.into_status());
                Box::new(f)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                let f = futures::future::err(e.into_status());
                Box::new(f)
            }
        }
    }

    type PutNodesFuture = BoxFuture<Response<IpfsHash>, tower_grpc::Status>;

    fn put_nodes(&mut self, request: Request<BulkPutReq>) -> Self::PutNodeFuture {
        match api_types::bulk_put::Req::from_proto(request.into_inner()) {
            Ok(bulk_put_req) => {
                let f = api::put_many(self.0.clone(), bulk_put_req)
                    .map(|hash| {
                        let proto_hash = hash.into_proto();
                        Response::new(proto_hash)
                    })
                    .map_err(|domain_err| domain_err.into_status());
                Box::new(f)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                let f = futures::future::err(e.into_status());
                Box::new(f)
            }
        }
    }
}

pub fn serve<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(caps: Arc<C>) {
    let _ = ::env_logger::init(); // using this + some other tracing logger? TODO: unify

    let new_service = server::IpfsCacheServer::new(App(caps));

    let mut server = Server::new(new_service);
    let http = Http::new().http2_only(true).clone();

    let addr = "127.0.0.1:10000".parse().unwrap();
    let bind = TcpListener::bind(&addr).expect("bind");

    println!("listining on {:?}", addr);

    let serve = bind
        .incoming()
        .for_each(move |sock| {
            if let Err(e) = sock.set_nodelay(true) {
                return Err(e);
            }

            let serve = server.serve_with(sock, http.clone());
            tokio::spawn(serve.map_err(|e| error!("h2 error: {:?}", e)));

            Ok(())
        })
        .map_err(|e| eprintln!("accept error: {}", e));

    tokio::run(serve);
}
