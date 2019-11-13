use crate::capabilities::lib::put_and_cache;
use crate::capabilities::{HasCacheCap, HasIPFSCap};
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use dag_cache_types::types;
use dag_cache_types::types::grpc;
use dag_cache_types::types::ipfs;
use futures::stream::StreamExt;
use futures::Stream;
use grpc::{server, BulkPutReq, GetResp, IpfsHash, IpfsNode};
use honeycomb_tracing::TraceId;
use std::sync::Arc;
use tonic::{Code, Request, Response, Status};
use tracing::instrument;
use tracing::{event, info, Level};

pub struct CacheServer<C> {
    pub caps: Arc<C>,
}

#[instrument(skip(caps))]
async fn get_node_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: &C,
    request: Request<IpfsHash>,
) -> Result<Response<GetResp>, Status> {
    // extract explicit tracing id (if any)
    extract_tracing_id_and_record(request.metadata())?;

    let request = ipfs::IPFSHash::from_proto(request.into_inner()).map_err( |e| {
        event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
        e
    })?;

    let resp = opportunistic_get::get(caps, request).await?;

    let resp = resp.into_proto();
    let resp = Response::new(resp);
    Ok(resp)
}

type GetNodesStream =
    Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + Sync + 'static>;

#[instrument(skip(caps))]
async fn get_nodes_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: Arc<C>,
    request: Request<IpfsHash>,
) -> Result<Response<GetNodesStream>, Status> {
    // extract explicit tracing id (if any)
    extract_tracing_id_and_record(request.metadata())?;

    let domain_hash = ipfs::IPFSHash::from_proto(request.into_inner()).map_err( |e| {
        event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
        e
    })?;

    // TODO: wrapper that holds span, instrument, basically - should be possible! maybe build inline?
    let s = batch_get::ipfs_fetch(caps, domain_hash)
        // NOTE: tracing_futures does not yet support this, tried to impl, was hard (weird pinning voodoo)
        // .instrument(tracing::info_span!("get-nodes-stream"))
        .map(|x| match x {
            Ok(n) => Ok(n.into_proto()),
            Err(de) => Err(std::convert::From::from(de)),
        });
    let s: GetNodesStream = Box::new(s);
    let resp = Response::new(s);
    Ok(resp)
}

#[instrument(skip(caps, request))] // skip potentially-large request (TODO record stats w/o full message body)
async fn put_node_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: &C,
    request: Request<IpfsNode>,
) -> Result<Response<IpfsHash>, Status> {
    // extract explicit tracing id (if any)
    extract_tracing_id_and_record(request.metadata())?;

    let domain_node = ipfs::DagNode::from_proto(request.into_inner()).map_err( |e| {
        event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
        e
    })?;

    info!("dag cache put handler"); //TODO,, better msgs

    let hash = put_and_cache(caps, domain_node).await?;
    let proto_hash = hash.into_proto();
    let resp = Response::new(proto_hash);
    Ok(resp)
}

#[instrument(skip(caps, request))] // skip potentially-large request (TODO record stats w/o full message body)
async fn put_nodes_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: Arc<C>,
    request: Request<BulkPutReq>,
) -> Result<Response<IpfsHash>, Status> {
    // extract explicit tracing id (if any)
    extract_tracing_id_and_record(request.metadata())?;

    let bulk_put_req = types::api::bulk_put::Req::from_proto(request.into_inner()).map_err( |e| {
        event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
        e
    })?;

    info!("dag cache put handler");
    let (_size, hash) = batch_put::ipfs_publish_cata(caps, bulk_put_req.validated_tree).await?;

    let proto_hash = hash.into_proto();
    let resp = Response::new(proto_hash);
    Ok(resp)
}

// NOTE: async_trait and instrument are mutually incompatible, so use non-async-trait fns and async trait stubs
#[tonic::async_trait]
impl<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static> server::IpfsCache for CacheServer<C> {
    async fn get_node(&self, request: Request<IpfsHash>) -> Result<Response<GetResp>, Status> {
        get_node_handler(self.caps.as_ref(), request).await
    }

    type GetNodesStream = GetNodesStream;

    async fn get_nodes(
        &self,
        request: Request<IpfsHash>,
    ) -> Result<Response<Self::GetNodesStream>, Status> {
        get_nodes_handler(self.caps.clone(), request).await
    }

    async fn put_node(&self, request: Request<IpfsNode>) -> Result<Response<IpfsHash>, Status> {
        put_node_handler(self.caps.as_ref(), request).await
    }

    async fn put_nodes(&self, request: Request<BulkPutReq>) -> Result<Response<IpfsHash>, Status> {
        put_nodes_handler(self.caps.clone(), request).await
    }
}

/// Extract a tracing id from the provided metadata
fn extract_tracing_id_and_record(meta: &tonic::metadata::MetadataMap) -> Result<(), Status> {
    match meta.get("trace_id") {
        Some(s) => {
            let tracing_id = s.to_str().map_err(|e| {
                event!(Level::ERROR, msg = "unable to parse trace_id header as ascii string", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("unable to parse trace_id header as ascii string: {:?}", e),
                )
            })?;

            let tracing_id = TraceId::new(tracing_id.to_string());
            tracing_id.record_on_current_span(); // record on current span using magic downcast_ref

            Ok(())
        }
        None => Ok(()), // no-op if header not present
    }
}
