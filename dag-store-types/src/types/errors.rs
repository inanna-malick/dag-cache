use crate::types::domain::Hash;
use std::error::Error;
#[cfg(feature = "grpc")]
use tonic::{Code, Status};

#[derive(Debug)]
pub enum DagCacheError {
    ProtoDecodingError(ProtoDecodingError),
    UnexpectedError(String),
    CASViolationError { actual_hash: Option<Hash> },
}

impl DagCacheError {
    pub fn unexpected<E: std::error::Error>(e: E) -> Self {
        DagCacheError::UnexpectedError(format!("unexpected error: {}", e))
    }
}

#[cfg(feature = "grpc")]
impl From<DagCacheError> for Status {
    fn from(error: DagCacheError) -> Status {
        match error {
            DagCacheError::ProtoDecodingError(de) => Status::new(
                Code::InvalidArgument,
                format!("error decoding proto, {:?}", de),
            ),
            DagCacheError::UnexpectedError(s) => {
                Status::new(Code::Internal, format!("unexpected error: {:?}", s))
            }
            DagCacheError::CASViolationError { actual_hash } => Status::new(
                Code::DeadlineExceeded,
                format!("cas violation: actual: {:?}", actual_hash),
            ),
        }
    }
}
impl From<ProtoDecodingError> for DagCacheError {
    fn from(error: ProtoDecodingError) -> DagCacheError {
        DagCacheError::ProtoDecodingError(error)
    }
}

#[derive(Debug)]
pub struct ProtoDecodingError(pub String);

impl std::fmt::Display for ProtoDecodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(feature = "grpc")]
impl From<ProtoDecodingError> for Status {
    fn from(error: ProtoDecodingError) -> Status {
        std::convert::From::from(DagCacheError::ProtoDecodingError(error))
    }
}

impl Error for ProtoDecodingError {
    fn description(&self) -> &str {
        &self.0
    }

    fn cause(&self) -> Option<&dyn Error> {
        None
    }
}
