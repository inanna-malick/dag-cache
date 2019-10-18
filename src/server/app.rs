use crate::capabilities::lib::put_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap};
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use crate::types;
use crate::types::grpc::{server, BulkPutReq, GetResp, IpfsHash, IpfsNode};
use crate::types::ipfs;
use futures::stream::StreamExt;
use futures::Stream;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::info;
use tracing_futures::Instrument;

pub struct CacheServer<C> {
    pub caps: Arc<C>,
}

#[tonic::async_trait]
impl<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static> server::IpfsCache for CacheServer<C> {
    async fn get_node(&self, request: Request<IpfsHash>) -> Result<Response<GetResp>, Status> {
        let f = async {
            let request = ipfs::IPFSHash::from_proto(request.into_inner())?;

            let resp = opportunistic_get::get(self.caps.as_ref(), request).await?;

            let resp = resp.into_proto();
            let resp = Response::new(resp);
            Ok(resp)
        };

        f.instrument(tracing::info_span!("get-node-handler")).await
    }

    type GetNodesStream = Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + 'static>;

    async fn get_nodes(
        &self,
        request: Request<IpfsHash>,
    ) -> Result<Response<Self::GetNodesStream>, Status> {
        let f = async {
            let domain_hash = ipfs::IPFSHash::from_proto(request.into_inner())?;

            // TODO: wrapper that holds span, instrument, basically - should be possible! maybe build inline?
            let s = batch_get::ipfs_fetch(self.caps.clone(), domain_hash)
                // NOTE: tracing_futures does not yet support this, tried to impl, was hard (weird pinning voodoo)
                // .instrument(tracing::info_span!("get-nodes-stream"))
                .map(|x| match x {
                    Ok(n) => Ok(n.into_proto()),
                    Err(de) => Err(std::convert::From::from(de)),
                });
            let s: Self::GetNodesStream = Box::new(s);
            let resp = Response::new(s);
            Ok(resp)
        };

        // TODO: this will complete immediately afaik,
        // TODO: mb figure out how to make span last for lifetime of returned stream?
        f.instrument(tracing::info_span!("get-nodes-handler")).await
    }

    async fn put_node(&self, request: Request<IpfsNode>) -> Result<Response<IpfsHash>, Status> {
        let f = async {
            let domain_node = ipfs::DagNode::from_proto(request.into_inner())?;
            info!("dag cache put handler"); //TODO,, better msgs

            let caps = self.caps.clone();

            let hash = put_and_cache(caps.as_ref(), domain_node).await?;
            let proto_hash = hash.into_proto();
            let resp = Response::new(proto_hash);
            Ok(resp)
        };

        f.instrument(tracing::info_span!("put-node-handler")).await
    }

    async fn put_nodes(&self, request: Request<BulkPutReq>) -> Result<Response<IpfsHash>, Status> {
        let f = async {
            let bulk_put_req = types::api::bulk_put::Req::from_proto(request.into_inner())?;
            info!("dag cache put handler");
            let caps = self.caps.clone();

            let (_size, hash) =
                batch_put::ipfs_publish_cata(caps, bulk_put_req.validated_tree).await?;

            let proto_hash = hash.into_proto();
            let resp = Response::new(proto_hash);
            Ok(resp)
        };

        f.instrument(tracing::info_span!("put-nodes-handler")).await
    }
}
