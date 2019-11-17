// #![deny(warnings)]
use dag_cache_types::types::ipfs::IPFSHash;
use dag_cache_types::types::{api, encodings, ipfs, validated_tree::ValidatedTree};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync + 'static>>;

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
pub struct RemoteNodeRef(pub NodeId, pub IPFSHash);

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

// cannonical format
impl Node<NodeId> {
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
    pub fn from_generic(g: ipfs::DagNode) -> Result<Self> {
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
                    .map(|node_ref| RemoteNodeRef(id, node_ref))
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
    pub fn into_generic(self) -> Result<api::bulk_put::DagNode> {
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
                    api::bulk_put::DagNodeLink::Local(id)
                }
                NodeRef::Unmodified(RemoteNodeRef(id, hash)) => {
                    let name = id.into_generic();
                    let hdr = ipfs::IPFSHeader {
                        size: 0, // TODO: FIXME
                        name: name.0,
                        hash,
                    };
                    api::bulk_put::DagNodeLink::Remote(hdr)
                }
            })
            .collect();
        let node = api::bulk_put::DagNode { data, links };
        Ok(node)
    }
}

impl<T> Node<T> {
    // TODO: remove arbitrary defaults. very janky.
    pub fn new(parent: Option<NodeId>) -> Self {
        Node {
            parent,
            children: Vec::new(),
            header: "new node header!".to_string(),
            body: "new node body!".to_string(),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Serialize, Debug, Deserialize)]
pub struct GetResp {
    pub requested_node: Node<RemoteNodeRef>,
    pub extra_nodes: HashMap<RemoteNodeRef, Node<RemoteNodeRef>>,
}

impl GetResp {
    pub fn from_generic(g: api::get::Resp) -> Result<Self> {
        let requested_node = Node::from_generic(g.requested_node)?;
        let mut extra_nodes = HashMap::new();
        for x in g.extra_nodes.into_iter() {
            let id = NodeId::from_generic(x.header.name)?;
            let node = Node::from_generic(x.node)?;
            extra_nodes.insert(RemoteNodeRef(id, x.header.hash), node);
        }

        let resp = GetResp {
            requested_node,
            extra_nodes,
        };
        Ok(resp)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct PutReq {
    pub head_node: Node<NodeRef>,
    pub extra_nodes: HashMap<NodeId, Node<NodeRef>>,
}

impl PutReq {
    pub fn into_generic(self) -> Result<api::bulk_put::Req> {
        let head = self.head_node.into_generic()?;
        let mut extra_nodes = HashMap::new();
        for (id, node) in self.extra_nodes.into_iter() {
            let node = node.into_generic()?;
            extra_nodes.insert(id.into_generic(), node);
        }

        let validated_tree = ValidatedTree::validate(head, extra_nodes)?;

        let req = api::bulk_put::Req { validated_tree };
        Ok(req)
    }
}

#[derive(Debug)]
pub struct ParseError(String);

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
