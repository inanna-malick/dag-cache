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
use std::convert::TryInto;
use std::sync::Arc;
use tonic::{Code, Request, Response, Status};
use tracing::info;
use tracing::instrument;

pub struct CacheServer<C> {
    pub caps: Arc<C>,
}

// for parsing out tracing id from binary metadata (if I still end up doing that)
fn read_be_u64(input: &mut &[u8]) -> Result<u64, std::array::TryFromSliceError> {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u64>());
    *input = rest;
    let int_bytes = int_bytes.try_into()?;
    Ok(u64::from_be_bytes(int_bytes))
}

// janky fn name.. code smell..
fn extract_or_gen_tracing_id_and_record(
    meta: &tonic::metadata::MetadataMap,
) -> Result<(), Status> {
    match meta.get("trace_id") {
        Some(s) => {
            let valid_string =  s.to_str().map_err(|e| {
                    Status::new(
                        Code::InvalidArgument,
                        format!("unable to parse trace_id header as ascii string: {:?}", e),
                    )
            })?;

            let decoded_bytes = base58::FromBase58::from_base58(valid_string).map_err(|e| {
                Status::new(
                    Code::InvalidArgument,
                    format!("unable to parse trace_id header as base58: {:?}", e),
                )
            })?;

            // FIXME: weird double-pointer thing here.. investigate read_be_u64 in detail..
            let tracing_id = read_be_u64(&mut &decoded_bytes[..]).map_err(|e| {
                Status::new(
                    Code::InvalidArgument,
                    format!(
                        "unable to convert provided base58 trace_id header into u64: {}", e
                    ),
                )
            })?;

            // let tracing_id = u64::from_be_bytes(u8_bytes);

            tracing::Span::current().record("trace_id", &tracing_id);

            Ok(())
        }
        None => Ok(()), // no-op if header not present
    }
}

#[instrument(skip(caps))]
async fn get_node_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: &C,
    request: Request<IpfsHash>,
) -> Result<Response<GetResp>, Status> {
    // extract explicit tracing id (if any)
    extract_or_gen_tracing_id_and_record(request.metadata())?;

    let request = ipfs::IPFSHash::from_proto(request.into_inner())?;

    let resp = opportunistic_get::get(caps, request).await?;

    let resp = resp.into_proto();
    let resp = Response::new(resp);
    Ok(resp)
}

#[instrument(skip(caps))]
async fn get_nodes_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: Arc<C>,
    request: Request<IpfsHash>,
) -> Result<
    Response<Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + 'static>>,
    Status,
> {
    // extract explicit tracing id (if any)
    extract_or_gen_tracing_id_and_record(request.metadata())?;

    let domain_hash = ipfs::IPFSHash::from_proto(request.into_inner())?;

    // TODO: wrapper that holds span, instrument, basically - should be possible! maybe build inline?
    let s = batch_get::ipfs_fetch(caps, domain_hash)
        // NOTE: tracing_futures does not yet support this, tried to impl, was hard (weird pinning voodoo)
        // .instrument(tracing::info_span!("get-nodes-stream"))
        .map(|x| match x {
            Ok(n) => Ok(n.into_proto()),
            Err(de) => Err(std::convert::From::from(de)),
        });
    let s: Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + 'static> = Box::new(s);
    let resp = Response::new(s);
    Ok(resp)
}

#[instrument(skip(caps, request))] // skip potentially-large request (TODO record stats w/o full message body)
async fn put_node_handler<C: HasCacheCap + HasIPFSCap + Sync + Send + 'static>(
    caps: &C,
    request: Request<IpfsNode>,
) -> Result<Response<IpfsHash>, Status> {
    // extract explicit tracing id (if any)
    extract_or_gen_tracing_id_and_record(request.metadata())?;

    let domain_node = ipfs::DagNode::from_proto(request.into_inner())?;
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
    extract_or_gen_tracing_id_and_record(request.metadata())?;

    let bulk_put_req = types::api::bulk_put::Req::from_proto(request.into_inner())?;
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

    type GetNodesStream = Box<dyn Stream<Item = Result<IpfsNode, Status>> + Unpin + Send + 'static>;

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
