use generic_array::GenericArray;
use crate::types::encodings::{Base58, Base64};
#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};
use std::str::FromStr;


#[derive(PartialEq, Hash, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Id(pub u32);

impl Id {
    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Id) -> Result<Self, ProtoDecodingError> {
        Ok(Id(p.id_data))
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Id {
        grpc::Id {
            id_data: self.0,
        }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(PartialEq, Eq, Copy, Clone, Debug, Serialize, Deserialize)]
pub struct Header {
    pub id: Id,
    pub hash: Hash,
}

impl Header {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Header {
        grpc::Header {
            header_id: Some(self.id.into_proto()),
            header_hash: Some(self.hash.into_proto()),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Header) -> Result<Self, ProtoDecodingError> {
        let hash = p.header_hash.ok_or(ProtoDecodingError(
            "hash field not present on Header proto".to_string(),
        ))?;
        let hash = Hash::from_proto(hash)?;

        let id = p.header_id.ok_or(ProtoDecodingError(
            "id field not present on Header proto".to_string(),
        ))?;
        let id = Id::from_proto(id)?;

        let hdr = Header {
            hash,
            id,
        };
        Ok(hdr)
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct Hash(pub GenericArray<u8, <blake2::Blake2s as blake2::Digest>::OutputSize>);


#[test]
fn assert_256_digest_size() {
    let n = Node{
        links: Vec::new(),
        data: Base64(Vec::new()),
    };
    let h = n.canonical_hash();
    assert_eq!(h.0.as_slice().len() * 8 /* u8's */, 256);
}

// TODO: impl this
/// TODO: skip writes, etc for null hash - or mb corresponding null node?
/// Magic null hash for empty values (eg null commit)
// pub const NULL_HASH: Hash = ...?

impl Hash {
    pub fn to_string_canonical(&self) -> String {
        format!("{}.blake2", self)
    }

    pub fn from_base58(b58: &str) -> Result<Self, Base58HashDecodeError> {
        let bytes = Base58::from_string(b58)
            .map_err(|e| Base58HashDecodeError(format!("invalid b58: {:?}", e)))?;
        Self::from_bytes(&bytes.0).ok_or(Base58HashDecodeError("invalid length".to_string()))
    }

    pub fn to_base58(&self) -> String {
        let b58 = Base58::from_bytes(self.0.as_slice().to_vec());
        format!("{}", b58)
    }

    pub fn from_bytes(x: &[u8]) -> Option<Self> {
        GenericArray::from_exact_iter(x.into_iter().map(|x| *x)).map(Self)
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Hash {
        grpc::Hash {
            hash_data: self.0.as_slice().to_vec(),
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Hash) -> Result<Self, ProtoDecodingError> {
        Self::from_bytes(&p.hash_data).ok_or(ProtoDecodingError("bad hash length".to_string()))
    }

    pub fn promote<T>(self) -> TypedHash<T> {
        TypedHash(self, std::marker::PhantomData)
    }
}

impl FromStr for Hash {
    type Err = Base58HashDecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base58(s)
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_base58().fmt(f)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Hash, D::Error>
    where
        D: Deserializer<'de>,
    {
        let res = Deserialize::deserialize(deserializer)?;
        Hash::from_bytes(res).ok_or(serde::de::Error::custom("foo"))
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Serialize::serialize(self.0.as_slice(), serializer)
    }
}

// phantom type param used to distinguish between hashes of different types
#[derive(PartialEq, Eq, Debug)]
pub struct TypedHash<T>(Hash, std::marker::PhantomData<T>);

impl<T> FromStr for TypedHash<T> {
    type Err = Base58HashDecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let h = Hash::from_base58(s)?;
        Ok(h.promote())
    }
}

impl<T> std::fmt::Display for TypedHash<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.demote().fmt(f)
    }
}

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

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Hash {
        self.demote().into_proto()
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Hash) -> Result<Self, ProtoDecodingError> {
        let h = Hash::from_proto(p)?;
        Ok(h.promote())
    }
}

impl<T> core::ops::Deref for TypedHash<T> {
    type Target = Hash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
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
    /// stable hashing function (not using proto because there's no canonical encoding)
    pub fn canonical_hash(&self) -> Hash {
        use blake2::Digest;
        let mut hasher = blake2::Blake2s::new();
        for link in self.links.iter() {
            hasher.update(&link.id.0.to_be_bytes());
            hasher.update(link.hash.0.as_slice());
        }
        hasher.update(&self.data.0);
        let hash = hasher.finalize();
        Hash(hash)
    }

    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Node {
        grpc::Node {
            node_links: self.links.into_iter().map(Header::into_proto).collect(),
            node_data: self.data.0,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_proto(p: grpc::Node) -> Result<Self, ProtoDecodingError> {
        let links: Result<Vec<Header>, ProtoDecodingError> =
            p.node_links.into_iter().map(Header::from_proto).collect();
        let links = links?;
        let node = Node {
            data: Base64(p.node_data),
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

// FIXME: figure out better error hierarchy

#[derive(Debug)]
pub struct Base58HashDecodeError(String);

impl std::fmt::Display for Base58HashDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self) // TODO: more idiomatic way of doing this
    }
}

impl std::error::Error for Base58HashDecodeError {
    fn description(&self) -> &str {
        &self.0
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        None
    }
}
