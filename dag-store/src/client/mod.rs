use futures::{StreamExt, TryStreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::{marker::PhantomData, sync::atomic::AtomicU32};

use dag_store_types::types::domain::{Header, NodeWithHash};
use dag_store_types::types::grpc;
use dag_store_types::types::{
    api,
    domain::{self, Hash},
    grpc::dag_store_client::DagStoreClient,
};
use recursion_schemes::functor::{FunctorExt, AsRefF};
use recursion_schemes::recursive::{Corecursive, Fix, RecursiveExt};
use recursion_schemes::{
    functor::{Compose, Functor, PartiallyApplied},
    recursive::Recursive,
};
use serde::{Deserialize, Serialize};
use tonic::transport::{self, Channel};

use self::shim::ClientFunctorShim;

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

pub enum MerkleLayer<X> {
    Local(Header, X),
    Remote(Header),            // remote, did not explore (perhaps due to pagination)
    ChoseNotToExplore(Header), // remote, explicitly filtered out
}

impl<X> MerkleLayer<X> {
    pub fn local(self) -> Option<X> {
        match self {
            MerkleLayer::Local(_, x) => Some(x),
            MerkleLayer::Remote(_) => None,
            MerkleLayer::ChoseNotToExplore(_) => None,
        }
    }
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
            MerkleLayer::ChoseNotToExplore(h) => MerkleLayer::ChoseNotToExplore(h),
        }
    }
}

impl AsRefF for MerkleLayer<PartiallyApplied> {
    type RefFunctor<'a> = MerkleLayer<PartiallyApplied>;

    fn as_ref<'a, A>(
        input: &'a <Self as Functor>::Layer<A>,
    ) -> <Self::RefFunctor<'a> as Functor>::Layer<&'a A> {
        match input {
            MerkleLayer::Local(hdr, x) => MerkleLayer::Local(hdr.clone(), &x),
            MerkleLayer::Remote(hdr) => MerkleLayer::Remote(hdr.clone()),
            MerkleLayer::ChoseNotToExplore(hdr) =>  MerkleLayer::ChoseNotToExplore(hdr.clone()),
        }
    }
}


pub enum BulkPutLink2<X> {
    Remote(domain::Hash),
    Local(X),
}

// TODO: will be used later when I get metadata stuff linked through
type Metadata = String;
pub trait GenerateMetadata: Functor {
    // for any X, gen metadata?
    fn gen_metadata<X>(l: <Self as Functor>::Layer<X>) -> <Self as Functor>::Layer<(Metadata, X)>;
}

pub enum BulkPutLink<X> {
    Remote(domain::Hash),
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

impl<F: Functor> Clone for Client<F> {
    fn clone(&self) -> Self {
        Self {
            underlying: self.underlying.clone(),
            _phantom: PhantomData,
        }
    }
}

// workaround for https://github.com/rust-lang/rust/issues/106832
mod shim {
    use super::*;
    pub trait ClientFunctorShim: Functor
    where
        <Self as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
        <Self as Functor>::Layer<domain::Header>: Clone,
    {
    }

