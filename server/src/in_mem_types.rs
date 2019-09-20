// mod ipfs_types;
use crate::api_types;
use crate::api_types::ClientSideHash;
use crate::encoding_types;
use crate::ipfs_types;

// ephemeral, used for data structure in memory
#[derive(Clone)]
pub struct DagTree { // TODO: better name
    pub links: Vec<DagTreeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
    pub data: encoding_types::Base64, // this node's data
}

#[derive(Clone)]
pub enum DagTreeLink {
    Local(ClientSideHash, Box<DagTree>),
    Remote(ipfs_types::IPFSHeader),
}

#[derive(Debug)]
pub enum DagTreeBuildErr {
    InvalidLink(ClientSideHash),
}

impl DagTree {
    // TODO: should error if remaining not empty? or return remaining nodes in tuple res
    pub fn build(
        entry: api_types::bulk_put::DagNode,
        remaining: &mut std::collections::HashMap<ClientSideHash, api_types::bulk_put::DagNode>,
    ) -> Result<DagTree, DagTreeBuildErr> {
        let api_types::bulk_put::DagNode { links, data } = entry;

        let links = links
            .into_iter()
            .map(|x| match x {
                api_types::bulk_put::DagNodeLink::Local(csh) => match remaining.remove(&csh) {
                    Some(dctp) => {
                        Self::build(dctp, remaining).map(|x| DagTreeLink::Local(csh, Box::new(x)))
                    }
                    None => Err(DagTreeBuildErr::InvalidLink(csh)),
                },
                api_types::bulk_put::DagNodeLink::Remote(nh) => Ok(DagTreeLink::Remote(nh)),
            })
            .collect::<Result<Vec<DagTreeLink>, DagTreeBuildErr>>()?;

        Ok(DagTree { links, data })
    }
}
