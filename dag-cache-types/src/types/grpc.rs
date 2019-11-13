//re-export macro magic
pub use ipfscache::*;

pub mod ipfscache {
    tonic::include_proto!("ipfscache");
}
