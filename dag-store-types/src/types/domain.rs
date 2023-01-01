#[cfg(feature = "grpc")]
use crate::types::errors::ProtoDecodingError;
#[cfg(feature = "grpc")]
use crate::types::grpc;
use slice_as_array::slice_to_array_clone;

#[derive(PartialEq, Hash, Eq, Clone, Copy, Debug)]
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

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub struct Header {
    pub id: Id,
    pub hash: Hash,
} // TODO: remove size

impl Header {
    #[cfg(feature = "grpc")]
    pub fn into_proto(self) -> grpc::Header {
        grpc::Header {
            id: Some(self.id.into_proto()),
            hash: Some(self.hash.into_proto()),
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

        let hdr = Header { hash, id };
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
    pub links: Vec<Header>,
    pub data: Vec<u8>,
}

impl Node {
    /// stable hashing function (not using proto because there's no canonical encoding)
    pub fn canonical_hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        for link in self.links.iter() {
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
            links: self.links.into_iter().map(Header::into_proto).collect(),
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
            links,
        };
        Ok(node)
    }
}

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug)]
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
