use serde::{Deserialize, Serialize};

use crate::encoding_types;

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct IPFSHeader {
    pub name: String,
    pub hash: IPFSHash,
    pub size: u64,
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct IPFSHash(encoding_types::Base58);

impl IPFSHash {
    pub fn from_string(x: &str) -> Result<IPFSHash, base58::FromBase58Error> {
        encoding_types::Base58::from_string(x).map(Self::from_raw)
    }

    // probably unsafe, but, like, what do I look like, a cop?
    pub fn from_raw(raw: encoding_types::Base58) -> IPFSHash {
        IPFSHash(raw)
    }

    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Deserialize, Serialize)]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encoding_types::Base64,
}

// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeWithHeader {
    pub header: IPFSHeader,
    pub node: DagNode,
}
