// mod ipfs_types;
use crate::api_types::ClientSideHash;
use crate::api_types::bulk_put::{DagNodeLink, DagNode};
use hashbrown::HashMap;


// ephemeral, used for data structure in memory
pub struct ValidatedTree { // how 2 make constructor priv but fields pub? just add pub accessors?
    pub root: ClientSideHash,
    pub nodes: HashMap<ClientSideHash, DagNode>,
}

// TODO: idk, tests?
impl ValidatedTree {
    pub fn validate(
        root: ClientSideHash,
        nodes: HashMap<ClientSideHash, DagNode>,
    ) -> Result<ValidatedTree, DagTreeBuildErr> {
        let mut node_visited_count = 0;
        let mut stack = vec![];

        stack.push(root.clone());

        while let Some(node_id) = stack.pop() {
            node_visited_count += 1;
            match nodes.get(&node_id) {
                Some(node) => {
                    for node_link in node.links.iter() {
                        match node_link {
                            // reference to node in map, must verify
                            DagNodeLink::Local(csh) => stack.push(csh.clone()),
                            // no-op, valid by definition
                            DagNodeLink::Remote(_) => {}
                        }
                    }
                }
                None => return Err(DagTreeBuildErr::InvalidLink(node_id)),
            }
        }

        if nodes.len() == node_visited_count {
            // all nodes in map visited
            Ok(ValidatedTree{root, nodes})
        } else {
            Err(DagTreeBuildErr::UnreachableNodes) // not all nodes in map are part of tree
        }
    }
}

#[derive(Debug)]
pub enum DagTreeBuildErr {
    InvalidLink(ClientSideHash),
    UnreachableNodes,
}
