// use dag_cache_types::types::api::TypedHash;
// use dag_cache_types::types::{api, encodings, ipfs, validated_tree::ValidatedTree};
// use serde::{Deserialize, Serialize};
// use std::collections::HashMap;
// use std::error::Error;

// pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

// #[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
// pub enum Commit {
//     Commit{
//         parent: TypedHash<Commit>, // always at least one (NonEmpty)
//         merged_parents: Vec<TypedHash<Commit>>,
//         root: TypedHash<crate::notes::CannonicalNode> // NOTE: this hash points to a NoteNode, not a Commit - phantom type param time for Hash?
//     },
//     Null, // shared origin for all commits
// }
