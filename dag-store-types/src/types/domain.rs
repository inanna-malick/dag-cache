#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use serde::{Deserialize, Serialize};
use slice_as_array::slice_to_array_clone;

#[derive(PartialEq, Hash, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Id(pub u32);

impl Id {
    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Id) -> Result<Self, ProtoDecodingError> {
        Ok(Self(p.id))
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Id {
        grpc::Id { id: self.0 }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct Header {
    pub id: Id,
    pub hash: Hash,
    pub metadata: String,
}

impl Header {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Header {
        grpc::Header {
            id: Some(self.id.into_proto()),
            hash: Some(self.hash.into_proto()),
            metadata: self.metadata,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Header) -> Result<Self, ProtoDecodingError> {
        let hash = p.hash.ok_or(ProtoDecodingError(
            "hash field not present on Header proto".to_string(),
        ))?;
        let hash = Hash::from_proto(hash)?;

        let id = p.id.ok_or(ProtoDecodingError(
            "id field not present on Header proto".to_string(),
        ))?;
        let id = Id::from_proto(id)?;

        let hdr = Header {
            hash,
            id,
            metadata: p.metadata,
        };
        Ok(hdr)
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct Hash(pub blake3::Hash);

impl Hash {
    pub fn to_string_canonical(&self) -> String {
        format!("{}.blake3", self)
    }

    pub fn to_base58(&self) -> String {
        let b58 = base58::ToBase58::to_base58(&self.0.as_bytes().to_vec()[..]);
        format!("{}", b58)
    }

    pub fn from_bytes(x: &[u8]) -> Option<Self> {
        slice_to_array_clone!(x, [u8; 32])
            .map(blake3::Hash::from)
            .map(Self)
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Hash {
        grpc::Hash {
            hash: self.0.as_bytes().to_vec(),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Hash) -> Result<Self, ProtoDecodingError> {
        Self::from_bytes(&p.hash).ok_or(ProtoDecodingError("bad hash length".to_string()))
    }

    pub fn promote<T>(self) -> TypedHash<T> {
        TypedHash(self, std::marker::PhantomData)
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_base58().fmt(f)
    }
}

// phantom type param used to distinguish between hashes of different types
#[derive(PartialEq, Eq, Debug)]
pub struct TypedHash<T>(Hash, std::marker::PhantomData<T>);

// if derived will place unneccessary Clone bound on T
impl<T> Clone for TypedHash<T> {
    fn clone(&self) -> Self {
        Self(self.0, std::marker::PhantomData)
    }
}

// if derived will place unneccessary Copy bound on T
impl<T> Copy for TypedHash<T> {}

impl<T> std::hash::Hash for TypedHash<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> TypedHash<T> {
    pub fn demote(self) -> Hash {
        self.0
    }
}

impl<T> core::ops::Deref for TypedHash<T> {
    type Target = Hash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Node {
    pub headers: Vec<Header>,
    pub data: Vec<u8>,
}

impl Node {
    /// stable hashing function (not using proto because there's no canonical encoding)
    pub fn canonical_hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        for link in self.headers.iter() {
            hasher.update(&link.id.0.to_be_bytes());
            hasher.update(link.hash.0.as_bytes());
        }
        hasher.update(&self.data);
        let hash = hasher.finalize();
        Hash(hash)
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Node {
        grpc::Node {
            links: self.headers.into_iter().map(Header::into_proto).collect(),
            data: self.data,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Node) -> Result<Self, ProtoDecodingError> {
        let links: Result<Vec<Header>, ProtoDecodingError> =
            p.links.into_iter().map(Header::from_proto).collect();
        let links = links?;
        let node = Node {
            data: p.data,
            headers: links,
        };
        Ok(node)
    }
}

#[derive(Clone, Debug)]
pub struct NodeWithHash {
    pub hash: Hash,
    pub node: Node,
}

impl NodeWithHash {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::NodeWithHash {
        let hash = self.hash.into_proto();
        let node = self.node.into_proto();

        grpc::NodeWithHash {
            hash: Some(hash),
            node: Some(node),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::NodeWithHash) -> Result<Self, ProtoDecodingError> {
        let hash = p
            .hash
            .ok_or(ProtoDecodingError("missing hash".to_string()))?;
        let hash = Hash::from_proto(hash)?;
        let node = p
            .node
            .ok_or(ProtoDecodingError("missing node".to_string()))?;
        let node = Node::from_proto(node)?;
        Ok(Self { hash, node })
    }
}

#[derive(Clone, Debug)]
pub enum GetNodesResp {
    Node(NodeWithHash),
    ChoseNotToExplore(Header),
}

impl GetNodesResp {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::GetNodesResp {
        match self {
            GetNodesResp::Node(n) => grpc::GetNodesResp {
                link: Some(grpc::get_nodes_resp::Link::NodeResponse(n.into_proto())),
            },
            GetNodesResp::ChoseNotToExplore(hdr) => grpc::GetNodesResp {
                link: Some(grpc::get_nodes_resp::Link::ChoseNotToTraverse(
                    hdr.into_proto(),
                )),
            },
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::GetNodesResp) -> Result<Self, ProtoDecodingError> {
        match p
            .link
            .ok_or(ProtoDecodingError("proto missing enum".to_string()))?
        {
            grpc::get_nodes_resp::Link::ChoseNotToTraverse(hdr) => {
                Ok(Self::ChoseNotToExplore(Header::from_proto(hdr)?))
            }
            grpc::get_nodes_resp::Link::NodeResponse(n) => {
                Ok(Self::Node(NodeWithHash::from_proto(n)?))
            }
        }
    }
}
