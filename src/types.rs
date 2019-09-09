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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSHeader {
    // todo more fitting name? idk
    pub name: String,
    pub hash: IPFSHash,
    pub size: u64,
}

// idea is that a put req will contain some number of nodes, with only client-side blake hashing performed.
// all hash links in body will solely use blake hash. ipfs is then treated as an implementation detail
// with parsing-time traverse op to pair each blake hash with the full ipfs hash from the links field
// of the dag node - dag node link fields would use name = blake2 hash (base58) (same format, why not, eh?)
// goal of all this is to be able to send fully packed large requests as lists of many nodes w/ just blake2 pointers
// (note: needs to be, like, {body, either (blake2hash, ipfshash)} )
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DagCacheBulkPutReq {
    pub entry_point: (ClientSideHash, DagCacheToPut),
    pub nodes: Vec<(ClientSideHash, DagCacheToPut)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DagCacheToPut {
    pub links: Vec<BulkDagCachePutHashLink>, // list of pointers - either to elems in this bulk req or already-uploaded
    pub data: encodings::Base64,             // this node's data
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BulkDagCachePutHashLink {
    NodeInThisReq(ClientSideHash),
    NodeInIpfs(IPFSHeader),
}

// ephemeral, used for data structure in memory, should it be here? mb not
pub struct DagCacheToPutInMem {
    pub links: Vec<BulkDagCachePutHashLinkInMem>, // list of pointers - either to elems in this bulk req or already-uploaded
    pub data: encodings::Base64,                  // this node's data
}

// ephemeral, used for data structure in memory, should it be here? mb not
pub enum BulkDagCachePutHashLinkInMem {
    NodeInThisReqInMem(ClientSideHash, Box<DagCacheToPutInMem>),
    NodeInIpfsInMem(IPFSHeader),
}

impl DagCacheToPutInMem {
    pub fn build(
        entry: DagCacheToPut,
        remaining: &mut std::collections::HashMap<ClientSideHash, DagCacheToPut>,
    ) -> Result<DagCacheToPutInMem, String> {
        let DagCacheToPut { links, data } = entry;

        let links = links.into_iter().map( |x| {
            match x {
                BulkDagCachePutHashLink::NodeInThisReq(csh) => {
                    match remaining.remove(&csh) {
                        Some(dctp) => Self::build(dctp, remaining).map(|x| {
                            BulkDagCachePutHashLinkInMem::NodeInThisReqInMem(csh, Box::new(x))

                        }),
                        None       => Err("failure building dag cache tree from bulk req - client side hash link broken for".to_string() + &csh.to_string()),
                    }},
                BulkDagCachePutHashLink::NodeInIpfs(nh) => {
                    Ok(BulkDagCachePutHashLinkInMem::NodeInIpfsInMem(nh))
                },

            }
        }).collect::<Result<Vec<BulkDagCachePutHashLinkInMem>, String>>()?;

        Ok(DagCacheToPutInMem { links, data })
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct ClientSideHash(encodings::Base58);
impl ClientSideHash {
    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSPutResp {
    // note: todo: schema is all fucky, should have 1x type specific to ipfs api, 1x for mine
    pub hash: IPFSHash,
}

// NOTE: would be cool if I knew these were constant size instead of having a vec
#[derive(Clone, Hash, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct IPFSHash(encodings::Base58);

impl IPFSHash {
    pub fn to_string<'a>(&self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encodings::Base64,
}

// ~= NonEmptyList (head, rest struct)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeGetResp {
    pub requested_node: (IPFSHash, DagNode),
    pub extra_node_count: usize,
    pub extra_nodes: Vec<(IPFSHash, DagNode)>, // will likely result in ugly json from tuples, but w/e
}
