use actix_web::{error, http, HttpResponse};
use failure::Fail;
use serde::{Deserialize, Serialize};

// pub mod types;
pub mod encodings; // still confused by the specifics of this
                   // use encodings;

#[derive(Fail, Debug)]
pub enum DagCacheError {
    #[fail(display = "ipfs error")]
    IPFSError,
    #[fail(display = "ipfs json error")]
    IPFSJsonError,
}

impl error::ResponseError for DagCacheError {
    fn error_response(&self) -> HttpResponse {
        match self {
            // TODO: add more info here later
            _ => HttpResponse::new(http::StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IpfsHeader {
    // todo more fitting name? idk
    pub name: String,
    pub hash: HashPointer,
    pub size: u64,
}

// idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
// all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
// with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
// of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
// goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
// (note: needs to be, like, {body, either (blake2hash, ipfshash)} )
// pub struct DagCachePutReq {
//
// }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSPutResp {
    // note: todo: schema is all fucky, should have 1x type specific to ipfs api, 1x for mine
    pub hash: HashPointer,
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HashPointer(encodings::Base58);

impl HashPointer {
    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DagNode {
    pub links: Vec<IpfsHeader>,
    pub data: encodings::Base64,
}

// ~= NonEmptyList (head, rest struct)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeGetResp {
    pub requested_node: (HashPointer, DagNode),
    pub extra_node_count: usize,
    pub extra_nodes: Vec<(HashPointer, DagNode)>, // will likely result in ugly json from tuples, but w/e
}
