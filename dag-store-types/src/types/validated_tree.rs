use crate::types::api::bulk_put::{Node, NodeLink};
use crate::types::api::ClientId;
use std::collections::HashMap;

// ephemeral, used for data structure in memory
#[derive(Debug)]
pub struct ValidatedTree {
    pub root_node: Node,
    pub nodes: HashMap<ClientId, Node>,
}

// TODO: tests
impl ValidatedTree {
    pub fn validate(
        root_node: Node,
        nodes: HashMap<ClientId, Node>,
    ) -> Result<ValidatedTree, DagTreeBuildErr> {
        let mut node_visited_count = 0;
        let mut stack = Vec::new();

        for node_link in root_node.links.iter() {
            match node_link {
                // reference to node in map, must verify
                NodeLink::Local(csh) => stack.push(csh.clone()),
                // no-op, valid by definition
                NodeLink::Remote(_) => {}
            }
        }

        while let Some(node_id) = stack.pop() {
            node_visited_count += 1;
            match nodes.get(&node_id) {
                Some(node) => {
                    for node_link in node.links.iter() {
                        match node_link {
                            // reference to node in map, must verify
                            NodeLink::Local(csh) => stack.push(csh.clone()),
                            // no-op, valid by definition
                            NodeLink::Remote(_) => {}
                        }
                    }
                }
                None => return Err(DagTreeBuildErr::InvalidLink(node_id)),
            }
        }

        if nodes.len() == node_visited_count {
            // all nodes in map visited
            Ok(ValidatedTree { root_node, nodes })
        } else {
            Err(DagTreeBuildErr::UnreachableNodes) // not all nodes in map are part of tree
        }
    }
}

#[derive(Debug)]
pub enum DagTreeBuildErr {
    InvalidLink(ClientId),
    UnreachableNodes,
}

impl std::fmt::Display for DagTreeBuildErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self) // TODO: more idiomatic way of doing this
    }
}

impl std::error::Error for DagTreeBuildErr {
    fn description(&self) -> &str {
        "dag cache build error"
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        None
    }
}
