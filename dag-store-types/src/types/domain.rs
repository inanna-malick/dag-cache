use crate::types::encodings::{Base58, Base64};
#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};
use blake2::{Blake2b, Digest};

#[derive(PartialEq, Hash, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct ClientId(pub String); // string? u128? idk

impl ClientId {
    pub fn new(x: String) -> ClientId { ClientId(x) }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::ClientId) -> Result<Self, ProtoDecodingError> {
        Ok(ClientId(p.hash)) // TODO: validation?
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::ClientId { grpc::ClientId { hash: self.0 } }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub hash: Hash,
    pub size: u64,
}

impl Header {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Header {
        grpc::Header {
            name: self.name,
            hash: Some(self.hash.into_proto()),
            size: self.size,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Header) -> Result<Self, ProtoDecodingError> {
        let hash = p.hash.ok_or(ProtoDecodingError(
            "hash field not present on Header proto".to_string(),
        ))?;
        let hash = Hash::from_proto(hash)?;
        let hdr = Header {
            name: p.name,
            size: p.size,
            hash,
        };
        Ok(hdr)
    }
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Hash(pub Base58);

impl Hash {
    pub fn to_string_canonical(&self) -> String {
        format!("{}.blake2", self)
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Hash {
        let base_58 = self.0;
        let raw = base_58.to_string();
        grpc::Hash { hash: raw }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Hash) -> Result<Self, ProtoDecodingError> {
        Base58::from_string(&p.hash)
            .map(Hash)
            .map_err(|e| ProtoDecodingError(format!("invalid base58 string in hash: {:?}", e)))
    }

    pub fn from_string(x: &str) -> Result<Self, base58::FromBase58Error> {
        Base58::from_string(x).map(Self::from_raw)
    }

    pub fn from_raw(raw: Base58) -> Hash { Hash(raw) }

    pub fn promote<T>(self) -> TypedHash<T> { TypedHash(self, std::marker::PhantomData) }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

// phantom type param used to distinguish between hashes of different types
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct TypedHash<T>(Hash, std::marker::PhantomData<T>);

impl<T> std::hash::Hash for TypedHash<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.0.hash(state); }
}

impl<T> TypedHash<T> {
    pub fn demote(self) -> Hash { self.0 }
}

impl<T> core::ops::Deref for TypedHash<T> {
    type Target = Hash;

    fn deref(&self) -> &Self::Target { &self.0 }
}

impl<'de, T> Deserialize<'de> for TypedHash<T> {
    fn deserialize<D>(deserializer: D) -> Result<TypedHash<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let res = Deserialize::deserialize(deserializer)?;
        Ok(TypedHash(res, std::marker::PhantomData))
    }
}

impl<T> Serialize for TypedHash<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Serialize::serialize(&self.0, serializer)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Deserialize, Serialize)]
pub struct Node {
    pub links: Vec<Header>,
    pub data: Base64,
}

impl Node {
    pub fn canonical_hash(&self) -> Hash {
        let mut hasher = Blake2b::new();
        for link in self.links.iter() {
            hasher.input(&link.name);
            hasher.input(&(link.hash.0).0);
            hasher.input(link.size.to_be_bytes());
        }
        hasher.input(&self.data.0);
        let hash = hasher.result();
        Hash::from_raw(Base58::from_bytes(hash.to_vec()))
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Node {
        grpc::Node {
            links: self.links.into_iter().map(Header::into_proto).collect(),
            data: self.data.0,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Node) -> Result<Self, ProtoDecodingError> {
        let links: Result<Vec<Header>, ProtoDecodingError> =
            p.links.into_iter().map(Header::from_proto).collect();
        let links = links?;
        let node = Node {
            data: Base64(p.data),
            links,
        };
        Ok(node)
    }
}

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeWithHeader {
    pub header: Header,
    pub node: Node,
}

impl NodeWithHeader {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::NodeWithHeader {
        let hdr = self.header.into_proto();
        let node = self.node.into_proto();

        grpc::NodeWithHeader {
            header: Some(hdr),
            node: Some(node),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::NodeWithHeader) -> Result<Self, ProtoDecodingError> {
        let header = p
            .header
            .ok_or(ProtoDecodingError("missing header".to_string()))?;
        let header = Header::from_proto(header)?;
        let node = p
            .node
            .ok_or(ProtoDecodingError("missing node".to_string()))?;
        let node = Node::from_proto(node)?;
        Ok(Self { header, node })
    }
}
