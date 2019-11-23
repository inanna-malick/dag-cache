use crate::capabilities::IPFSCapability;
use dag_cache_types::types::encodings;
use dag_cache_types::types::errors::DagCacheError;
use dag_cache_types::types::ipfs;
use reqwest::r#async::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json;
use tracing::instrument;
use tracing::{event, Level};

pub struct IPFSNode(reqwest::Url); //base url, copy mutated to produce specific path. should have no path component

impl IPFSNode {
    pub fn new(a: reqwest::Url) -> Self {
        IPFSNode(a)
    }
}

impl IPFSNode {
    // TODO TODO FIXME: handle case where id not known to IPFS via means other than indefinite timeout...
    // FIXME FIXME FIXME
    #[instrument(skip(self))]
    async fn get_(&self, hash: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError> {
        println!("ipfs get start");

        let mut url = self.0.clone();
        url.set_path("api/v0/object/get");
        url.query_pairs_mut()
            .append_pair("data-encoding", "base64")
            .append_pair("arg", &hash.to_string());

        // TODO: shared client? mb global in ipfs node? per thread? lmao idk.
        let resp = Client::new().get(url.clone()).send().await.map_err(|e| {
            event!(Level::ERROR,  msg = "failed getting node from IPFS", response.error = ?e);
            DagCacheError::IPFSError
        })?;

        let node: DagNode = resp.json().await.map_err(|e| {
            event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
            // TODO: all domain errors sent as events via telemetry from generic app wrapper
            DagCacheError::IPFSJsonError
        })?;

        let node = ipfs::DagNode {
            data: node.data,
            links: node
                .links
                .into_iter()
                .map(|IPFSHeader { hash, name, size }| ipfs::IPFSHeader { hash, name, size })
                .collect(),
        };

        println!("ipfs get done");

        Ok(node)
    }

    #[instrument(skip(self, v))]
    async fn put_(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError> {
        println!("ipfs put start");

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

        let resp = Client::new()
            .post(url.clone())
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                DagCacheError::IPFSError
            })?;

        let IPFSPutResp { hash } = resp.json().await.map_err(|e| {
            event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
            DagCacheError::IPFSJsonError
        })?;

        println!("ipfs put done");

        Ok(hash)
    }
}

#[tonic::async_trait]
impl IPFSCapability for IPFSNode {
    async fn get(&self, hash: ipfs::IPFSHash) -> Result<ipfs::DagNode, DagCacheError> {
        self.get_(hash).await
    }

    async fn put(&self, v: ipfs::DagNode) -> Result<ipfs::IPFSHash, DagCacheError> {
        self.put_(v).await
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
    use crate::utils;
    use dag_cache_types::types::encodings::Base64;
    use dag_cache_types::types::ipfs::{DagNode, IPFSHash, IPFSHeader};
    use rand;

    // NOTE: assumes IPFS daemon running locally at localhost:5001. Daemon can be shared between tests.
    #[tokio::test]
    async fn test_put_and_get() {
        utils::init_test_env(); // tracing subscriber

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

        let input_hash = ipfs_node.put(input.clone()).await.expect("ipfs put error");

        let output = ipfs_node.get(input_hash).await.expect("ipfs get error");

        assert!(input == output);
    }
}
