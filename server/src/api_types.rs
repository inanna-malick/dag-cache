use actix_web::{error, http, HttpResponse};
use failure::Fail;
use serde::{Deserialize, Serialize};

use crate::encoding_types::{Base58};

#[derive(Fail, Debug)]
pub enum DagCacheError {
    #[fail(display = "ipfs error")]
    IPFSError,
    #[fail(display = "ipfs json error")]
    IPFSJsonError,
    // #[fail(display = "ipfs json error, foo: {:?}")]
    // IPFSJsonError(Foo), // todo, look at docs :)
}

impl error::ResponseError for DagCacheError {
    fn error_response(&self) -> HttpResponse {
        match self {
            // TODO: add more info here later
            _ => HttpResponse::new(http::StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct ClientSideHash(Base58);
impl ClientSideHash {
    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

pub mod bulk_put {
    use super::ClientSideHash;
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
        pub entry_point: (ClientSideHash, DagNode),
        pub nodes: Vec<(ClientSideHash, DagNode)>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct DagNode {
        pub links: Vec<DagNodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
        pub data: Base64,            // this node's data
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub enum DagNodeLink {
        Local(ClientSideHash),
        Remote(ipfs_types::IPFSHeader),
    }
}

pub mod get {
    use crate::ipfs_types;
    use serde::{Deserialize, Serialize};

    // ~= NonEmptyList (head, rest struct)
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Resp {
        pub requested_node: ipfs_types::DagNode,
        pub extra_node_count: usize,
        pub extra_nodes: Vec<ipfs_types::DagNodeWithHeader>,
    }
}
