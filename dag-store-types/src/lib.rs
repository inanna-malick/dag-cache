#![deny(warnings)]
pub mod types;


// #[cfg(feature = "test")]
pub mod test {
    use crate::types::domain::Id;
    use recursion_schemes::functor::{Functor, PartiallyApplied};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    // simple sketch of merkle structure for tests
    #[derive(Serialize, Deserialize)]
    pub enum MerkleToml<T> {
        Map(HashMap<String, T>),
        List(Vec<T>),
        Scalar(String),
    }

    impl Functor for MerkleToml<PartiallyApplied> {
        type Layer<X> = MerkleToml<X>;

        fn fmap<F, A, B>(input: Self::Layer<A>, mut f: F) -> Self::Layer<B>
    where
        F: FnMut(A) -> B {
            match input {
                MerkleToml::Map(xs) => {
                    MerkleToml::Map(xs.into_iter().map(|(k, v)| (k, f(v))).collect())
                }
                MerkleToml::List(xs) => MerkleToml::List(xs.into_iter().map(f).collect()),
                MerkleToml::Scalar(s) => MerkleToml::Scalar(s),
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

    impl MerkleToml<Id> {
        pub fn from_str(s: &str) -> serde_json::Result<Self> {
            let x: MerkleToml<String> = serde_json::from_str(s)?;
            x.traverse(|s| s.parse().map(Id)).map_err(|e| serde::de::Error::custom(e))
        }
        pub fn to_str(self) -> String {
            // just use string repr for ID's, json int values are FUCKY
            let node = MerkleToml::<PartiallyApplied>::fmap(self, |id| id.0.to_string());
            serde_json::to_string(&node).expect("invalid serialization? nonsense, I simply choose to panic.")
        }
    }
}
