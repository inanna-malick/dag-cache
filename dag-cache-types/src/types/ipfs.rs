use crate::types::encodings::{Base58, Base64};
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct IPFSHeader {
    pub name: String,
    pub hash: IPFSHash,
    pub size: u64,
}

impl IPFSHeader {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::IpfsHeader {
        grpc::IpfsHeader {
            name: self.name,
            hash: Some(self.hash.into_proto()),
            size: self.size,
        }
    }

    #[cfg(feature = "grpc")]
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
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::IpfsHash {
        let base_58 = self.0;
        let raw = base_58.to_string();
        grpc::IpfsHash { hash: raw }
    }

    #[cfg(feature = "grpc")]
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
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::IpfsNode {
        grpc::IpfsNode {
            links: self.links.into_iter().map(IPFSHeader::into_proto).collect(),
            data: self.data.0,
        }
    }

    #[cfg(feature = "grpc")]
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

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeWithHeader {
    pub header: IPFSHeader,
    pub node: DagNode,
}

impl DagNodeWithHeader {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::IpfsNodeWithHeader {
        let hdr = self.header.into_proto();
        let node = self.node.into_proto();

        grpc::IpfsNodeWithHeader {
            header: Some(hdr),
            node: Some(node),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::IpfsNodeWithHeader) -> Result<Self, ProtoDecodingError> {
        let header = p.header.ok_or(ProtoDecodingError {
            cause: "missing header".to_string(),
        })?;
        let header = IPFSHeader::from_proto(header)?;
        let node = p.node.ok_or(ProtoDecodingError {
            cause: "missing node".to_string(),
        })?;
        let node = DagNode::from_proto(node)?;
        Ok(Self { header, node })
    }
}
