// use crate::server::app::ipfscache::{server, BulkPutReq, GetResp, IpfsHash, IpfsNode};

//re-export macro magic
pub use ipfscache::{
    bulk_put_link, server, BulkPutIpfsNode, BulkPutIpfsNodeWithHash, BulkPutLink, BulkPutReq,
    ClientSideHash, GetResp, IpfsHash, IpfsHeader, IpfsNode, IpfsNodeWithHeader,
};

// question not the gprc macro magic (I sadly have no idea what this does, or how)
pub mod ipfscache {
    include!(concat!(env!("OUT_DIR"), "/ipfscache.rs"));
}
