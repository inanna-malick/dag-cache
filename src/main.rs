use actix;
use actix_web::{error, client, http, web, App, HttpServer, HttpResponse};
use std::sync::Mutex;
use lru::LruCache;
use serde::{de, Deserialize, Serialize, Serializer, Deserializer};

use base64;

use futures::future::Future;
use futures::future;
use failure::Fail;
use base58;
use http::uri;

use http::header::CONTENT_TYPE;

use serde_json;

use std::io::Cursor;

use actix_multipart_rfc7578::client::{multipart};

// use reqwest::multipart;


// TODO:
// - use multicache or similar (weighted lru) to have upper bound on size, not cardinality

#[derive(Fail, Debug)]
enum DagCacheError {
    #[fail(display = "ipfs error")]
    IPFSError,
    #[fail(display = "ipfs error")]
    IPFSJsonError,
    #[fail(display = "unkhttps://www.muji.us/store/4550002185565.htmlnown error")]
    UnknownError,
}

impl error::ResponseError for DagCacheError {
    fn error_response(&self) -> HttpResponse {
        match *self { // will add more info here later
            _ => {
                HttpResponse::new(http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct IpfsHeader {
    name: String,
    hash: DagNodeLink,
    size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct IPFSPutResp {
    hash: DagNodeLink,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct DagNodeLink(Vec<u8>);

// always serialize as string (json)
impl Serialize for DagNodeLink {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base58::ToBase58::to_base58(&self.0[..]))
    }
}

impl<'de> Deserialize<'de> for DagNodeLink {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = &String::deserialize(deserializer)?;
        base58::FromBase58::from_base58(s)
            .map(DagNodeLink)
            .map_err(|e| match e {
                base58::FromBase58Error::InvalidBase58Character(c, _) =>
                    de::Error::custom(format!("invalid base58 char {}", c)),
                base58::FromBase58Error::InvalidBase58Length =>
                    de::Error::custom(format!("invalid base58 length(?)")),
            })
    }
}



#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct DagNode {
    links: Vec<IpfsHeader>,
    data: Base64,
}


#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct Base64(Vec<u8>);

// always serialize as string (json)
impl Serialize for Base64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        base64::decode(&s).map(Base64).map_err(de::Error::custom)
    }
}



struct State {
    cache: Mutex<LruCache<DagNodeLink, DagNode>>,
}


fn get(data: web::Data<State>, k: web::Path<(DagNodeLink)>) -> Box<dyn Future<Item = web::Json<DagNode>, Error = DagCacheError>> {
    // unwrap k
    let kprime = k.into_inner().clone();
    // base58-encode key (for logging/url construction)
    let k58 = base58::ToBase58::to_base58(&kprime.0[..]);

    let data = data.into_inner();
    let mutex = data.clone();

    println!("cache get for {:?}", &k58);

    let mut cache = mutex.cache.lock().unwrap(); // <- get cache's MutexGuard
    let mv = cache.get(&kprime);

    println!("cache get for {:?}, result: {:?}", &k58, &mv);

    match mv {
        Some(res) => {
            Box::new(future::ok(web::Json(res.clone()))) // probably bad (clone is mb not needed?)
        }
        None      => {
            let pnq = "/api/v0/object/get?data-encoding=base64&arg=".to_owned() + &k58;
            let pnq_prime: uri::PathAndQuery
                = pnq.parse().expect("uri path and query component build failed (???)");
            let u = uri::Uri::builder()
                .scheme("http") // lmao
                .authority("localhost:5001")
                .path_and_query(pnq_prime)
                .build()
                .expect("uri build failed(???)");

            println!("hitting url: {:?}", u.clone());
            let f = client::Client::new().get(u) // <- hardcoded, lmao
                .send()
                .map_err(|_e| DagCacheError::IPFSError) // todo: wrap originating error
                .and_then(|mut res| {
                    client::ClientResponse::json(&mut res)
                        .map_err(|e| {
                            println!("error converting response body to json: {:?}", e);
                            DagCacheError::IPFSJsonError
                        })
                })
                .and_then( move |dag_node: DagNode| {
                    let mut cache = data.cache.lock().unwrap(); // <- get cache's MutexGuard
                    cache.put(kprime, dag_node.clone()); // todo: log prev item in cache?
                    Ok(web::Json(dag_node))
                });
            Box::new(f)
        }
    }
}





struct TestGenerator;

impl multipart::BoundaryGenerator for TestGenerator {
    fn generate_boundary() -> String {
        // this should not be hardcoded comma lmao
        "------------------------38b3f234-0aa2-4d2d-b0a3-b693724fd735".to_string() // lmao, extreme hack
    }
}

fn put(data: web::Data<State>, v: web::Json<DagNode>) -> Box<dyn Future<Item = web::Json<IPFSPutResp>, Error = DagCacheError>> {

    let vprime = v.into_inner();

    let u = uri::Uri::builder()
        .scheme("http") // lmao
        .authority("localhost:5001")
        .path_and_query("/api/v0/object/put?datafieldenc=base64")
        .build()
        .expect("uri build failed(???)");

    println!("hitting url: {:?}", u.clone());

    let bytes = serde_json::to_vec(&vprime.clone()).expect("json _serialize_ failed(???)");

    let cursor = Cursor::new(bytes);

    let mut form = multipart::Form::new::<TestGenerator>();
    form.add_reader_file("file", cursor, "data"); // 'name'/'data' is mock filename/name(?)..

    let header: &str = "multipart/form-data; boundary=------------------------38b3f234-0aa2-4d2d-b0a3-b693724fd735".as_ref();
    // req.body(I::from(Body::from(self)).into())

    let body: multipart::Body = multipart::Body::from(form);

    // println!("{:?}", body.boundary);

    let body = futures::stream::Stream::map_err(body, |_e| DagCacheError::IPFSError);

    let f = client::Client::new()
        .post(u)
        .header(CONTENT_TYPE, header)
        .send_stream(body)
        .map_err(|_e| DagCacheError::IPFSError)
        .and_then(|mut res| {

            client::ClientResponse::json(&mut res)
                .map_err(|e| {
                    println!("error converting response body to json: {:?}", e);
                    DagCacheError::IPFSJsonError
                })
        })
    .and_then( move |dag_node_link: IPFSPutResp| {
        let mut cache = data.cache.lock().unwrap();
        cache.put(dag_node_link.hash.clone(), vprime);
        Ok(web::Json(dag_node_link))
    }
    );
    Box::new(f)

}


fn main() {

    // PROBLEM: provisioning based on number of entities and _not_ number of bytes allocated total
    //          some dag nodes may be small and some may be large.
    let sys = actix::System::new("system");  // <- create Actix system

    let cache = LruCache::new(2);
    // let state = State{ cache: Mutex::new(cache), client: IpfsClient::default() };
    let state = State{ cache: Mutex::new(cache) };
    let data = web::Data::new(state);

    HttpServer::new(move || {
        println!("init app");
        App::new()
            .register_data(data.clone()) // <- register the created data
            .route("/get/{n}", web::get().to_async(get))
            .route("/put", web::post().to_async(put))
    })
        .bind("127.0.0.1:8088")
        .expect("Can not bind to 127.0.0.1:8088")
        .start();
        // .unwrap()
        // .run()
        // .unwrap();

    let _ = sys.run();  // <- Run actix system, this method actually starts all async processes
}
