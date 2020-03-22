use crate::notes::CannonicalNode;
use dag_store_types::types::domain::TypedHash;
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
pub enum Commit {
    Commit {
        parent: TypedHash<Commit>, // always at least one (NonEmpty)
        additional_parents: Vec<TypedHash<Commit>>,
        root: TypedHash<CannonicalNode>,
    },
    Null, // shared origin for all commits
}
