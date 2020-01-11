use crate::types::api::bulk_put::{Node, NodeLink};
use crate::types::domain::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatedTree_<K: Eq + Hash, V> {
    pub root_node: V,
    pub nodes: HashMap<K, V>,
}

pub type ValidatedTree = ValidatedTree_<Id, Node>;

impl ValidatedTree {
    pub fn validate(
        root_node: Node,
        nodes: HashMap<Id, Node>,
    ) -> Result<ValidatedTree, ValidatedTreeBuildErr<Id>> {
        ValidatedTree::validate_(root_node, nodes, |x| {
            x.links.clone().into_iter().filter_map(|x| match x {
                NodeLink::Local(csh) => Some(csh),
                NodeLink::Remote(_) => None,
            })
        })
    }
}

// TODO: tests
impl<K: Eq + Hash, V> ValidatedTree_<K, V> {
    pub fn validate_<F: Fn(&V) -> I, I: Iterator<Item = K>>(
        root_node: V,
        nodes: HashMap<K, V>,
        extract_keys: F,
    ) -> Result<Self, ValidatedTreeBuildErr<K>> {
        let mut node_visited_count = 0;
        let mut stack = Vec::new();

        for link in extract_keys(&root_node) {
            stack.push(link);
        }

        while let Some(node_id) = stack.pop() {
            node_visited_count += 1;
            match nodes.get(&node_id) {
                Some(node) => {
                    for link in extract_keys(node) {
                        stack.push(link);
                    }
                }
                None => return Err(ValidatedTreeBuildErr::InvalidLink(node_id)),
            }
        }

        if nodes.len() == node_visited_count {
            // all nodes in map visited
            Ok(ValidatedTree_ { root_node, nodes })
        } else {
            Err(ValidatedTreeBuildErr::UnreachableNodes) // not all nodes in map are part of tree
        }
    }
}

#[derive(Debug)]
pub enum ValidatedTreeBuildErr<K> {
    InvalidLink(K),
    UnreachableNodes,
}

impl<K: std::fmt::Debug> std::fmt::Display for ValidatedTreeBuildErr<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self) // TODO: more idiomatic way of doing this
    }
}

impl<K: std::fmt::Debug> std::error::Error for ValidatedTreeBuildErr<K> {
    fn description(&self) -> &str {
        "validated tree build error"
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        None
    }
}
