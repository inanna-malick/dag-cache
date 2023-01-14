use futures::TryStreamExt;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::{marker::PhantomData, sync::atomic::AtomicU32};

use dag_store_types::types::domain::{Header, NodeWithHash};
use dag_store_types::types::grpc;
use dag_store_types::types::{
    api,
    domain::{self, Hash},
    grpc::dag_store_client::DagStoreClient,
};
use recursion_schemes::functor::FunctorExt;
use recursion_schemes::recursive::{Fix, RecursiveExt};
use recursion_schemes::{
    functor::{Compose, Functor, PartiallyApplied},
    recursive::Recursive,
};
use serde::{Deserialize, Serialize};
use tonic::transport::{self, Channel};

// struct PartialMerkleTree<F: Functor> {
//     root: Hash,
//     // not all headers link to nodes in the map, if it's not in there it exists remotely
//     nodes: HashMap<Hash, F::Layer<Header>>,
// }

type PartialMerkleTreeLayer<F> = Compose<F, MerkleLayer<PartiallyApplied>>;
type PartialMerkleTree<F> = Fix<PartialMerkleTreeLayer<F>>;

// type LocalTreeLayer<F> = Compose<F, BulkPutLink<PartiallyApplied>>::Layer<Id>;

// struct LocalTree<F: Functor> {
//     root: LocalTreeLayer<F>,
//     nodes: HashMap<Id, LocalTreeLayer<F>>,
// }

enum MerkleLayer<X> {
    Local(Header, X),
    Remote(Header),
}

impl Functor for MerkleLayer<PartiallyApplied> {
    type Layer<X> = MerkleLayer<X>;

    fn fmap<F, A, B>(input: Self::Layer<A>, mut f: F) -> Self::Layer<B>
    where
        F: FnMut(A) -> B,
    {
        match input {
            MerkleLayer::Local(h, x) => MerkleLayer::Local(h, f(x)),
            MerkleLayer::Remote(h) => MerkleLayer::Remote(h),
        }
    }
}

pub enum BulkPutLink<X> {
    Remote(domain::Header),
    Local(X), // TODO: include metadata for building headers?
}

impl Functor for BulkPutLink<PartiallyApplied> {
    type Layer<X> = BulkPutLink<X>;

    fn fmap<F, A, B>(input: Self::Layer<A>, mut f: F) -> Self::Layer<B>
    where
        F: FnMut(A) -> B,
    {
        match input {
            BulkPutLink::Local(x) => BulkPutLink::Local(f(x)),
            BulkPutLink::Remote(h) => BulkPutLink::Remote(h),
        }
    }
}

pub struct Client<FunctorToken> {
    underlying: DagStoreClient<Channel>,
    _phantom: PhantomData<FunctorToken>,
}

// shit name
trait Shim {
    type EncodeDecodeable;
    type CloneableWithHeaders;

    fn encode(e: &Self::EncodeDecodeable) -> Result<Vec<u8>, anyhow::Error>;
    fn decode(e: &[u8]) -> Result<Self::EncodeDecodeable, anyhow::Error>;

    fn clone(e: &Self::CloneableWithHeaders) -> Self::CloneableWithHeaders;
}

impl<F: Functor> Shim for F
where
    F::Layer<domain::Header>: Clone,
    F::Layer<domain::Id>: Serialize,
    for<'a> F::Layer<domain::Id>: Deserialize<'a>,
{
    type EncodeDecodeable = F::Layer<domain::Id>;

    fn encode(e: &Self::EncodeDecodeable) -> Result<Vec<u8>, anyhow::Error> {
        let x = serde_json::to_vec(e)?;
        Ok(x)
    }

    fn decode(e: &[u8]) -> Result<Self::EncodeDecodeable, anyhow::Error> {
        let x = serde_json::from_slice(e)?;
        Ok(x)
    }

    type CloneableWithHeaders = F::Layer<domain::Header>;

    fn clone(e: &Self::CloneableWithHeaders) -> Self::CloneableWithHeaders {
        e.clone()
    }
}

