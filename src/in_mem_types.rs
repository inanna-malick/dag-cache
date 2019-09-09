
use cargo::ipfs_types;

// ephemeral, used for data structure in memory, should it be here? mb not
pub struct DagNode {
    pub links: Vec<DagNodeLink>, // list of pointers - either to elems in this bulk req or already-uploaded
    pub data: encoding_types::Base64, // this node's data
}

pub enum DagNodeLink {
    Local(ClientSideHash, Box<DagNode>),
    Remote(ipfs_types::IPFSHeader),
}

#[derive(Debug)]
pub enum DagNodeBuildErr {
    InvalidLink(ClientSideHash),
}

impl DagNode {
    pub fn build(
        entry: api_types::DagNode,
        remaining: &mut std::collections::HashMap<ClientSideHash, api_types::DagNode>,
    ) -> Result<DagNode, DagNodeBuildErr> {
        let DagCacheToPut { links, data } = entry;

        let links = links
            .into_iter()
            .map(|x| match x {
                api_types::DagNodeLink::Local(csh) => match remaining.remove(&csh) {
                    Some(dctp) => {
                        Self::build(dctp, remaining).map(|x| DagNodeLink::Local(csh, Box::new(x)))
                    }
                    None => Err(DagNodeBuildErr(csh)),
                },
                api_types::DagNodeLink::Remote(nh) => Ok(DagNodeLink::Remote(nh)),
            })
            .collect::<Result<Vec<DagNode>, DagNodeBuildErr>>()?;

        Ok(DagNode { links, data })
    }
}
