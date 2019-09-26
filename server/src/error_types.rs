use crate::server::ipfscache as proto;
use failure::Fail;
use serde::{Deserialize, Serialize};

use crate::encoding_types::Base58;
use tower_grpc::Code;
use tower_grpc::Status;

//TODO: remove 'Fail' entirely, see what breaks
#[derive(Fail, Debug)]
pub enum DagCacheError {
    #[fail(display = "ipfs error")]
    IPFSError,
    #[fail(display = "ipfs json parse error")] // FIXME - does this make sense here?
    IPFSJsonError,
    #[fail(display = "error decoding input")] // FIXME - does this make sense here?
    ProtoDecodingError(ProtoDecodingError),
    #[fail(display = "unexpected error: {}", msg)]
    UnexpectedError { msg: String },
}

impl DagCacheError {
    pub fn into_status(self) -> Status {
        match self {
            DagCacheError::IPFSError => Status::new(Code::Internal, "ipfs error"),
            DagCacheError::IPFSJsonError => Status::new(Code::Internal, "ipfs json error"),
            DagCacheError::ProtoDecodingError(de) => Status::new(
                Code::InvalidArgument,
                "error decoding proto, ".to_owned() + &de.cause,
            ),
            DagCacheError::UnexpectedError { msg: s } => {
                Status::new(Code::Internal, "unexpected error, ".to_owned() + &s)
            }
        }
    }
}

#[derive(Debug)]
pub struct ProtoDecodingError {
    pub cause: String,
}
