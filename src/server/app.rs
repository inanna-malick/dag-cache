use crate::capabilities::lib::put_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap, HasTelemetryCap};
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use crate::types;
use crate::types::errors::DagCacheError;
use crate::types::grpc::{server, BulkPutReq, GetResp, IpfsHash, IpfsNode};
use crate::types::ipfs;
use futures::stream::StreamExt;
use futures::Stream;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct CacheServer<C> {
    pub caps: Arc<C>,
}

// not sure why but derived Clone was trying to clone C instead of the Arc,
// this shouldn't really be required as an explicit instance
impl<C> Clone for CacheServer<C> {
    fn clone(&self) -> Self {
        CacheServer {
            caps: self.caps.clone(),
        }
    }
}

#[tonic::async_trait]
impl<C: HasCacheCap + HasTelemetryCap + HasIPFSCap + Sync + Send + 'static> server::IpfsCache
    for CacheServer<C>
{
    async fn get_node(&self, request: Request<IpfsHash>) -> Result<Response<GetResp>, Status> {
        let request = ipfs::IPFSHash::from_proto(request.into_inner()).map_err(|de| {
            let e = DagCacheError::ProtoDecodingError(de);
            e.into_status()
        })?;

        let resp = opportunistic_get::get(self.caps.clone(), request)
            .await
            .map_err(|de| de.into_status())?;

        let resp = resp.into_proto();
        let resp = Response::new(resp);
        Ok(resp)
    }

    type GetNodesStream = Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + 'static>;

    async fn get_nodes(
        &self,
        request: Request<IpfsHash>,
    ) -> Result<Response<Self::GetNodesStream>, Status> {
        match ipfs::IPFSHash::from_proto(request.into_inner()) {
            Ok(domain_hash) => {
                let s = batch_get::ipfs_fetch(self.caps.clone(), domain_hash).map(|x| match x {
                    Ok(n) => Ok(n.into_proto()),
                    Err(de) => Err(de.into_status()),
                });
                let s: Self::GetNodesStream = Box::new(s);
                let resp = Response::new(s);
                Ok(resp)
            }

            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                Err(e.into_status())
            }
        }
    }

    async fn put_node(&self, request: Request<IpfsNode>) -> Result<Response<IpfsHash>, Status> {
        match ipfs::DagNode::from_proto(request.into_inner()) {
            Ok(domain_node) => {
                info!("dag cache put handler"); //TODO,, better msgs

                let caps = self.caps.clone();

                let hash = put_and_cache(caps, domain_node)
                    .await
                    .map_err(|domain_err| domain_err.into_status())?;
                let proto_hash = hash.into_proto();
                let resp = Response::new(proto_hash);
                Ok(resp)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                Err(e.into_status())
            }
        }
    }

    async fn put_nodes(&self, request: Request<BulkPutReq>) -> Result<Response<IpfsHash>, Status> {
        match types::api::bulk_put::Req::from_proto(request.into_inner()) {
            Ok(bulk_put_req) => {
                info!("dag cache put handler");
                let caps = self.caps.clone();

                let (_size, hash) = batch_put::ipfs_publish_cata(caps, bulk_put_req.validated_tree)
                    .await
                    .map_err(|domain_err| domain_err.into_status())?;

                let proto_hash = hash.into_proto();
                let resp = Response::new(proto_hash);
                Ok(resp)
            }
            Err(de) => {
                let e = DagCacheError::ProtoDecodingError(de);
                Err(e.into_status())
            }
        }
    }
}
