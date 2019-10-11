use crate::capabilities::IPFSCapability;
use crate::types::encodings;
use crate::types::errors::DagCacheError;
use crate::types::ipfs;
use async_trait::async_trait;
use futures::compat::Future01CompatExt;
use futures01::future::Future;
use reqwest::r#async::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json;
use tracing::{event, span, Level};
use tracing_futures::Instrument;

pub struct IPFSNode(reqwest::Url); //base url, copy mutated to produce specific path. should have no path component

impl IPFSNode {
    pub fn new(a: reqwest::Url) -> Self { IPFSNode(a) }
}

#[async_trait]
impl IPFSCapability for IPFSNode {
    async fn get(&self, k: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError> {
        let mut url = self.0.clone();
        url.set_path("api/v0/object/get");
        url.query_pairs_mut()
            .append_pair("data-encoding", "base64")
            .append_pair("arg", &k.to_string());

        let f = Client::new()
            .get(url.clone())
            .send()
            .map_err(|e| {
                event!(Level::ERROR,  msg = "failed getting node from IPFS", response.error = ?e);
                DagCacheError::IPFSError
            })
            .and_then(|mut x| {
                x.json().map_err(|e| {
                    event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                    DagCacheError::IPFSJsonError
                })
            })
            .map(|e: DagNode| ipfs::DagNode {
                data: e.data,
                links: e
                    .links
                    .into_iter()
                    .map(|IPFSHeader { hash, name, size }| ipfs::IPFSHeader { hash, name, size })
                    .collect(),
            })
            .instrument(
                span!(Level::TRACE, "ipfs-get", hash_pointer = k.to_string().as_str(), url = ?url ),
            );

        f.compat().await
    }

    async fn put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError> {
        let mut url = self.0.clone();
        url.set_path("api/v0/object/put");
        url.query_pairs_mut().append_pair("datafieldenc", "base64");

        let v = DagNode {
            data: v.data,
            links: v
                .links
                .into_iter()
                .map(|ipfs::IPFSHeader { hash, name, size }| IPFSHeader { hash, name, size })
                .collect(),
        };
        let bytes = serde_json::to_vec(&v).expect("json _serialize_ failed (should be impossible)");

        event!(Level::DEBUG, ipfs_put_body = ?std::str::from_utf8(&bytes));

        let part = multipart::Part::bytes(bytes).file_name("data"); // or vice versa, idk
        let form = multipart::Form::new().part("file", part);

        let f = Client::new()
            .post(url.clone())
            .multipart(form)
            .send()
            .and_then(|mut x| x.json())
            .map(|IPFSPutResp { hash }| hash)
            .map_err(|e| {
                event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                DagCacheError::IPFSJsonError
            })
            .instrument(span!(Level::TRACE, "ipfs-put", url = ?url ));

        f.compat().await
    }
}

// IPFS API resp types live here, not a huge fan of their json format - stays here
// NOTE: these mirror types in ipfs, only difference is upper-case first char in json field names
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSPutResp {
    pub hash: ipfs::IPFSHash,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct IPFSHeader {
    pub name: String,
    pub hash: ipfs::IPFSHash,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DagNode {
    pub links: Vec<IPFSHeader>,
    pub data: encodings::Base64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib;
    use crate::types::encodings::Base64;
    use crate::types::ipfs::{DagNode, IPFSHash, IPFSHeader};
    use futures::future::{FutureExt, TryFutureExt};
    use rand;

    #[test]
    fn test_put_and_get() {
        lib::run_test(|| {
            let f = test_put_and_get_worker().boxed().compat();
            Box::new(f)
        })
    }

    // NOTE: assumes IPFS daemon running locally at localhost:5001. Daemon can be shared between tests.
    async fn test_put_and_get_worker() -> Result<(), String> {
        let mut random_bytes = vec![];

        for _ in 0..64 {
            random_bytes.push(rand::random())
        }

        let header = IPFSHeader {
            name: "foo".to_string(),
            // needs to be a valid IPFS hash (length, encoding bit, etc), so just use a randomly-chosen one
            hash: IPFSHash::from_string("QmVC1ZwqPxSzs1KyrSJdgF1zfEFTNGwBGRadx5aEfJV6Q9").unwrap(),
            size: 1337,
        };
        let input = DagNode {
            links: vec![header],
            data: Base64(random_bytes),
        };

        let ipfs_node = IPFSNode::new(reqwest::Url::parse("http://localhost:5001").unwrap());

        let input_hash = ipfs_node.put(input.clone()).await.map_err(|e| {
            println!("error handler for ipfs put");
            format!("ipfs put error: {:?}", e)
        })?;

        println!("ipfs put done, running get");
        let output = ipfs_node
            .get(input_hash)
            .await
            .map_err(|e| format!("ipfs get error: {:?}", e))?;

        if input == output {
            Ok(())
        } else {
            Err(format!("input {:?} != output {:?}", input, output))
        }
    }
}
