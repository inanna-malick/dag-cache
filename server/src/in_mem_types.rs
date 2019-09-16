// mod ipfs_types;
use crate::api_types;
use crate::api_types::ClientSideHash;
use crate::encoding_types;
use crate::ipfs_types;

// ephemeral, used for data structure in memory

#[derive(Clone)]
pub struct DagNode {
    pub links: Vec<DagNodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
    pub data: encoding_types::Base64, // this node's data
}

#[derive(Clone)]
pub enum DagNodeLink {
    Local(ClientSideHash, Box<DagNode>),
    Remote(ipfs_types::IPFSHeader),
}

#[derive(Debug)]
pub enum DagNodeBuildErr {
    InvalidLink(ClientSideHash),
}

impl DagNode {
    // TODO: should error if remaining not empty? or return remaining nodes in tuple res
    pub fn build(
        entry: api_types::bulk_put::DagNode,
        remaining: &mut std::collections::HashMap<ClientSideHash, api_types::bulk_put::DagNode>,
    ) -> Result<DagNode, DagNodeBuildErr> {
        let api_types::bulk_put::DagNode { links, data } = entry;

        let links = links
            .into_iter()
            .map(|x| match x {
                api_types::bulk_put::DagNodeLink::Local(csh) => match remaining.remove(&csh) {
                    Some(dctp) => {
                        Self::build(dctp, remaining).map(|x| DagNodeLink::Local(csh, Box::new(x)))
                    }
                    None => Err(DagNodeBuildErr::InvalidLink(csh)),
                },
                api_types::bulk_put::DagNodeLink::Remote(nh) => Ok(DagNodeLink::Remote(nh)),
            })
            .collect::<Result<Vec<DagNodeLink>, DagNodeBuildErr>>()?;

        Ok(DagNode { links, data })
    }
}