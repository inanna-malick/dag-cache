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

// TODO:
// - use multicache or similar (weighted lru) to have upper bound on size, not cardinality

#[derive(Fail, Debug)]
enum DagCacheError {
    #[fail(display = "ipfs error")]
    IPFSError,
    #[fail(display = "ipfs error")]
    IPFSJsonError,
    #[fail(display = "unknown error")]
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

    #[serde(rename = "Type")]
    typ: Option<String>,
}


#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct DagNodeLink(Vec<u8>);

// always serialize as string (json)
impl Serialize for DagNodeLink {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::encode(&self.0))
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

    // let mv = {
    //     let mut cache = data.cache.lock().unwrap(); // <- get cache's MutexGuard
    //     let res = cache.get(&k2.clone());
    //     res.clone() // so as not to prolong lifetime of mut cache
    // }; // explicitly release mutex

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
            // let u = Uri::builder()
            //     .scheme("http") // lmao
            //     .authority("locahost")
            //     .path_and_query()
            //     .build()
            //     .expect("lmao - url build failed");
            let u = "http://localhost:5001/api/v0/object/get?data-encoding=base64&arg=".to_owned() + &k58;
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
                    // let dag_node_links = resp.links.into_iter().map( |l| DagNodeLink{ link: l.hash}).collect();
                    // let dag_node = DagNode
                    //             { body: resp.data
                    //             , links: dag_node_links
                    //             };

                    let mut cache = data.cache.lock().unwrap(); // <- get cache's MutexGuard
                    cache.put(kprime, dag_node.clone()); // todo: log prev item in cache?
                    Ok(web::Json(dag_node))
                });
            Box::new(f)
            // format!("res: {:?}", res)
        }
    }
}

// fn put(data: web::Data<State>, k: web::Path<(DagNodeLink)>, v: web::Json<DagNode>) -> String {
//     let mut cache = data.cache.lock().unwrap(); // <- get cache's MutexGuard

//     let k2 = k.into_inner();
//     let v2 = v.into_inner();

//     cache.put(k2.clone(), v2.clone());

//     format!("wrote {:?} for key {:?}", v2, k2)
// }



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
            .route("/{n}", web::get().to_async(get))
            // .route("/{n}", web::post().to(put))
    })
        .bind("127.0.0.1:8088")
        .expect("Can not bind to 127.0.0.1:8088")
        .start();
        // .unwrap()
        // .run()
        // .unwrap();

    let _ = sys.run();  // <- Run actix system, this method actually starts all async processes
}
