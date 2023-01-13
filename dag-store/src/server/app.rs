use crate::capabilities::{get_and_cache, put_and_cache};
use crate::capabilities::{Cache, HashedBlobStore};
use crate::server::batch_get;
use crate::server::batch_put;
use dag_store_types::types::{
    api, domain,
    grpc::{dag_store_server::DagStore, BulkPutReq, GetNodesReq, Hash, Node, NodeWithHash},
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
    async fn get_node_handler(&self, request: Request<Hash>) -> Result<Response<Node>, Status> {
        let hash = domain::Hash::from_proto(request.into_inner()).map_err( |e| {
            event!(Level::ERROR, msg = "unable to parse request proto as valid domain object", error = ?e);
            e
        })?;

        let node = get_and_cache(&self.hashed_blob_store, &self.cache, hash).await?;

        let resp = Response::new(node.into_proto());
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

        // TODO: figure out strat for traversal filtering, probably start with an enum:
        // StringMatch: str (exact match on metadata)
        // IfInCache: str (exact match on metadata)
        // All
        // OR: make my life simpler (?), it's just js: you get (metadata, is_in_cache) pair and filter it
        //  BUT: what if it drops from cache in the meantime b/c this is slow and etc
        //  A: I guess reserve its slot in the cache? or just rerun the fn ugh lol
        // NOTE: this kinda sucks and I don't want to do it, so instead it's just a "hey does this exact string match" type situation
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
    ) -> Result<Response<Hash>, Status> {
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

type GetNodesStream = Pin<Box<dyn Stream<Item = Result<NodeWithHash, Status>> + Send + 'static>>;

// NOTE: async_trait and instrument are mutually incompatible, so use non-async-trait fns and async trait stubs
#[tonic::async_trait]
impl DagStore for Runtime {
    async fn get_node(&self, request: Request<Hash>) -> Result<Response<Node>, Status> {
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

    async fn put_nodes(&self, request: Request<BulkPutReq>) -> Result<Response<Hash>, Status> {
        self.put_nodes_handler(request).await
    }
}
