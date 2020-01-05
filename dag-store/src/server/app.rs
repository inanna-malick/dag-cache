use crate::capabilities::put_and_cache;
use crate::capabilities::{Cache, HashedBlobStore, MutableHashStore};
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use dag_store_types::types::{
    api, domain,
    grpc::{
        self, server, BulkPutReq, BulkPutResp, GetHashForKeyReq, GetHashForKeyResp, GetResp, Hash,
        Node,
    },
};
use futures::{Stream, StreamExt};
use honeycomb_tracing::{TraceCtx, TraceId};
use prost::Message;
use std::sync::Arc;
use tonic::{Code, Request, Response, Status};
use tracing::{event, info, instrument, Level};

// TODO (maybe): parameterize over E where E is the underlying error type (different for txn vs. main scope)
pub struct Runtime {
    pub cache: Arc<Cache>,
    pub mutable_hash_store: Arc<dyn MutableHashStore>,
    pub hashed_blob_store: Arc<dyn HashedBlobStore>,
}

type GetNodesStream = Box<dyn Stream<Item = Result<Node, Status>> + Unpin + Send + Sync + 'static>;

impl Runtime {
    #[instrument(skip(self))]
    async fn get_node_handler(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let request = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let resp =
            opportunistic_get::get(self.hashed_blob_store.clone(), self.cache.clone(), request)
                .await?;

        let resp = resp.into_proto();
        let resp = Response::new(resp);
        Ok(resp)
    }

    #[instrument(skip(self))]
    async fn get_nodes_handler(
        &self,
        request: Request<Hash>,
    ) -> Result<Response<GetNodesStream>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let domain_hash = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        // TODO: wrapper that holds span, instrument, basically - should be possible! maybe build inline?
        let s = batch_get::batch_get(
            self.hashed_blob_store.clone(),
            self.cache.clone(),
            domain_hash,
        )
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

    #[instrument(skip(self, request))] // skip potentially-large request (TODO record stats w/o full message body)
    async fn put_node_handler(&self, request: Request<Node>) -> Result<Response<Hash>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let domain_node = domain::Node::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        info!("dag cache put handler"); //TODO,, better log msgs

        let hash = put_and_cache(
            self.hashed_blob_store.clone(),
            self.cache.clone(),
            domain_node,
        )
        .await?;
        let proto_hash = hash.into_proto();
        let resp = Response::new(proto_hash);
        Ok(resp)
    }

    #[instrument(skip(self, request))] // skip potentially-large request (TODO record stats w/o full message body)
    async fn put_nodes_handler(
        &self,
        request: Request<BulkPutReq>,
    ) -> Result<Response<BulkPutResp>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let request = api::bulk_put::Req::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        info!("dag cache put handler request, cas: {:?}", &request.cas);
        let resp = batch_put::batch_put_cata_with_cas(
            self.mutable_hash_store.clone(),
            self.hashed_blob_store.clone(),
            self.cache.clone(),
            request.validated_tree,
            request.cas,
        )
        .await?;

        let resp = Response::new(resp.into_proto());
        Ok(resp)
    }

    #[instrument(skip(self, request))] // skip potentially-large request (TODO record stats w/o full message body)
    async fn get_hash_for_key_handler(
        &self,
        request: Request<GetHashForKeyReq>,
    ) -> Result<Response<GetHashForKeyResp>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let hash = self
            .mutable_hash_store
            .get(&request.into_inner().key)
            .await?
            .map(|h| h.into_proto());

        let resp = GetHashForKeyResp { hash };
        Ok(Response::new(resp))
    }
}

// NOTE: async_trait and instrument are mutually incompatible, so use non-async-trait fns and async trait stubs
#[tonic::async_trait]
impl server::DagStore for Runtime {
    async fn get_hash_for_key(
        &self,
        request: Request<GetHashForKeyReq>,
    ) -> Result<Response<GetHashForKeyResp>, Status> {
        self.get_hash_for_key_handler(request).await
    }

    async fn get_node(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        self.get_node_handler(request).await
    }

    type GetNodesStream = GetNodesStream;

    async fn get_nodes(
        &self,
        request: Request<Hash>,
    ) -> Result<Response<Self::GetNodesStream>, Status> {
        self.get_nodes_handler(request).await
    }

    async fn put_node(&self, request: Request<Node>) -> Result<Response<Hash>, Status> {
        self.put_node_handler(request).await
    }

    async fn put_nodes(
        &self,
        request: Request<BulkPutReq>,
    ) -> Result<Response<BulkPutResp>, Status> {
        self.put_nodes_handler(request).await
    }
}

/// Extract a tracing id from the provided metadata
fn extract_tracing_id_and_record(meta: &tonic::metadata::MetadataMap) -> Result<(), Status> {
    println!("got meta: {:?}", &meta);
    match meta.get_bin("trace-ctx-bin") {
        Some(b) => {
            let b = b.to_bytes().map_err(|e| {
                event!(Level::ERROR, msg = "died converting to bytes", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("died converting to bytes: {:?}", e),
                )
            })?;
            println!("got key for binary from meta: {:?}", &b);
            let trace_ctx = grpc::TraceCtx::decode(b).map_err(|e| {
                event!(Level::ERROR, msg = "unable to parse trace-ctx-bin metadata value as proto", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("unable to parse trace_id header as proto: {:?}", e),
                )
            })?;

            let trace_ctx = api::meta::trace_ctx_from_proto(trace_ctx).map_err(|e| {
                event!(Level::ERROR, msg = "unable to decode trace_ctx proto into domain form", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("unable to decode trace_ctx proto into domain form, {:?}", e),
                )
            })?;

            trace_ctx.record_on_current_span();

            Ok(())
        }
        None => {
            let trace_id = TraceId::generate();
            println!("generated trace id {:?}", &trace_id);
            TraceCtx {
                trace_id,
                parent_span: None,
            }
            .record_on_current_span();

            Ok(()) // just generate, if header not present
        }
    }
}