impl<F: Functor> Client<F>
// TODO: F::Layer<X> expected to have X == Id if there's a direct bound here on F::Layer<Id>: Serialize/Deserialize later in this impl block
where
    F: Shim<
        EncodeDecodeable = F::Layer<domain::Id>,
        CloneableWithHeaders = F::Layer<domain::Header>,
    >,
    F::Layer<domain::Header>: Clone,
{
    fn encode(to_encode: F::Layer<domain::Id>) -> Vec<u8> {
        F::encode(&to_encode).unwrap() // doesn't have to be json but makes debugging easier
    }

    // TODO: make encode the reverse of this. problem: encode is used for both single put and bulkput (different type, bulk put link)
    fn decode(node: domain::Node) -> anyhow::Result<F::Layer<domain::Header>> {
        let decoded = F::decode(&node.data)?; // doesn't have to be json but makes debugging easier

        let mut headers = {
            let mut h = HashMap::new();
            for header in node.headers.into_iter() {
                h.insert(header.id, header);
            }
            h
        };

        // TODO: this needs the ability to unwrap Result/failures eg _traverse_. Until then, just panic.
        // NOTE: assumes each header is used _once_ which, idk, fine with that yolo &etc
        let res = F::fmap(decoded, |id| {
            headers.remove(&id).expect("TODO: handle this")
        });

        Ok(res)
    }

    async fn build(path: String) -> Result<Self, transport::Error> {
        let underlying = DagStoreClient::connect(path).await?;
        Ok(Self {
            underlying,
            _phantom: PhantomData,
        })
    }

    async fn get_node(mut self, h: domain::Hash) -> anyhow::Result<F::Layer<domain::Header>> {
        // TODO: remove all that opportunistic get crap, having a filter fn removes the need for it
        // NOTE: can also have v1 be an exact matching on the string value
        let resp = self
            .underlying
            .get_node(tonic::Request::new(h.into_proto()))
            .await?;

        let decoded = Self::decode(domain::Node::from_proto(resp.into_inner())?)?;

        Ok(decoded)
    }

    async fn get_nodes(mut self, h: domain::Hash) -> anyhow::Result<PartialMerkleTree<F>> {
        // TODO: get stream, collapse into hashmap, return that mb? or just basic ass tree structure (?)
        use futures::future;
        use futures::StreamExt;
        use futures::TryStreamExt;

        let node_stream = self
            .underlying
            .get_nodes(tonic::Request::new(h.into_proto()))
            .await?
            .into_inner();

        let node_map: HashMap<domain::Hash, F::Layer<Header>> = node_stream
            .map_err(|status| anyhow::Error::from(status))
            .and_then(|x| {
                future::ready(domain::NodeWithHash::from_proto(x).map_err(anyhow::Error::from))
            })
            .and_then(|NodeWithHash { hash, node }| {
                future::ready(Self::decode(node).map(|node| (hash, node)))
            })
            .try_collect()
            .await?;

        let root_node = node_map.get(&h).ok_or(anyhow::Error::msg(
            "get_nodes must at least return node for root hash",
        ))?;

        let res: Fix<Compose<F, MerkleLayer<PartiallyApplied>>> = Fix(Box::new(F::fmap(
            F::clone(root_node),
            |header: Header| -> MerkleLayer<Fix<Compose<F, MerkleLayer<PartiallyApplied>>>> {
                <Compose<MerkleLayer<PartiallyApplied>, F> as FunctorExt>::expand_and_collapse(
                        header,
                        |header: Header| -> <Compose<MerkleLayer<PartiallyApplied>, F> as Functor>::Layer<
                            Header,
                        > {
                            match node_map.get(&header.hash) {
                                // NOTE: requires clone to handle duplicate nodes, shrug emoji (cleaner API)
                                Some(node) => MerkleLayer::Local(header, F::clone(node)),
                                None => MerkleLayer::Remote(header),
                            }
                        },
                        |layer: MerkleLayer<<F as Functor>::Layer<MerkleLayer<Fix<Compose<F, MerkleLayer<PartiallyApplied>>>>>>| -> MerkleLayer<Fix<Compose<F, MerkleLayer<PartiallyApplied>>>>   {
                            <MerkleLayer<PartiallyApplied> as Functor>::fmap(layer, |x: <F as Functor>::Layer<MerkleLayer<Fix<Compose<F, MerkleLayer<PartiallyApplied>>>>>| -> Fix<Compose<F, MerkleLayer<PartiallyApplied>>> {
                                Fix(Box::new(x))
                            })
                        },
                    )
            },
        )));

        Ok(res)
    }

    /// upload a tree of nodes with only local subnodes
    async fn put_nodes_full(mut self, local_tree: Fix<F>) -> anyhow::Result<grpc::Hash> {
        let local_tree =
            local_tree.fold_recursive(|layer| -> Fix<Compose<F, BulkPutLink<PartiallyApplied>>> {
                let layer = F::fmap(layer, |x| BulkPutLink::Local(x));
                Fix(Box::new(layer))
            });
        self.put_nodes(local_tree).await
    }

    /// upload a tree of nodes, with subnodes either being local or already existing remotely
    async fn put_nodes(
        mut self,
        local_tree: Fix<Compose<F, BulkPutLink<PartiallyApplied>>>,
    ) -> anyhow::Result<grpc::Hash> {
        use recursion_schemes::recursive::RecursiveExt;

        let mut id_gen = AtomicU32::new(0);

        let mut nodes: Vec<api::bulk_put::NodeWithId> = Vec::new();
        let root_node: api::bulk_put::Node = local_tree.fold_recursive(
            |x: <F as Functor>::Layer<BulkPutLink<api::bulk_put::Node>>| {
                let mut links = Vec::new();
                let to_encode = F::fmap(x, |l| match l {
                    BulkPutLink::Remote(header) => {
                        let id = header.id.clone();
                        links.push(api::bulk_put::NodeLink::Remote(header));
                        id
                    }
                    BulkPutLink::Local(node) => {
                        let id = domain::Id(id_gen.fetch_add(1, Ordering::SeqCst));
                        nodes.push(api::bulk_put::NodeWithId { id, node });
                        links.push(api::bulk_put::NodeLink::Local(id));
                        id
                    }
                });
                api::bulk_put::Node {
                    links,
                    data: Self::encode(to_encode),
                }
            },
        );

        let put_req = grpc::BulkPutReq {
            nodes: nodes.into_iter().map(|n| n.into_proto()).collect(),
            root_node: Some(root_node.into_proto()),
        };
        let request = tonic::Request::new(put_req);
        let resp = self.underlying.put_nodes(request).await?;
        Ok(resp.into_inner())
    }

    async fn put_node(mut self, node: F::Layer<domain::Header>) -> anyhow::Result<Hash> {
        let mut headers = Vec::new();
        let to_encode = F::fmap(node, |h| {
            let id = h.id.clone();
            headers.push(h);
            id
        });
        let node = domain::Node {
            headers,
            data: Self::encode(to_encode),
        };
        let request = tonic::Request::new(node.into_proto());
        let response = self.underlying.put_node(request).await?;
        let hash = domain::Hash::from_proto(response.into_inner())?;

        Ok(hash)
    }

    async fn get_batch(&mut self, hash: Hash) -> anyhow::Result<()> {
        let streaming_response = self.underlying.get_nodes(hash.into_proto()).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_round_trip() {}
}