    impl<X: Functor> ClientFunctorShim for X
    where
        <X as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
        <X as Functor>::Layer<domain::Header>: Clone,
    {
    }
}

impl<F: shim::ClientFunctorShim> Client<F>
where
    // TODO: remove shim after fix for https://github.com/rust-lang/rust/issues/106832
    <F as Functor>::Layer<domain::Id>: Serialize + for<'a> Deserialize<'a>,
    <F as Functor>::Layer<domain::Header>: Clone,
{
    fn encode(to_encode: F::Layer<domain::Id>) -> Vec<u8> {
        serde_json::to_vec(&to_encode).unwrap() // doesn't have to be json but makes debugging easier
    }

    // TODO: make encode the reverse of this. problem: encode is used for both single put and bulkput (different type, bulk put link)
    fn decode(node: domain::Node) -> anyhow::Result<F::Layer<domain::Header>> {
        let decoded = serde_json::from_slice(&node.data)?; // doesn't have to be json but makes debugging easier

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

    pub async fn build(path: String) -> anyhow::Result<Self> {
        let underlying = DagStoreClient::connect(path).await?;
        Ok(Self {
            underlying,
            _phantom: PhantomData,
        })
    }

    async fn get_node(&mut self, h: domain::Hash) -> anyhow::Result<F::Layer<domain::Header>> {
        // TODO: remove all that opportunistic get crap, having a filter fn removes the need for it
        // NOTE: can also have v1 be an exact matching on the string value
        let resp = self
            .underlying
            .get_node(tonic::Request::new(h.into_proto()))
            .await?;

        let decoded = Self::decode(domain::Node::from_proto(resp.into_inner())?)?;

        Ok(decoded)
    }

    async fn get_nodes<X>(&mut self, h: domain::Hash, max_elems: Option<usize>) -> anyhow::Result<X>
    where
        X: Corecursive<FunctorToken = PartialMerkleTreeLayer<F>>,
    {
        // TODO: get stream, collapse into hashmap, return that mb? or just basic ass tree structure (?)
        use futures::future;

        let node_stream = self
            .underlying
            .get_nodes(tonic::Request::new(h.into_proto()))
            .await?
            .into_inner();

        let mut chose_not_to_explore = HashSet::new();

        let node_map: HashMap<domain::Hash, F::Layer<Header>> = {
            let s = node_stream
                .map_err(|status| anyhow::Error::from(status))
                .and_then(|x| {
                    future::ready(domain::GetNodesResp::from_proto(x).map_err(anyhow::Error::from))
                })
                .try_filter_map(|x| match x {
                    domain::GetNodesResp::Node(NodeWithHash { hash, node }) => {
                        future::ready(Self::decode(node).map(|node| Some((hash, node))))
                    }
                    domain::GetNodesResp::ChoseNotToExplore(hdr) => {
                        chose_not_to_explore.insert(hdr);
                        future::ok(None)
                    }
                });

            if let Some(max_elems) = max_elems {
                s.take(max_elems).try_collect().await?
            } else {
                s.try_collect().await?
            }
        };

        let root_node = node_map.get(&h).ok_or(anyhow::Error::msg(
            "get_nodes must at least return node for root hash",
        ))?;

        let res = X::from_layer(F::fmap(root_node.clone(), |header| {
            Compose::<MerkleLayer<PartiallyApplied>, F>::expand_and_collapse(
                header,
                |header| {
                    if chose_not_to_explore.contains(&header) {
                        todo!()
                    } else {
                        match node_map.get(&header.hash) {
                            // NOTE: requires clone to handle duplicate nodes, shrug emoji (cleaner API)
                            Some(node) => MerkleLayer::Local(header, node.clone()),
                            None => MerkleLayer::Remote(header),
                        }
                    }
                },
                |layer| <MerkleLayer<PartiallyApplied> as Functor>::fmap(layer, X::from_layer),
            )
        }));

        Ok(res)
    }

    /// upload a tree of nodes with only local subnodes
    async fn put_nodes_full<X>(&mut self, local_tree: X) -> anyhow::Result<domain::Hash>
    where
        X: Recursive<FunctorToken = F>,
    {
        let local_tree =
            local_tree.fold_recursive(|layer| -> Fix<Compose<F, BulkPutLink<PartiallyApplied>>> {
                let layer = F::fmap(layer, |x| BulkPutLink::Local(x));
                Fix::new(layer)
            });
        self.put_nodes(local_tree).await
    }

    /// upload a tree of nodes, with subnodes either being local or already existing remotely
    async fn put_nodes(
        &mut self,
        local_tree: Fix<Compose<F, BulkPutLink<PartiallyApplied>>>,
    ) -> anyhow::Result<domain::Hash> {
        use recursion_schemes::recursive::RecursiveExt;

        let mut id_gen = AtomicU32::new(0);

        let mut nodes: Vec<api::bulk_put::NodeWithId> = Vec::new();
        let root_node: api::bulk_put::Node = local_tree.fold_recursive(
            |x: <F as Functor>::Layer<BulkPutLink<api::bulk_put::Node>>| {
                let mut links = Vec::new();
                let to_encode = F::fmap(x, |l| match l {
                    BulkPutLink::Remote(hash) => {
                        let id = domain::Id(id_gen.fetch_add(1, Ordering::SeqCst));
                        let header = domain::Header {
                            id,
                            hash,
                            metadata: String::new(),
                        };
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
        Ok(domain::Hash::from_proto(resp.into_inner())?)
    }

    async fn put_node(&mut self, node: F::Layer<domain::Header>) -> anyhow::Result<Hash> {
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::time::Duration;
    use std::{fmt::Debug, thread};

    use dag_store_types::test::TomlSimple;
    // TODO just move here mb
    use dag_store_types::test::{MerkleToml, MerkleTomlFunctorToken};
    use futures::future::BoxFuture;
    use futures::FutureExt;
    use proptest::{prelude::*, test_runner::*};
    use recursion_schemes::join_future::CorecursiveAsyncExt;
    use recursion_schemes::recursive::Corecursive;
    use tokio::runtime::Runtime;

    use super::*;

    enum TomlPartial {
        Map(HashMap<String, MerkleLayer<TomlPartial>>),
        List(Vec<MerkleLayer<TomlPartial>>),
        Scalar(i32),
    }

    impl TomlPartial {
        fn as_simple(self) -> Option<TomlSimple> {
            match self {
                TomlPartial::Map(xs) => xs
                    .into_iter()
                    .map(|(k, v)| v.local().and_then(|v| v.as_simple().map(|v| (k, v))))
                    .collect::<Option<_>>()
                    .map(TomlSimple::Map),
                TomlPartial::List(xs) => xs
                    .into_iter()
                    .map(|v| v.local().and_then(|v| v.as_simple()))
                    .collect::<Option<_>>()
                    .map(TomlSimple::List),
                TomlPartial::Scalar(x) => Some(TomlSimple::Scalar(x)),
            }
        }
    }

    impl Corecursive for TomlPartial {
        type FunctorToken = Compose<MerkleTomlFunctorToken<String>, MerkleLayer<PartiallyApplied>>;

        fn from_layer(x: <Self::FunctorToken as Functor>::Layer<Self>) -> Self {
            match x {
                MerkleToml::Map(xs) => {
                    TomlPartial::Map(xs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
                }
                MerkleToml::List(xs) => TomlPartial::List(xs.into_iter().collect()),
                MerkleToml::Scalar(x) => TomlPartial::Scalar(x),
            }
        }
    }

    async fn round_trip(input: TomlSimple) -> anyhow::Result<()> {
        let port = 8098; // TODO: reserve port somehow? idk
                         // spawn svc

        // Fails to connect (???, soln: just spawn a process I guess? ugh lol)
        let svc = tokio::spawn(async move {
            let opt = crate::Opt {
                port,
                fs_path: "/tmp/testdir_lazy_manual_created".to_string(), // tempdir
                max_cache_entries: NonZeroUsize::new(64).unwrap(),
            };

            let bind_to = format!("0.0.0.0:{}", &opt.port);
            let runtime = opt.into_runtime();

            let addr = bind_to.parse().unwrap();

            crate::run(runtime, addr).await.unwrap();
        });

        //wait a bit, hopefully service will be up (TODO, better?)
        thread::sleep(Duration::new(3, 0));
        // OK! fuck this is a blocker lol, need to figure this out. generic method for working with fold over _ref's_, to impl clone and etc

        println!("BUILD CLIENT");

        let mut client =
            Client::<MerkleTomlFunctorToken>::build(format!("http://0.0.0.0:{}", port))
                .await
                .unwrap();

        let hash = client.put_nodes_full(input.clone()).await?;

        let fetched: TomlPartial = client.get_nodes(hash.clone()).await?;
        let fetched = fetched.as_simple().expect("I haven't implemented stop conditions for the get_nodes stream though (maximum size or etc");

        // provided tree was just round-tripped.
        assert_eq!(input, fetched);

        // NOTE: paginated fetch would be cool even if specifics never exposed to callers of API - max batch size
        // NOTE2: paginated fetch would be cool - can add on client side if I want, just nuke stream/conn after N elements
        // NOTE: pagination and filtering need to be expressed differently, so need to have server end (eventually) send back
        // NOTE: something like "the filter says you can't have this node" VS. this node hasn't hit the stream yet
        // NOTE: sort of a 'cap'/'filter_says_no' token sent as part of the node

        // TODO futumorphism here, eventually, to deal with case where resp is paginated). async futumorphism, just like in haskell.
        // TODO lmao fun fun fun

        // use a janky async function to fetch a structure node by node, simpl async recursive
        // let fetched = fetch_recursive_naive(hash, client).await;
        let client2 = client.clone();
        let fetched = TomlSimple::unfold_recursive_async(
            hash,
            Arc::new(move |hash| {
                let client2 = client2.clone();
                async move {
                    let res = client2
                        .clone()
                        .get_node(hash)
                        .await
                        .expect("failed getting layer");
                    MerkleTomlFunctorToken::<String>::fmap(res, |hdr| hdr.hash)
                }
                .boxed()
            }),
        )
        .await;
        println!("fetched: {:?}", fetched);
        assert_eq!(input, fetched);

        Ok(())
    }

    fn fetch_recursive_naive(
        h: Hash,
        mut c: Client<MerkleTomlFunctorToken>,
    ) -> BoxFuture<'static, TomlSimple> {
        async move {
            let node = c.get_node(h).await.unwrap();

            match node {
                MerkleToml::Map(m) => {
                    let mut mm = HashMap::new();
                    for (k, h) in m.into_iter() {
                        let n = fetch_recursive_naive(h.hash.clone(), c.clone()).await;
                        mm.insert(k, n);
                    }

                    TomlSimple::Map(mm)
                }
                MerkleToml::List(l) => {
                    let mut ll = Vec::new();
                    for h in l.into_iter() {
                        let n = fetch_recursive_naive(h.hash.clone(), c.clone()).await;
                        ll.push(n);
                    }

                    TomlSimple::List(ll)
                }
                MerkleToml::Scalar(s) => TomlSimple::Scalar(s),
            }
        }
        .boxed()
    }

    fn pair(a: TomlSimple, b: TomlSimple) -> TomlSimple {
        TomlSimple::List(vec![a, b])
    }

    // needs to be multithread to work w/ spawning app to test against
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_round_trip_hardcoded_scenario() {
        let t = TomlSimple::List(vec![TomlSimple::Scalar(1), TomlSimple::Scalar(2)]);

        let h = vec![("a".to_string(), t.clone()), ("b".to_string(), t.clone())]
            .into_iter()
            .collect();

        let t = TomlSimple::Map(h);

        round_trip(t).await.unwrap();
    }

    // #[test]
    fn test_round_trip() {
        // there's only one leaf type
        let leaf = prop::arbitrary::any::<i32>().prop_map(|x| TomlSimple::Scalar(x));

        // Now define a strategy for a whole tree
        let tree = leaf.prop_recursive(
            32,  // No more than 16 branch levels deep
            128, // Target around 128 total elements
            8,   // Each collection is up to 8 elements long
            |element| {
                prop_oneof![
                    // NB `element` is an `Arc` and we'll need to reference it twice,
                    // so we clone it the first time.
                    prop::collection::vec(element.clone(), 0..16)
                        .prop_map(|xs| TomlSimple::List(xs)),
                    prop::collection::hash_map("a*", element, 0..16)
                        .prop_map(|xs| TomlSimple::Map(xs)),
                ]
            },
        );

        let mut runner = TestRunner::new(Config {
            // Turn failure persistence off for demonstration
            failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
            ..Config::default()
        });
        let result = runner.run(&tree, |v| {
            println!("SPAWN RUNTIME");
            Runtime::new()
                .unwrap()
                .block_on(async { round_trip(v).await })
                .unwrap();

            Ok(())
        });
        result.unwrap();

        // match result {
        //     Err(TestError::Fail(_, value)) => {
        //         println!("here u go: {:?}", value);
        //         // assert!(false);
        //     }
        //     result => panic!("Unexpected result: {:?}", result),
        // }
    }
}
