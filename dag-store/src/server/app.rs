use crate::capabilities::put_and_cache;
use crate::capabilities::{Cache, HashedBlobStore, MutableHashStore};
use crate::server::batch_put;
use crate::server::opportunistic_get;
use dag_store_types::types::{
    api, domain,
    grpc::{
        dag_store_server::DagStore, BulkPutReq, BulkPutResp, GetHashForKeyReq, GetHashForKeyResp,
        GetResp, Hash, Node,
    },
};
use std::{str::FromStr, sync::Arc};
use tonic::{Code, Request, Response, Status};
use tracing::{event, info, instrument, Level};
use tracing_honeycomb::{register_dist_tracing_root, SpanId, TraceId};

// TODO (maybe): parameterize over E where E is the underlying error type (different for txn vs. main scope)
pub struct Runtime {
    pub cache: Arc<Cache>,
    pub mutable_hash_store: Arc<dyn MutableHashStore>,
    pub hashed_blob_store: Arc<dyn HashedBlobStore>,
}

impl Runtime {
    #[instrument(skip(self))]
    async fn get_node_handler(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        // extract explicit tracing id (if any)
        extract_tracing_id_and_record(request.metadata())?;

        let request = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let resp = opportunistic_get::get(&self.hashed_blob_store, &self.cache, request).await?;

        let resp = resp.into_proto();
        let resp = Response::new(resp);
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

        let hash = put_and_cache(&self.hashed_blob_store, &self.cache, domain_node).await?;
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
            &self.mutable_hash_store,
            &self.hashed_blob_store,
            &self.cache,
            request.validated_tree,
            request.cas,
        )
        .await?;

        let resp = Response::new(resp.into_proto());
        Ok(resp)
    }

    // TODO: corresponding put method
    #[instrument(skip(self))]
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
impl DagStore for Runtime {
    async fn get_hash_for_key(
        &self,
        request: Request<GetHashForKeyReq>,
    ) -> Result<Response<GetHashForKeyResp>, Status> {
        self.get_hash_for_key_handler(request).await
    }

    async fn get_node(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        self.get_node_handler(request).await
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
    match (
        meta.get(TraceId::meta_field_name()),
        meta.get(SpanId::meta_field_name()),
    ) {
        (Some(trace_id), Some(parent_span_id)) => {
            let trace_id = trace_id.to_str().map_err(|e| {
                event!(Level::ERROR, msg = "trace id metadata not valid ascii", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("trace id metadata not valid ascii, {:?}", e),
                )
            })?;
            let trace_id = TraceId::from_str(trace_id).map_err(|e| {
                event!(Level::ERROR, msg = "died parsing trace id from metadata", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("died parsing trace id from metadata, {:?}", e),
                )
            })?;

            let parent_span_id = parent_span_id.to_str().map_err(|e| {
                event!(Level::ERROR, msg = "parent span id metadata not valid ascii", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("parent span id metadata not valid ascii, {:?}", e),
                )
            })?;
            let parent_span_id = SpanId::from_str(parent_span_id).map_err(|e| {
                event!(Level::ERROR, msg = "died parsing span id from metadata", error = ?e);
                Status::new(
                    Code::InvalidArgument,
                    format!("died parsing span id from metadata, {:?}", e),
                )
            })?;

            register_dist_tracing_root(trace_id, Some(parent_span_id)).unwrap();

            Ok(())
        }
        (Some(_), None) | (None, Some(_)) => {
            event!(
                Level::ERROR,
                msg = "provided trace id but not span id or vice versa"
            );
            let err = Status::new(
                Code::InvalidArgument,
                format!("metadata included trace id but not span id or vice versa"),
            );
            Err(err)
        }
        (None, None) => {
            // register as top-level trace root
            let trace_id = TraceId::generate();
            register_dist_tracing_root(trace_id, None).unwrap();

            Ok(())
        }
    }
}
