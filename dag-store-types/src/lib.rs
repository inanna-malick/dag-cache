#![deny(warnings)]
pub mod types;

// #[cfg(feature = "test")]
pub mod test {
    use crate::types::domain;
    use core::fmt::Debug;
    use futures::FutureExt;
    use recursion_schemes::{
        functor::*,
        join_future::JoinFuture,
        recursive::{Corecursive, Fix, Recursive},
    };
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, fmt::Display};

    // simple sketch of merkle structure for tests
    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub enum MerkleToml<T, K: Eq + std::hash::Hash = String> {
        Map(HashMap<K, T>),
        List(Vec<T>),
        Scalar(i32),
    }

    pub type Toml<K = String> = Fix<MerkleTomlFunctorToken<K>>;

    pub type MerkleTomlFunctorToken<K = String> = MerkleToml<PartiallyApplied, K>;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum TomlSimple {
        Map(HashMap<String, TomlSimple>),
        List(Vec<TomlSimple>),
        Scalar(i32),
    }

    impl Corecursive for TomlSimple {
        type FunctorToken = MerkleTomlFunctorToken<String>;

        fn from_layer(x: <Self::FunctorToken as Functor>::Layer<Self>) -> Self {
            match x {
                MerkleToml::Map(xs) => {
                    TomlSimple::Map(xs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
                }
                MerkleToml::List(xs) => TomlSimple::List(xs.into_iter().collect()),
                MerkleToml::Scalar(x) => TomlSimple::Scalar(x),
            }
        }
    }

    impl Recursive for TomlSimple {
        type FunctorToken = MerkleTomlFunctorToken<String>;

        fn into_layer(self) -> <Self::FunctorToken as Functor>::Layer<Self> {
            match self {
                TomlSimple::Map(xs) => {
                    MerkleToml::Map(xs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
                }
                TomlSimple::List(xs) => MerkleToml::List(xs.into_iter().collect()),
                TomlSimple::Scalar(x) => MerkleToml::Scalar(x),
            }
        }
    }

    // janky & etc
    impl<X: Display + Eq + std::hash::Hash> Display for MerkleToml<String, X> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let s = match self {
                MerkleToml::Map(xs) => {
                    format!(
                        "Map({})",
                        // very inefficient, ehh
                        xs.iter().fold("".to_string(), |s, (k, v)| format!(
                            "{}({} -> {}), ",
                            s, k, v
                        ))
                    )
                }
                MerkleToml::List(xs) => {
                    format!(
                        "List({})",
                        // very inefficient
                        xs.iter()
                            .fold("".to_string(), |s, elem| format!("{}{}, ", s, elem))
                    )
                }
                MerkleToml::Scalar(s) => format!("Scalar({})", s),
            };

            f.write_str(&s)
        }
    }

    impl<'a> ToOwnedF for MerkleToml<PartiallyApplied, &'a str> {
        type OwnedFunctor = MerkleToml<PartiallyApplied, String>;
        fn to_owned<A>(
            input: <Self as Functor>::Layer<A>,
        ) -> <Self::OwnedFunctor as Functor>::Layer<A> {
            match input {
                MerkleToml::Map(xs) => {
                    MerkleToml::Map(xs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
                }
                MerkleToml::List(xs) => MerkleToml::List(xs.into_iter().collect()),
                MerkleToml::Scalar(x) => MerkleToml::Scalar(x),
            }
        }
    }

    impl AsRefF for MerkleToml<PartiallyApplied, String> {
        type RefFunctor<'a> = MerkleToml<PartiallyApplied, &'a str>;

        fn as_ref<'a, A>(
            input: &'a <Self as Functor>::Layer<A>,
        ) -> <Self::RefFunctor<'a> as Functor>::Layer<&'a A> {
            match input {
                MerkleToml::Map(xs) => {
                    MerkleToml::Map(xs.iter().map(|(k, v)| (&k[..], v)).collect())
                }
                MerkleToml::List(xs) => MerkleToml::List(xs.iter().collect()),
                MerkleToml::Scalar(x) => MerkleToml::Scalar(*x),
            }
        }
    }

    impl<K: Eq + std::hash::Hash + Ord> Functor for MerkleTomlFunctorToken<K> {
        type Layer<X> = MerkleToml<X, K>;

        fn fmap<F, A, B>(input: Self::Layer<A>, mut f: F) -> Self::Layer<B>
        where
            F: FnMut(A) -> B,
        {
            match input {
                MerkleToml::Map(xs) => {
                    let mut xs: Vec<_> = xs.into_iter().collect();
                    xs.sort_by(|(a, _), (b, _)| a.cmp(b)); // sort by hashmap keys to ensure same iteration order
                    MerkleToml::Map(xs.into_iter().map(|(k, v)| (k, f(v))).collect())
                }
                MerkleToml::List(xs) => MerkleToml::List(xs.into_iter().map(f).collect()),
                MerkleToml::Scalar(s) => MerkleToml::Scalar(s),
            }
        }
    }

    impl<K: Eq + std::hash::Hash + Ord + Send + Sync + 'static> JoinFuture for MerkleTomlFunctorToken<K> {
        fn join_layer<A: Send + 'static>(
            input: <Self as Functor>::Layer<futures::future::BoxFuture<'static, A>>,
        ) -> futures::future::BoxFuture<'static, <Self as Functor>::Layer<A>> {
                match input {
                    MerkleToml::Map(xs) => async {
                        let xs = futures::future::join_all(
                            xs.into_iter().map(|(k, fv)| fv.map(|v| (k, v))),
                        )
                        .await;
                        MerkleToml::Map(xs.into_iter().collect())
                    }.boxed(),
                    MerkleToml::List(xs) => async {
                        let xs = futures::future::join_all(
                            xs.into_iter(),
                        )
                        .await;
                        MerkleToml::List(xs)
                    }.boxed(),
                    MerkleToml::Scalar(s) => futures::future::ready(MerkleToml::Scalar(s)).boxed(),
                }
        }
    }

    impl<A> MerkleToml<A> {
        pub fn traverse<X, B, F: Fn(A) -> Result<B, X>>(self, f: F) -> Result<MerkleToml<B>, X> {
            Ok(match self {
                MerkleToml::Map(xs) => MerkleToml::Map(
                    xs.into_iter()
                        .map(|(k, v)| f(v).map(|x| (k, x)))
                        .collect::<Result<HashMap<String, B>, X>>()?,
                ),
                MerkleToml::List(xs) => {
                    MerkleToml::List(xs.into_iter().map(f).collect::<Result<Vec<B>, X>>()?)
                }
                MerkleToml::Scalar(s) => MerkleToml::Scalar(s),
            })
        }
    }

    impl MerkleToml<domain::Id> {
        pub fn from_str(s: &str) -> serde_json::Result<Self> {
            let x: MerkleToml<String> = serde_json::from_str(s)?;
            x.traverse(|s| s.parse().map(domain::Id))
                .map_err(|e| serde::de::Error::custom(e))
        }
        pub fn to_str(self) -> String {
            // just use string repr for ID's, json int values are FUCKY
            let node = MerkleTomlFunctorToken::fmap(self, |id| id.0.to_string());
            serde_json::to_string(&node)
                .expect("invalid serialization? nonsense, I simply choose to panic.")
        }
    }
}
