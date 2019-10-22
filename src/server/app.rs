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
use std::convert::TryInto;

pub struct CacheServer<C> {
    pub caps: Arc<C>,
}

// for parsing out tracing id from binary metadata
fn read_be_u64(input: &mut &[u8]) -> u64 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u64>());
    *input = rest;
    u64::from_be_bytes(int_bytes.try_into().unwrap()) // FIXME: error handling, maybe. fails if insufficient u8's
}

#[tonic::async_trait]
impl<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static> server::IpfsCache for CacheServer<C> {
    async fn get_node(&self, request: Request<IpfsHash>) -> Result<Response<GetResp>, Status> {
        let x: u64 = 1234561235;
        let span = tracing::info_span!("get-node-handler", trace_id = x);

        // TODO: some more durable method of passing this in
        // if let Some(k) = request.metadata().get("trace_id") {
        //     let mut trace_id = k.as_bytes().clone();
        //     let trace_id: u64 = read_be_u64(&mut trace_id);
        //     println!("record explicit trace id {}", &trace_id);
        //     // FIXME: magic string (trace_id) known to telemetry subscriber
        //     span.record("trace_id", &trace_id);
        //     // TODO: error handling req'd for malformed tracing ids
        // } else {
        //     println!("trace id not found on req, md: {:?}", request.metadata());
        // };

        let f = async {

            // validation
            let request = ipfs::IPFSHash::from_proto(request.into_inner())?;

            let resp = opportunistic_get::get(self.caps.as_ref(), request).await?;

            let resp = resp.into_proto();
            let resp = Response::new(resp);
            Ok(resp)
        };

        // a bit janky - just take first u8
        f.instrument(span).await
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
