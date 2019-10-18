use crate::types::encodings::Base58;
use crate::types::errors::ProtoDecodingError;
use crate::types::grpc;

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct ClientSideHash(Base58);
impl ClientSideHash {
    #[cfg(test)]
    pub fn new(x: Base58) -> ClientSideHash { ClientSideHash(x) }

    pub fn from_proto(p: grpc::ClientSideHash) -> Result<Self, ProtoDecodingError> {
        Base58::from_string(&p.hash)
            .map(ClientSideHash)
            .map_err(|e| ProtoDecodingError {
                cause: format!("invalid base58 string in client side hash: {:?}", e),
            })
    }
}

impl std::fmt::Display for ClientSideHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

pub mod bulk_put {
    use super::{grpc, ClientSideHash, ProtoDecodingError};
    use crate::types::encodings::Base64;
    use crate::types::ipfs;
    use crate::types::validated_tree::ValidatedTree;

    // idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
    // all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
    // with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
    // of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
    // goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
    #[derive(Debug)]
    pub struct Req {
        pub validated_tree: ValidatedTree,
    }

    impl Req {
        pub fn from_proto(p: grpc::BulkPutReq) -> Result<Self, ProtoDecodingError> {
            let root_node = p.root_node.ok_or(ProtoDecodingError {
                cause: "root node not present on Bulk Put Req proto".to_string(),
            })?;
            let root_node = DagNode::from_proto(root_node)?;

            let nodes: Result<Vec<DagNodeWithHash>, ProtoDecodingError> = p
                .nodes
                .into_iter()
                .map(DagNodeWithHash::from_proto)
                .collect();
            let nodes = nodes?;

            let mut node_map = hashbrown::HashMap::with_capacity(nodes.len());

            for DagNodeWithHash { hash, node } in nodes.into_iter() {
                node_map.insert(hash, node);
            }

            let validated_tree =
                ValidatedTree::validate(root_node, node_map).map_err(|e| ProtoDecodingError {
                    cause: format!("invalid tree provided in Bulk Put Req proto, {:?}", e),
                })?;

            Ok(Req { validated_tree })
        }
    }

    #[derive(Clone, Debug)]
    pub struct DagNodeWithHash {
        pub hash: ClientSideHash,
        pub node: DagNode,
    }

    impl DagNodeWithHash {
        pub fn from_proto(p: grpc::BulkPutIpfsNodeWithHash) -> Result<Self, ProtoDecodingError> {
            let hash = p.client_side_hash.ok_or(ProtoDecodingError {
                cause: "client side hash not present on BulkPutIpfsNodeWithHash proto".to_string(),
            })?;

            let hash = ClientSideHash::from_proto(hash)?;

            let node = p.node.ok_or(ProtoDecodingError {
                cause: "node not present on BulkPutIpfsNodeWithHash proto".to_string(),
            })?;
            let node = DagNode::from_proto(node)?;
            Ok(DagNodeWithHash { hash, node })
        }
    }

    #[derive(Clone, Debug)]
    pub struct DagNode {
        pub links: Vec<DagNodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
        pub data: Base64,            // this node's data
    }

    impl DagNode {
        pub fn from_proto(p: grpc::BulkPutIpfsNode) -> Result<Self, ProtoDecodingError> {
            let data = Base64(p.data);

            let links: Result<Vec<DagNodeLink>, ProtoDecodingError> =
                p.links.into_iter().map(DagNodeLink::from_proto).collect();
            let links = links?;
            Ok(DagNode { links, data })
        }
    }

    #[derive(Clone, Debug)]
    pub enum DagNodeLink {
        Local(ClientSideHash),
        Remote(ipfs::IPFSHeader),
    }

    impl DagNodeLink {
        pub fn from_proto(p: grpc::BulkPutLink) -> Result<Self, ProtoDecodingError> {
            match p.link {
                Some(grpc::bulk_put_link::Link::InIpfs(hdr)) => {
                    ipfs::IPFSHeader::from_proto(hdr).map(DagNodeLink::Remote)
                }
                Some(grpc::bulk_put_link::Link::InReq(csh)) => {
                    let csh = ClientSideHash::from_proto(csh)?;
                    Ok(DagNodeLink::Local(csh))
                }
                None => Err(ProtoDecodingError {
                    cause: "no value for bulk put link oneof".to_string(),
                }),
            }
            // let hash = ClientSideHash::from_proto(p.hash)?;
            // let node = DagNode::from_proto(p.node)?;
            // Ok(DagNodeWithHash{hash, node})
        }
    }
}

pub mod get {
    use super::grpc;
    use crate::types::ipfs;

    // ~= NonEmptyList (head, rest struct)
    #[derive(Clone, Debug)]
    pub struct Resp {
        pub requested_node: ipfs::DagNode,
        pub extra_node_count: u64,
        pub extra_nodes: Vec<ipfs::DagNodeWithHeader>,
    }

    impl Resp {
        pub fn into_proto(self) -> grpc::GetResp {
            grpc::GetResp {
                requested_node: Some(self.requested_node.into_proto()),
                extra_node_count: self.extra_node_count,
                extra_nodes: self
                    .extra_nodes
                    .into_iter()
                    .map(|x| x.into_proto())
                    .collect(),
            }
        }
    }
}
