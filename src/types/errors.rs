use tonic::Code;
use tonic::Status;

#[derive(Debug)]
pub enum DagCacheError {
    IPFSError,
    IPFSJsonError,
    ProtoDecodingError(ProtoDecodingError),
    UnexpectedError { msg: String },
}

impl From<DagCacheError> for Status {
    fn from(error: DagCacheError) -> Status {
        match error {
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

impl From<ProtoDecodingError> for Status {
    fn from(error: ProtoDecodingError) -> Status {
        std::convert::From::from(DagCacheError::ProtoDecodingError(error))
    }
}
