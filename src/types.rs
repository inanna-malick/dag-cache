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
    name: String,
    hash: HashPointer,
    size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSPutResp {
    pub hash: HashPointer,
}

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
    links: Vec<IpfsHeader>,
    data: encodings::Base64,
}
