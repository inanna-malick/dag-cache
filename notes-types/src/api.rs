use crate::notes;
use dag_store_types::types::domain::TypedHash;
use dag_store_types::types::validated_tree::ValidatedTree_;
use dag_store_types::types::{api, validated_tree::ValidatedTree};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;

pub static CAS_KEY: &str = "notes-app";

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

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

// TODO: will need to make this heterogenous - must allow tree w/ commits + notes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PutReq {
    // TODO: this fails to serialize b/c key type is not string which JSON requires
    pub tree: ValidatedTree_<notes::NodeId, notes::Node<notes::NodeRef>>,
    pub cas_hash: Option<TypedHash<notes::CannonicalNode>>,
}

impl PutReq {
    pub fn into_generic(self) -> Result<api::bulk_put::Req> {
        let head = self.tree.root_node.into_generic()?;
        let mut extra_nodes = HashMap::new();
        for (id, node) in self.tree.nodes.into_iter() {
            let node = node.into_generic()?;
            extra_nodes.insert(id.into_generic(), node);
        }

        let validated_tree = ValidatedTree::validate(head, extra_nodes)?;

        let req = api::bulk_put::Req {
            validated_tree,
            cas: Some(api::bulk_put::CAS {
                required_previous_hash: self.cas_hash.map(|x| x.demote()),
                cas_key: CAS_KEY.to_string(),
            }),
        };
        Ok(req)
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
