use actix_multipart_rfc7578::client::multipart;
use actix_web::{client, http};
use futures::future::Future;
use http::header::CONTENT_TYPE;
use http::uri;
use serde::{Deserialize, Serialize};
use serde_json;
use std::io::Cursor;

use crate::api_types::DagCacheError;
use crate::encoding_types;
use crate::ipfs_types;

use tracing::{event, info, span, Level};
use tracing_futures::Instrument;

pub struct IPFSNode(http::uri::Authority);

impl IPFSNode {
    pub fn new(a: http::uri::Authority) -> Self {
        IPFSNode(a)
    }

    // TODO: bias cache strongly towards small nodes
    pub fn get(
        &self,
        k: ipfs_types::IPFSHash,
    ) -> Box<dyn Future<Item = ipfs_types::DagNode, Error = DagCacheError>> {
        let pnq = "/api/v0/object/get?data-encoding=base64&arg=".to_owned() + &k.to_string();
        let pnq_prime: uri::PathAndQuery = pnq
            .parse()
            // TODO: move to some lib that provides better builder fn, should not have to run partial parse fn
            .expect("uri path and query component build failed (should not be possible, base58 is uri safe)");
        let u = uri::Uri::builder()
            .scheme("http")
            .authority(self.0.clone()) // feels weird to be cloning this ~constant value
            .path_and_query(pnq_prime)
            .build()
            .expect("uri build failed(???)");

        let f = client::Client::new()
            .get(&u)
            .send()
            .map_err(|_e| DagCacheError::IPFSError) // todo: wrap originating error
            .and_then(|mut res| {
                event!(Level::TRACE, msg = "attempting to parse resp");
                client::ClientResponse::json(&mut res).map(|DagNode{links,data}| ipfs_types::DagNode{ data: data, links: links.into_iter().map(|IPFSHeader{hash, name, size}| ipfs_types::IPFSHeader{hash, name, size}).collect()} ).map_err(|e| { // FIXME: lmao

                    event!(Level::ERROR,  msg = "failed parsing json", response.error = ?e);
                    DagCacheError::IPFSJsonError
                }).and_then(|x| {

                    event!(Level::INFO, msg = "successfully parsed resp");
                    Ok(x)

                })
            })
            // NOTE: outermost in chain wraps all previous b/c poll model
            .instrument(span!(Level::TRACE, "ipfs-get", hash_pointer = k.to_string().as_str(), uri = ?u ));

        Box::new(f)
    }

    pub fn put(
        &self,
        v: ipfs_types::DagNode,
    ) -> Box<dyn Future<Item = ipfs_types::IPFSHash, Error = DagCacheError>> {
        let u = uri::Uri::builder()
            .scheme("http")
            .authority(self.0.clone()) // feels weird to be cloning this ~constant value
            .path_and_query("/api/v0/object/put?datafieldenc=base64")
            .build()
            .expect("uri build failed(???)");

        println!("hitting url: {:?}", &u);

        let v = DagNode {
            data: v.data,
            links: v
                .links
                .into_iter()
                .map(|ipfs_types::IPFSHeader { hash, name, size }| IPFSHeader { hash, name, size })
                .collect(),
        };
        let bytes = serde_json::to_vec(&v).expect("json _serialize_ failed (should be impossible)");

        let cursor = Cursor::new(bytes);

        let mut form = multipart::Form::default();
        form.add_reader_file("file", cursor, "data"); // 'name'/'data' is mock filename/name(?)..

        let header: &str = &("multipart/form-data; boundary=".to_owned() + &form.boundary);

        let body: multipart::Body = multipart::Body::from(form);

        // TODO: figure out how to minimize this
        let body = futures::stream::Stream::map_err(body, |_e| DagCacheError::IPFSError);

        let f = client::Client::new()
            .post(&u)
            .header(CONTENT_TYPE, header)
            .send_stream(body)
            .map_err(|_e| DagCacheError::IPFSError)
            .and_then(|mut res| {
                // client::ClientResponse::json(&mut res)
                //     .map_err(|e| {
                //         println!("error converting response body to json: {:?}", e);
                //         DagCacheError::IPFSJsonError
                //     })

                // TODO: flow here is crap, crap crap - unify errors, meaningful enum, etc
                // NOTE: error handling is kinda crap in the more-ergonomic json extractor case
                // NOTE: so leave like this, todo later is to branch on response code and either consume
                // NOTE: as json (via ) or log
                client::ClientResponse::body(&mut res)
                    .map_err(|e| {
                        event!(Level::ERROR, msg = "error getting response body(?)", err = ?e);
                        DagCacheError::IPFSJsonError
                    })
                    .and_then(|b| {
                        println!("raw bytes from resp: {:?}", b);
                        let cursor = Cursor::new(b);
                        let res: IPFSPutResp = serde_json::de::from_reader(cursor).expect("lmao, will fail"); // TODO: not this (FIXME)

                        info!("test");
                        Ok(res.hash)
                    })
            })
            .instrument(span!(Level::TRACE, "ipfs-put", uri = ?u));

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
