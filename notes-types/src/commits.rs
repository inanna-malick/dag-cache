use crate::api::{ParseError, Result};
use crate::notes::{self, NoteHash};
use dag_store_types::types::api;
use dag_store_types::types::domain::TypedHash;
use dag_store_types::types::{domain, encodings};
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
pub struct Commit<R> {
    pub parents: Vec<CommitHash>,
    pub root_note: R,
}

pub type CannonicalCommit = Commit<domain::Id>;

pub type CommitHash = TypedHash<CannonicalCommit>;

//cannonical form
impl CannonicalCommit {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let res = serde_json::to_vec(self)?;
        Ok(res)
    }

    pub fn decode(v: &[u8]) -> Result<Self> {
        let res = serde_json::from_slice(v)?;
        Ok(res)
    }

    pub fn into_generic(self) -> api::bulk_put::Node {
        let data = Commit::encode(&self).expect("encoding commit failed (?)");
        let data = encodings::Base64(data);

        let mut links: Vec<api::bulk_put::NodeLink> = self
            .parents
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let hdr = domain::Header {
                    size: 0, // TODO: FIXME impl or drop size field. idk.
                    id: domain::Id(i as u128),
                    hash: h.demote(),
                };
                api::bulk_put::NodeLink::Remote(hdr)
            })
            .collect();

        // add link for root note
        links.push(api::bulk_put::NodeLink::Local(
            notes::NodeId::root().into_generic(),
        ));

        api::bulk_put::Node { data, links }
    }
}

impl Commit<NoteHash> {
    pub fn from_generic(g: domain::Node) -> Result<Self> {
        // parse as Commit<domain::Id>
        let commit: Commit<domain::Id> = Commit::decode(&g.data.0[..])?;

        let root_node_hdr = g
            .links
            .iter()
            .find(|hdr| hdr.id == commit.root_note)
            .ok_or(Box::new(ParseError(
                "expected to find magic root node id hdr in commit links".to_string(),
            )))?;

        let c = Commit {
            parents: commit.parents,
            root_note: root_node_hdr.hash.promote(),
        };
        Ok(c)
    }
}

// FIXME/TODO/NOTE: all kinds of potential problems if this colides w/ a generated id
// get it? because it's the root node? ehh
// pub const MAGIC_ROOT_NOTE_ID: domain::Id = domain::Id(1337);
