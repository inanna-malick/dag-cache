use crate::commits::{CannonicalCommit, Commit, CommitHash};
use crate::notes::{self, NodeId, NoteHash, RemoteNodeRef};
use dag_store_types::types::validated_tree::ValidatedTree_;
use dag_store_types::types::{api, validated_tree::ValidatedTree};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;

pub static CAS_KEY: &str = "notes-app";

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

#[derive(PartialEq, Eq, Clone, Serialize, Debug, Deserialize)]
pub enum InitialState {
    Persisted {
        commit_hash: CommitHash,
        commit: Commit<NoteHash>,
        // NOTE: using tuples so it serializes, all the json here looks like crap and could do with some hand tuning
        extra_nodes: Vec<(notes::RemoteNodeRef, notes::Node<notes::RemoteNodeRef>)>,
    },
    Fresh,
}

impl InitialState {
    pub fn from_generic(commit_hash: CommitHash, g: api::get::Resp) -> Result<Self> {
        let commit = Commit::from_generic(g.requested_node)?;

        let mut get_resp_extra_nodes = HashMap::new();
        for x in g.extra_nodes.into_iter() {
            get_resp_extra_nodes.insert(x.header.hash, x);
        }

        let mut extra_nodes = HashMap::new();
        let mut stack = vec![RemoteNodeRef(NodeId::root(), commit.root_note)];

        while let Some(next) = stack.pop() {
            if let Some(x) = get_resp_extra_nodes.remove(&next.1) {
                let id = notes::NodeId::from_generic(x.header.id)?;
                let node = notes::Node::from_generic(x.node)?;
                for remote_node_ref in node.children.iter() {
                    stack.push(*remote_node_ref);
                }
                extra_nodes.insert(notes::RemoteNodeRef(id, x.header.hash.promote()), node);
            }
        }

        let extra_nodes = extra_nodes.into_iter().collect();

        let is = InitialState::Persisted {
            commit_hash,
            commit,
            extra_nodes,
        };
        Ok(is)
    }
}

#[derive(PartialEq, Eq, Clone, Serialize, Debug, Deserialize)]
pub struct GetResp {
    pub requested_node: notes::Node<notes::RemoteNodeRef>,
    // NOTE: using tuples so it serializes, all the json here looks like crap and could do with some hand tuning
    pub extra_nodes: Vec<(notes::RemoteNodeRef, notes::Node<notes::RemoteNodeRef>)>,
}

impl GetResp {
    pub fn from_generic(g: api::get::Resp) -> Result<Self> {
        let requested_node = notes::Node::from_generic(g.requested_node)?;
        let mut extra_nodes = HashMap::new();
        for x in g.extra_nodes.into_iter() {
            let id = notes::NodeId::from_generic(x.header.id)?;
            let node = notes::Node::from_generic(x.node)?;
            extra_nodes.insert(notes::RemoteNodeRef(id, x.header.hash.promote()), node);
        }

        let extra_nodes = extra_nodes.into_iter().collect();

        let resp = GetResp {
            requested_node,
            extra_nodes,
        };
        Ok(resp)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PutReq {
    pub tree: ValidatedTree_<notes::NodeId, notes::Node<notes::NodeRef>>,
    pub parent_hash: Option<CommitHash>,
}

impl PutReq {
    pub fn into_generic(self) -> Result<api::bulk_put::Req> {
        let head_note = self.tree.root_node.into_generic()?;
        let mut extra_nodes = HashMap::new();
        for (id, node) in self.tree.nodes.into_iter() {
            let node = node.into_generic()?;
            extra_nodes.insert(id.into_generic(), node);
        }

        extra_nodes.insert(notes::NodeId::root().into_generic(), head_note);

        // TODO: create commits elsewhere - doing so here is CODE SMELL (FIXME)
        let commit: CannonicalCommit = Commit {
            parents: self.parent_hash.into_iter().collect(),
            root_note: notes::NodeId::root().into_generic(),
        };

        let validated_tree = ValidatedTree::validate(commit.into_generic(), extra_nodes)?;

        let req = api::bulk_put::Req {
            validated_tree,
            cas: Some(api::bulk_put::CAS {
                required_previous_hash: self.parent_hash.map(|x| x.demote()),
                cas_key: CAS_KEY.to_string(),
            }),
        };
        Ok(req)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum PutResp {
    Success {
        new_root_hash: NoteHash,
        new_commit_hash: CommitHash,
        additional_uploaded: Vec<(notes::NodeId, NoteHash)>,
    },
    MergeConflict(String), // TODO: structured data goes here
}

impl PutResp {
    pub fn from_generic(g: api::bulk_put::Resp) -> Result<Self> {
        // FIXME: it's weird that type param is req'd here - file bug?
        let new_commit_hash: CommitHash = g.root_hash.promote();

        let root_id = NodeId::root().into_generic();
        let index = g
            .additional_uploaded
            .iter()
            .position(|(id, _)| id == &root_id)
            .ok_or(ParseError(
                "expected root node in additional_uploaded".to_string(),
            ))?;
        let mut additional_uploaded = g.additional_uploaded;
        let (_, new_root_hash) = additional_uploaded.remove(index);
        let new_root_hash = new_root_hash.promote();

        let additional_uploaded: Result<Vec<(notes::NodeId, NoteHash)>> = additional_uploaded
            .into_iter()
            .map(|(id, h)| {
                let id = NodeId::from_generic(id)?;
                Ok((id, h.promote()))
            })
            .collect();
        let additional_uploaded = additional_uploaded?;

        let resp = PutResp::Success {
            new_commit_hash,
            new_root_hash,
            additional_uploaded,
        };
        Ok(resp)
    }
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self) // TODO: more idiomatic way of doing this
    }
}

impl std::error::Error for ParseError {
    fn description(&self) -> &str {
        &self.0
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        None
    }
}
