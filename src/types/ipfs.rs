use crate::types::encodings::{Base58, Base64};
use crate::types::errors::ProtoDecodingError;
use crate::types::grpc;
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct IPFSHeader {
    pub name: String,
    pub hash: IPFSHash,
    pub size: u64,
}

impl IPFSHeader {
    pub fn into_proto(self) -> grpc::IpfsHeader {
        grpc::IpfsHeader {
            name: self.name,
            hash: Some(self.hash.into_proto()),
            size: self.size,
        }
    }

    pub fn from_proto(p: grpc::IpfsHeader) -> Result<Self, ProtoDecodingError> {
        let hash = p.hash.ok_or(ProtoDecodingError {
            cause: "hash field not present on IpfsHeader proto".to_string(),
        })?;
        let hash = IPFSHash::from_proto(hash)?;
        let hdr = IPFSHeader {
            name: p.name,
            size: p.size,
            hash,
        };
        Ok(hdr)
    }
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct IPFSHash(Base58);

impl IPFSHash {
    pub fn into_proto(self) -> grpc::IpfsHash {
        let base_58 = self.0;
        let raw = base_58.to_string();
        grpc::IpfsHash { hash: raw }
    }

    pub fn from_proto(p: grpc::IpfsHash) -> Result<Self, ProtoDecodingError> {
        Base58::from_string(&p.hash)
            .map(IPFSHash)
            .map_err(|e| ProtoDecodingError {
                cause: format!("invalid base58 string in ipfs hash: {:?}", e),
            })
    }

    #[cfg(test)]
    pub fn from_string(x: &str) -> Result<Self, base58::FromBase58Error> {
        Base58::from_string(x).map(Self::from_raw)
    }

    #[cfg(test)]
    pub fn from_raw(raw: Base58) -> IPFSHash { IPFSHash(raw) }
}

impl std::fmt::Display for IPFSHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

#[derive(PartialEq, Eq, Clone, Debug, Deserialize, Serialize)]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: Base64,
}

impl DagNode {
    pub fn into_proto(self) -> grpc::IpfsNode {
        grpc::IpfsNode {
            links: self.links.into_iter().map(IPFSHeader::into_proto).collect(),
            data: self.data.0,
        }
    }

    pub fn from_proto(p: grpc::IpfsNode) -> Result<Self, ProtoDecodingError> {
        let links: Result<Vec<IPFSHeader>, ProtoDecodingError> =
            p.links.into_iter().map(IPFSHeader::from_proto).collect();
        let links = links?;
        let node = DagNode {
            data: Base64(p.data),
            links,
        };
        Ok(node)
    }
}

impl DagNodeWithHeader {
    pub fn into_proto(self) -> grpc::IpfsNodeWithHeader {
        let hdr = self.header.into_proto();
        let node = self.node.into_proto();

        grpc::IpfsNodeWithHeader {
            header: Some(hdr),
            node: Some(node),
        }
    }
}

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeWithHeader {
    pub header: IPFSHeader,
    pub node: DagNode,
}
