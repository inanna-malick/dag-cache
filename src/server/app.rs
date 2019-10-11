use crate::capabilities::lib::put_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::lib::BoxFuture;
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use crate::types;
use crate::types::errors::DagCacheError;
use crate::types::grpc::{server, BulkPutReq, GetResp, IpfsHash, IpfsNode};
use crate::types::ipfs;
use futures::future::{FutureExt, TryFutureExt};
use futures01::{Future, Stream};
use std::sync::Arc;
use tower_grpc::{Request, Response};
use tracing::info;
use futures::stream::TryStreamExt;

pub struct Server<C> {
    pub caps: Arc<C>,
}

// not sure why but derived Clone was trying to clone C instead of the Arc,
// this shouldn't really be required as an explicit instance
impl<C> Clone for Server<C> {
    fn clone(&self) -> Self {
        Server {
            caps: self.caps.clone(),
        }
    }
}

async fn get_node_async<C: HasCacheCap + HasTelemetryCap + HasIPFSCap + Sync + Send + 'static>(
    caps: Arc<C>,
    request: Request<IpfsHash>,
) -> Result<Response<GetResp>, tower_grpc::Status> {
    let request = ipfs::IPFSHash::from_proto(request.into_inner()).map_err(|de| {
        let e = DagCacheError::ProtoDecodingError(de);
        e.into_status()
    })?;

    let resp = opportunistic_get::get(caps, request)
        .await
        .map_err(|de| de.into_status())?;

    let resp = resp.into_proto();
    let resp = Response::new(resp);
    Ok(resp)
}

impl<C: HasCacheCap + HasTelemetryCap + HasIPFSCap + Sync + Send + 'static> server::IpfsCache
    for Server<C>
{
    type GetNodeFuture = BoxFuture<Response<GetResp>, tower_grpc::Status>;

    // note: trait requires mut here? ideally would allow non-mut as impl
    fn get_node(&mut self, request: Request<IpfsHash>) -> Self::GetNodeFuture {
        // note: boxed() uses box to provide UnPin - not == to Box::new
        let f = get_node_async(self.caps.clone(), request).boxed().compat();
        Box::new(f)
    }

    type GetNodesStream = Box<dyn Stream<Item = IpfsNode, Error = tower_grpc::Status> + Send>;
    type GetNodesFuture = BoxFuture<Response<Self::GetNodesStream>, tower_grpc::Status>;

    fn get_nodes(&mut self, request: Request<IpfsHash>) -> Self::GetNodesFuture {
        match ipfs::IPFSHash::from_proto(request.into_inner()) {
            Ok(domain_hash) => {
                let s = batch_get::ipfs_fetch(self.caps.clone(), domain_hash)
                    .compat()
                    .map(|n: ipfs::DagNode| n.into_proto())
                    .map_err(|domain_err| domain_err.into_status());
                let resp: Response<Self::GetNodesStream> = Response::new(Box::new(s));
                Box::new(futures01::future::ok(resp))
            }

            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                let f = futures01::future::err(e.into_status());
                Box::new(f)
            }
        }
    }

    type PutNodeFuture = BoxFuture<Response<IpfsHash>, tower_grpc::Status>;

    fn put_node(&mut self, request: Request<IpfsNode>) -> Self::PutNodeFuture {
        match ipfs::DagNode::from_proto(request.into_inner()) {
            Ok(domain_node) => {
                info!("dag cache put handler"); //TODO,, better msgs

                let caps = self.caps.clone();

                let f = put_and_cache(caps, domain_node)
                    .map(|hash| {
                        let proto_hash = hash.into_proto();
                        Response::new(proto_hash)
                    })
                    .map_err(|domain_err| domain_err.into_status());
                Box::new(f)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                let f = futures01::future::err(e.into_status());
                Box::new(f)
            }
        }
    }

    type PutNodesFuture = BoxFuture<Response<IpfsHash>, tower_grpc::Status>;

    fn put_nodes(&mut self, request: Request<BulkPutReq>) -> Self::PutNodeFuture {
        match types::api::bulk_put::Req::from_proto(request.into_inner()) {
            Ok(bulk_put_req) => {
                info!("dag cache put handler");
                let caps = self.caps.clone();

                let f = batch_put::ipfs_publish_cata(caps, bulk_put_req.validated_tree)
                    .map(|(_size, hash)| {
                        let proto_hash = hash.into_proto();
                        Response::new(proto_hash)
                    })
                    .map_err(|domain_err| domain_err.into_status());

                Box::new(f)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                let f = futures01::future::err(e.into_status());
                Box::new(f)
            }
        }
    }
}
