use crate::types::domain::{Hash, Header, Id, Node, NodeWithHeader};
#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use serde::{Deserialize, Serialize};
#[cfg(feature = "grpc")]
use std::collections::HashMap;

pub mod bulk_put {
    use super::*;
    use crate::types::encodings::Base64;
    use crate::types::validated_tree::ValidatedTree;

    #[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
    pub struct Resp {
        pub root_hash: Hash,
        pub additional_uploaded: Vec<(Id, Hash)>,
    }

    #[cfg(feature = "grpc")]
    impl Resp {
        pub fn into_proto(self) -> grpc::BulkPutResp {
            grpc::BulkPutResp {
                root_hash: Some(self.root_hash.into_proto()),
                additional_uploaded: self
                    .additional_uploaded
                    .into_iter()
                    .map(|x| grpc::BulkPutRespPair {
                        client_id: Some(x.0.into_proto()),
                        hash: Some(x.1.into_proto()),
                    })
                    .collect(),
            }
        }

        pub fn from_proto(p: grpc::BulkPutResp) -> Result<Self, ProtoDecodingError> {
            let root_hash = p.root_hash.ok_or(ProtoDecodingError(
                "root hash not present on Bulk Put Resp proto".to_string(),
            ))?;
            let root_hash = Hash::from_proto(root_hash)?;

            let additional_uploaded: Result<Vec<(Id, Hash)>, ProtoDecodingError> = p
                .additional_uploaded
                .into_iter()
                .map(|bp| {
                    let client_id = bp.client_id.ok_or(ProtoDecodingError(
                        "client_id not present on Bulk Put Resp proto pair".to_string(),
                    ))?;
                    let client_id = Id::from_proto(client_id)?;

                    let hash = bp.hash.ok_or(ProtoDecodingError(
                        "hash not present on Bulk Put Resp proto pair".to_string(),
                    ))?;
                    let hash = Hash::from_proto(hash)?;
                    Ok((client_id, hash))
                })
                .collect();
            let additional_uploaded = additional_uploaded?;

            Ok(Resp {
                root_hash,
                additional_uploaded,
            })
        }
    }

    #[derive(Debug)]
    pub struct CAS {
        /// previous hash required for operation to succeed - optional, to allow for first set operation
        pub required_previous_hash: Option<Hash>,
        pub cas_key: String,
    }

    #[cfg(feature = "grpc")]
    impl CAS {
        pub fn into_proto(self) -> grpc::CheckAndSet {
            grpc::CheckAndSet {
                required_previous_hash: self.required_previous_hash.map(|x| x.into_proto()),
                cas_key: self.cas_key,
            }
        }

        pub fn from_proto(p: grpc::CheckAndSet) -> Result<Self, ProtoDecodingError> {
            let required_previous_hash =
                p.required_previous_hash.map(Hash::from_proto).transpose()?;

            Ok(Self {
                required_previous_hash,
                cas_key: p.cas_key,
            })
        }
    }

    // TODO: revisit docs now that not using ipfs
    // idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
    // all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
    // with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
    // of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
    // goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
    #[derive(Debug)]
    pub struct Req {
        pub validated_tree: ValidatedTree,
        pub cas: Option<CAS>,
    }

    #[cfg(feature = "grpc")]
    impl Req {
        pub fn into_proto(self) -> grpc::BulkPutReq {
            let root_node = self.validated_tree.root_node.into_proto();

            let cas = self.cas.map(|x| x.into_proto());

            let nodes = self
                .validated_tree
                .nodes
                .into_iter()
                .map(|(id, n)| grpc::BulkPutNodeWithHash {
                    node: Some(n.into_proto()),
                    client_side_hash: Some(id.into_proto()),
                })
                .collect();

            grpc::BulkPutReq {
                root_node: Some(root_node),
                nodes,
                cas,
            }
        }

        pub fn from_proto(p: grpc::BulkPutReq) -> Result<Self, ProtoDecodingError> {
            let cas = match p.cas {
                Some(cas) => {
                    let cas = CAS::from_proto(cas)?;
                    Some(cas)
                }
                None => None,
            };

            let root_node = p.root_node.ok_or(ProtoDecodingError(
                "root node not present on Bulk Put Req proto".to_string(),
            ))?;
            let root_node = Node::from_proto(root_node)?;

            let nodes: Result<Vec<NodeWithHash>, ProtoDecodingError> =
                p.nodes.into_iter().map(NodeWithHash::from_proto).collect();
            let nodes = nodes?;

            let mut node_map = HashMap::with_capacity(nodes.len());

            for NodeWithHash { hash, node } in nodes.into_iter() {
                node_map.insert(hash, node);
            }

            let validated_tree = ValidatedTree::validate(root_node, node_map).map_err(|e| {
                ProtoDecodingError(format!(
                    "invalid tree provided in Bulk Put Req proto, {:?}",
                    e
                ))
            })?;

            Ok(Req {
                validated_tree,
                cas,
            })
        }
    }

    #[derive(Clone, Debug)]
    pub struct NodeWithHash {
        pub hash: Id,
        pub node: Node,
    }

    impl NodeWithHash {
        #[cfg(feature = "grpc")]
        pub fn from_proto(p: grpc::BulkPutNodeWithHash) -> Result<Self, ProtoDecodingError> {
            let hash = p.client_side_hash.ok_or(ProtoDecodingError(
                "client side hash not present on BulkPutNodeWithHash proto".to_string(),
            ))?;
            let hash = Id::from_proto(hash)?;

            let node = p.node.ok_or(ProtoDecodingError(
                "node not present on BulkPutNodeWithHash proto".to_string(),
            ))?;
            let node = Node::from_proto(node)?;
            Ok(NodeWithHash { hash, node })
        }
    }

    #[derive(Clone, Debug)]
    pub struct Node {
        pub links: Vec<NodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
        pub data: Base64,         // this node's data
    }

    #[cfg(feature = "grpc")]
    impl Node {
        pub fn from_proto(p: grpc::BulkPutNode) -> Result<Self, ProtoDecodingError> {
            let data = Base64(p.data);

            let links: Result<Vec<NodeLink>, ProtoDecodingError> =
                p.links.into_iter().map(NodeLink::from_proto).collect();
            let links = links?;
            Ok(Node { links, data })
        }

        pub fn into_proto(self) -> grpc::BulkPutNode {
            grpc::BulkPutNode {
                data: self.data.0,
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

pub mod get {
    use super::*;

    // ~= NonEmptyList (head, rest struct)
    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Resp {
        pub requested_node: Node,
        pub extra_nodes: Vec<NodeWithHeader>,
    }

    impl Resp {
        #[cfg(feature = "grpc")]
        pub fn from_proto(p: grpc::GetResp) -> Result<Self, ProtoDecodingError> {
            let extra_nodes: Result<Vec<NodeWithHeader>, ProtoDecodingError> = p
                .extra_nodes
                .into_iter()
                .map(|n| NodeWithHeader::from_proto(n))
                .collect();
            let extra_nodes = extra_nodes?;

            let requested_node = p
                .requested_node
                .ok_or(ProtoDecodingError("missing requested_node".to_string()))?;
            let requested_node = Node::from_proto(requested_node)?;

            let res = Self {
                requested_node,
                extra_nodes,
            };
            Ok(res)
        }

        #[cfg(feature = "grpc")]
        pub fn into_proto(self) -> grpc::GetResp {
            grpc::GetResp {
                requested_node: Some(self.requested_node.into_proto()),
                extra_nodes: self
                    .extra_nodes
                    .into_iter()
                    .map(|x| x.into_proto())
                    .collect(),
            }
        }
    }
}
