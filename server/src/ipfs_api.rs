// use actix_multipart_rfc7578::client::multipart;
// use actix_web::{client, http};
use futures::future::Future;
// use http::uri;
use serde::{Deserialize, Serialize};
use serde_json;

use crate::api_types::DagCacheError;
use crate::encoding_types;
use crate::ipfs_types;
use crate::lib::BoxFuture;

use tracing::{event, span, Level};
use tracing_futures::Instrument;

use reqwest::r#async::*; // todo, idk

pub struct IPFSNode(reqwest::Url); //base url, copy mutated to produce specific path. should have no path component

pub trait IPFSCapability {
    fn get(&self, k: ipfs_types::IPFSHash) -> BoxFuture<ipfs_types::DagNode, DagCacheError>;

    fn put(&self, v: ipfs_types::DagNode) -> BoxFuture<ipfs_types::IPFSHash, DagCacheError>;
}

impl IPFSNode {
    pub fn new(a: reqwest::Url) -> Self {
        IPFSNode(a)
    }
}

impl IPFSCapability for IPFSNode {
    // TODO: bias cache strongly towards small nodes
    fn get(&self, k: ipfs_types::IPFSHash) -> BoxFuture<ipfs_types::DagNode, DagCacheError> {
        let mut url = self.0.clone();
        url.set_path("api/v0/object/get");
        url.query_pairs_mut()
            .append_pair("data-encoding", "base64")
            .append_pair("arg", &k.to_string());

        println!("hitting url: {:?}", &url);

        let f = Client::new()
            .get(url.clone())
            .send()
            .and_then(|mut x| x.json())
            .map( |e: DagNode|
                   ipfs_types::DagNode {
                       data: e.data,
                       links: e
                           .links
                           .into_iter()
                           .map(|IPFSHeader { hash, name, size }| ipfs_types::IPFSHeader { hash, name, size })
                           .collect(),
                   }
            )
            .map_err(|e| {
                event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                DagCacheError::IPFSJsonError
            })
            .instrument(
                span!(Level::TRACE, "ipfs-get", hash_pointer = k.to_string().as_str(), url = ?url ),
            );

        Box::new(f)
    }

    fn put(&self, v: ipfs_types::DagNode) -> BoxFuture<ipfs_types::IPFSHash, DagCacheError> {
        let mut url = self.0.clone();
        url.set_path("api/v0/object/put");
        url.query_pairs_mut().append_pair("datafieldenc", "base64");

        println!("hitting url: {:?}", &url);

        let v = DagNode {
            data: v.data,
            links: v
                .links
                .into_iter()
                .map(|ipfs_types::IPFSHeader { hash, name, size }| IPFSHeader { hash, name, size })
                .collect(),
        };
        let bytes = serde_json::to_vec(&v).expect("json _serialize_ failed (should be impossible)");

        println!("sending bytes: {:?}", &std::str::from_utf8(&bytes));

        let part = multipart::Part::bytes(bytes).file_name("data"); // or vice versa, idk
        let form = multipart::Form::new().part("file", part);

        let f = Client::new()
            .post(url.clone())
            .multipart(form)
            .send()
            .and_then(|mut x| x.json())
            .map(|IPFSPutResp{hash}| hash )
            .map_err(|e| {
                event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                DagCacheError::IPFSJsonError
            })
            .instrument(span!(Level::TRACE, "ipfs-put", url = ?url ));

        Box::new(f)
    }
}

// IPFS API resp types - lives here, not a huge fan of their json format - stays here
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSPutResp {
    pub hash: ipfs_types::IPFSHash,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSHeader {
    pub name: String,
    pub hash: ipfs_types::IPFSHash,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encoding_types::Base64,
}

//NOTE: why is this here? FIXME
// exists primarily to have better serialized json (tuples result in 2-elem lists)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DagNodeWithHash {
    pub hash: IPFSHeader,
    pub node: DagNode,
}
