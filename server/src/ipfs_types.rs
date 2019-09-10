use actix_web::{error, http, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::encoding_types;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IPFSHeader {
    pub name: String,
    pub hash: IPFSHash,
    pub size: u64,
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct IPFSHash(encoding_types::Base58);

impl IPFSHash {
    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encoding_types::Base64,
}

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeWithHash {
    pub hash: IPFSHeader,
    pub node: DagNode,
}
