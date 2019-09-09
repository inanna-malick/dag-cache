use actix_web::{error, http, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::encoding_types;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSHeader {
    // todo more fitting name? idk
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
#[serde(rename_all = "PascalCase")]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encoding_types::Base64,
}
