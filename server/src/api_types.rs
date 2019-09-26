use crate::error_types::ProtoDecodingError;
use crate::ipfs_types;
use crate::server::ipfscache as proto;
use serde::{Deserialize, Serialize};

use crate::encoding_types::Base58;

#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct ClientSideHash(Base58);
impl ClientSideHash {
    #[cfg(test)]
    pub fn new(x: Base58) -> ClientSideHash {
        ClientSideHash(x)
    }

    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }

    pub fn from_proto(p: proto::ClientSideHash) -> Self {
        ClientSideHash(Base58(p.hash))
    }
}

pub mod bulk_put {
    use super::{proto, ClientSideHash, ProtoDecodingError};
    use crate::encoding_types::Base64;
    use crate::ipfs_types;
    use serde::{Deserialize, Serialize};

    // idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
    // all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
    // with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
    // of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
    // goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
    // (note: needs to be, like, {body, either (blake2hash, ipfshash)} )
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Req {
        pub root_node: DagNode,
        pub nodes: Vec<DagNodeWithHash>,
    }

    impl Req {
        pub fn from_proto(p: proto::BulkPutReq) -> Result<Self, ProtoDecodingError> {
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

            let req = Req { root_node, nodes };
            Ok(req)
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct DagNodeWithHash {
        pub hash: ClientSideHash,
        pub node: DagNode,
    }

    impl DagNodeWithHash {
        pub fn from_proto(p: proto::BulkPutIpfsNodeWithHash) -> Result<Self, ProtoDecodingError> {
            let hash = p.client_side_hash.ok_or(ProtoDecodingError {
                cause: "client side hash not present on BulkPutIpfsNodeWithHash proto".to_string(),
            })?;

            let hash = ClientSideHash::from_proto(hash);

            let node = p.node.ok_or(ProtoDecodingError {
                cause: "node not present on BulkPutIpfsNodeWithHash proto".to_string(),
            })?;
            let node = DagNode::from_proto(node)?;
            Ok(DagNodeWithHash { hash, node })
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct DagNode {
        pub links: Vec<DagNodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
        pub data: Base64,            // this node's data
    }

    impl DagNode {
        pub fn from_proto(p: proto::BulkPutIpfsNode) -> Result<Self, ProtoDecodingError> {
            let data = Base64(p.data);

            let links: Result<Vec<DagNodeLink>, ProtoDecodingError> =
                p.links.into_iter().map(DagNodeLink::from_proto).collect();
            let links = links?;
            Ok(DagNode { links, data })
        }
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub enum DagNodeLink {
        Local(ClientSideHash),
        Remote(ipfs_types::IPFSHeader),
    }

    impl DagNodeLink {
        pub fn from_proto(p: proto::BulkPutLink) -> Result<Self, ProtoDecodingError> {
            match p.link {
                Some(proto::bulk_put_link::Link::InIpfs(hdr)) => {
                    ipfs_types::IPFSHeader::from_proto(hdr).map(DagNodeLink::Remote)
                }
                Some(proto::bulk_put_link::Link::InReq(csh)) => {
                    let csh = ClientSideHash::from_proto(csh);
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
    use super::proto;
    use crate::ipfs_types;
    use serde::{Deserialize, Serialize};

    // ~= NonEmptyList (head, rest struct)
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Resp {
        pub requested_node: ipfs_types::DagNode,
        pub extra_node_count: u64,
        pub extra_nodes: Vec<ipfs_types::DagNodeWithHeader>,
    }

    impl Resp {
        pub fn into_proto(self) -> proto::GetResp {
            proto::GetResp {
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
