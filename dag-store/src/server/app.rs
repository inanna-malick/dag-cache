use crate::capabilities::put_and_cache;
use crate::capabilities::{Cache, HashedBlobStore};
use crate::server::batch_get;
use crate::server::batch_put;
use crate::server::opportunistic_get;
use dag_store_types::types::{
    api, domain,
    grpc::{dag_store_server::DagStore, BulkPutReq, BulkPutResp, GetResp, Hash, Node},
};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{event, info, instrument, Level};

use futures::{Stream, StreamExt};
use std::pin::Pin;

// TODO (maybe): parameterize over E where E is the underlying error type (different for txn vs. main scope)
pub struct Runtime {
    pub cache: Arc<Cache>,
    pub hashed_blob_store: Arc<dyn HashedBlobStore>,
}

impl Runtime {
    #[instrument(skip(self))]
    async fn get_node_handler(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        let request = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let resp = opportunistic_get::get(&self.hashed_blob_store, &self.cache, request).await?;

        let resp = resp.into_proto();
        let resp = Response::new(resp);
        Ok(resp)
    }

    #[instrument(skip(self))]
    fn get_nodes_handler(
        &self,
        request: Request<Hash>,
    ) -> Result<Response<GetNodesStream>, Status> {
        let hash = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let stream = batch_get::batch_get(&self.hashed_blob_store, &self.cache, hash);

        let stream = stream
            .map(|elem| match elem {
                Ok(n) => Ok(n.into_proto()),
                Err(e) => Err(e.into()),
            })
            .boxed();

        Ok(Response::new(stream))
    }

    #[instrument(skip(self, request))] // skip potentially-large request (TODO record stats w/o full message body)
    async fn put_node_handler(&self, request: Request<Node>) -> Result<Response<Hash>, Status> {
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
        let request = api::bulk_put::Req::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let resp =
            batch_put::batch_put_cata(&self.hashed_blob_store, &self.cache, request.validated_tree)
                .await?;

        let resp = Response::new(resp.into_proto());
        Ok(resp)
    }
}

type GetNodesStream = Pin<Box<dyn Stream<Item = Result<Node, Status>> + Send + 'static>>;

// NOTE: async_trait and instrument are mutually incompatible, so use non-async-trait fns and async trait stubs
#[tonic::async_trait]
impl DagStore for Runtime {
    async fn get_node(&self, request: Request<Hash>) -> Result<Response<GetResp>, Status> {
        self.get_node_handler(request).await
    }

    async fn put_node(&self, request: Request<Node>) -> Result<Response<Hash>, Status> {
        self.put_node_handler(request).await
    }

    type GetNodesStream = GetNodesStream;

    async fn get_nodes(
        &self,
        request: Request<Hash>,
    ) -> Result<Response<Self::GetNodesStream>, Status> {
        self.get_nodes_handler(request)
    }

    async fn put_nodes(
        &self,
        request: Request<BulkPutReq>,
    ) -> Result<Response<BulkPutResp>, Status> {
        self.put_nodes_handler(request).await
    }
}
