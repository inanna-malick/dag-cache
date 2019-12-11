use crate::api::{ParseError, Result};
use dag_cache_types::types::{
    api,
    domain::{self, TypedHash},
    encodings,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(PartialEq, Eq, Copy, Clone, Hash, Serialize, Deserialize, Debug)]
pub struct NodeId(pub u128);

impl NodeId {
    pub fn from_generic(g: String) -> Result<Self> {
        let id = u128::from_str_radix(&g, 10)?; // panics if invalid...
        Ok(NodeId(id))
    }

    pub fn into_generic(self) -> api::ClientId {
        api::ClientId(format!("{}", self.0))
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Serialize, Deserialize, Debug)]
pub enum NodeRef {
    Modified(NodeId),
    Unmodified(RemoteNodeRef),
}

impl NodeRef {
    pub fn node_id(&self) -> NodeId {
        match self {
            NodeRef::Modified(id) => *id,
            NodeRef::Unmodified(RemoteNodeRef(id, _hash)) => *id,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Serialize, Deserialize, Debug)]
pub struct RemoteNodeRef(pub NodeId, pub TypedHash<CannonicalNode>);

#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
pub struct Node<T> {
    pub parent: Option<NodeId>, // _not_ T, constant type. NOTE: enforces that this is a TREE and not a DAG
    pub children: Vec<T>,
    pub header: String,
    pub body: String,
}

impl<T> Node<T> {
    pub fn map_mut<F: Fn(&mut T)>(&mut self, f: F) {
        for x in self.children.iter_mut() {
            f(x)
        }
    }

    pub fn map<X, F: Fn(T) -> X>(self, f: F) -> Node<X> {
        Node {
            parent: self.parent,
            children: self.children.into_iter().map(f).collect(),
            header: self.header,
            body: self.body,
        }
    }
}

// cannonical format (this is what is written to the dag store)
// TODO: consider exporting _this_ as 'Node', audit usage patterns for this type..
pub type CannonicalNode = Node<NodeId>;
impl CannonicalNode {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let res = serde_json::to_vec(self)?;
        Ok(res)
    }

    pub fn decode(v: &[u8]) -> Result<Self> {
        let res = serde_json::from_slice(v)?;
        Ok(res)
    }
}

impl Node<RemoteNodeRef> {
    pub fn from_generic(g: domain::Node) -> Result<Self> {
        // parse as Node<NodeId>
        let node: Node<NodeId> = Node::decode(&g.data.0[..])?;

        let mut hdr_map = HashMap::new();
        // map from name(node id) to hash
        for hdr in g.links.into_iter() {
            let id = NodeId::from_generic(hdr.name)?;
            hdr_map.insert(id, hdr.hash);
        }

        let node_children: Result<Vec<RemoteNodeRef>> = node
            .children
            .into_iter()
            .map(|id| {
                let x: Result<RemoteNodeRef> = hdr_map
                    .remove(&id)
                    .map(|node_ref| RemoteNodeRef(id, node_ref.promote()))
                    .ok_or(Box::new(ParseError(
                        "invalid node-header reference".to_string(),
                    )));
                x
            })
            .collect();
        let node_children: Vec<RemoteNodeRef> = node_children?;

        let node = Node {
            parent: node.parent,
            children: node_children,
            header: node.header,
            body: node.body,
        };

        Ok(node)
    }
}

impl Node<NodeRef> {
    pub fn into_generic(self) -> Result<api::bulk_put::Node> {
        let data = Node {
            parent: self.parent,
            children: self
                .children
                .iter()
                .map(|node_ref| node_ref.node_id())
                .collect(),
            header: self.header,
            body: self.body,
        };
        let data = Node::encode(&data)?;
        let data = encodings::Base64(data);

        let links = self
            .children
            .into_iter()
            .map(|r| match r {
                NodeRef::Modified(id) => {
                    let id = id.into_generic();
                    api::bulk_put::NodeLink::Local(id)
                }
                NodeRef::Unmodified(RemoteNodeRef(id, hash)) => {
                    let name = id.into_generic();
                    let hdr = domain::Header {
                        size: 0, // TODO: FIXME or drop size field. idk.
                        name: name.0,
                        hash: hash.demote(),
                    };
                    api::bulk_put::NodeLink::Remote(hdr)
                }
            })
            .collect();
        let node = api::bulk_put::Node { data, links };
        Ok(node)
    }
}

impl<T> Node<T> {
    pub fn new(parent: Option<NodeId>) -> Self {
        Node {
            parent,
            children: Vec::new(),
            header: "".to_string(),
            body: "".to_string(),
        }
    }
}
