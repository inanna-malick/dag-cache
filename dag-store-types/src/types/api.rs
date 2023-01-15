use crate::types::domain::{Header, Id};
#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
#[cfg(feature = "grpc")]
use std::collections::HashMap;

pub mod bulk_put {
    use super::*;
    use crate::types::validated_tree::ValidatedTree;

    // TODO: revisit docs now that not using ipfs
    // idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
    // all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
    // with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
    // of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
    // goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
    #[derive(Debug)]
    pub struct Req {
        pub validated_tree: ValidatedTree,
    }

    #[cfg(feature = "grpc")]
    impl Req {
        pub fn into_proto(self) -> grpc::BulkPutReq {
            let root_node = self.validated_tree.root_node.into_proto();

            let nodes = self
                .validated_tree
                .nodes
                .into_iter()
                .map(|(id, n)| grpc::BulkPutNodeWithId {
                    node: Some(n.into_proto()),
                    id: Some(id.into_proto()),
                })
                .collect();

            grpc::BulkPutReq {
                root_node: Some(root_node),
                nodes,
            }
        }

        pub fn from_proto(p: grpc::BulkPutReq) -> Result<Self, ProtoDecodingError> {
            let root_node = p.root_node.ok_or(ProtoDecodingError(
                "root node not present on Bulk Put Req proto".to_string(),
            ))?;
            let root_node = Node::from_proto(root_node)?;

            let nodes: Result<Vec<NodeWithId>, ProtoDecodingError> =
                p.nodes.into_iter().map(NodeWithId::from_proto).collect();
            let nodes = nodes?;

            let mut node_map = HashMap::with_capacity(nodes.len());

            for NodeWithId { id, node } in nodes.into_iter() {
                node_map.insert(id, node);
            }

            let validated_tree = ValidatedTree::validate(root_node, node_map).map_err(|e| {
                ProtoDecodingError(format!(
                    "invalid tree provided in Bulk Put Req proto, {:?}",
                    e
                ))
            })?;

            Ok(Req { validated_tree })
        }
    }

    #[derive(Clone, Debug)]
    pub struct NodeWithId {
        pub id: Id,
        pub node: Node,
    }

    impl NodeWithId {
        #[cfg(feature = "grpc")]
        pub fn from_proto(p: grpc::BulkPutNodeWithId) -> Result<Self, ProtoDecodingError> {
            let id = p.id.ok_or(ProtoDecodingError(
                "client side hash not present on BulkPutNodeWithHash proto".to_string(),
            ))?;
            let id = Id::from_proto(id)?;

            let node = p.node.ok_or(ProtoDecodingError(
                "node not present on BulkPutNodeWithHash proto".to_string(),
            ))?;
            let node = Node::from_proto(node)?;
            Ok(NodeWithId { id, node })
        }

        #[cfg(feature = "grpc")]
        pub fn into_proto(self) -> grpc::BulkPutNodeWithId {
            grpc::BulkPutNodeWithId {
                id: Some(self.id.into_proto()),
                node: Some(self.node.into_proto()),
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct Node {
        pub links: Vec<NodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
        pub data: Vec<u8>,        // this node's data
    }

    #[cfg(feature = "grpc")]
    impl Node {
        pub fn from_proto(p: grpc::BulkPutNode) -> Result<Self, ProtoDecodingError> {
            let links: Result<Vec<NodeLink>, ProtoDecodingError> =
                p.links.into_iter().map(NodeLink::from_proto).collect();
            let links = links?;
            Ok(Node {
                links,
                data: p.data,
            })
        }

        pub fn into_proto(self) -> grpc::BulkPutNode {
            grpc::BulkPutNode {
                data: self.data,
                links: self.links.into_iter().map(|x| x.into_proto()).collect(),
            }
        }
    }

    #[derive(Clone, Debug)]
    pub enum NodeLink {
        Local(Id),
        Remote(Header),
    }

    #[cfg(feature = "grpc")]
    impl NodeLink {
        pub fn into_proto(self) -> grpc::BulkPutLink {
            let link = match self {
                NodeLink::Local(id) => grpc::bulk_put_link::Link::InReq(id.into_proto()),

                NodeLink::Remote(hdr) => grpc::bulk_put_link::Link::InStore(hdr.into_proto()),
            };
            grpc::BulkPutLink { link: Some(link) }
        }

        pub fn from_proto(p: grpc::BulkPutLink) -> Result<Self, ProtoDecodingError> {
            match p.link {
                Some(grpc::bulk_put_link::Link::InStore(hdr)) => {
                    Header::from_proto(hdr).map(NodeLink::Remote)
                }
                Some(grpc::bulk_put_link::Link::InReq(csh)) => {
                    let csh = Id::from_proto(csh)?;
                    Ok(NodeLink::Local(csh))
                }
                None => Err(ProtoDecodingError(
                    "no value for bulk put link oneof".to_string(),
                )),
            }
        }
    }
}
