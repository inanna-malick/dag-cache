//re-export macro magic
pub use dagstore::*;

pub mod dagstore {
    tonic::include_proto!("dagstore");
}
