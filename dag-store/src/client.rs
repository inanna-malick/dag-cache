use std::marker::PhantomData;

use dag_store_types::types::{
    api, domain::Hash,
    grpc::{dag_store_client::DagStoreClient},
};
use tonic::transport::{Channel, self};

pub struct Client<FunctorToken> {
    underlying: DagStoreClient<Channel>,
    _phantom: PhantomData<FunctorToken>,
}

impl<FunctorToken> Client<FunctorToken> {
    async fn build(path: String) -> Result<Self, transport::Error> {
        let underlying = DagStoreClient::connect(path).await?;
        Ok(Self { underlying, _phantom: PhantomData })
    }

    async fn get_batch(&mut self, hash: Hash) -> anyhow::Result<()> {
        let streaming_response = self.underlying.get_nodes(hash.into_proto()).await?;


        Ok(())
    }
}