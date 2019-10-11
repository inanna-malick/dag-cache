use crate::capabilities::{Event, TelemetryCapability};
use ::libhoney::{json, Value};
use libhoney::FieldHolder;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Telemetry(Mutex<libhoney::Client<libhoney::transmission::Transmission>>);

impl Event {
    pub fn into_data(self) -> HashMap<String, libhoney::Value> {
        let mut data: HashMap<String, Value> = HashMap::new();

        match self {
            Event::CacheHit(k) => {
                data.insert("cache_hit".to_string(), json!(k.to_string()));
            }
            Event::CacheMiss(k) => {
                data.insert("cache_miss".to_string(), json!(k.to_string()));
            }
            Event::CachePut(k) => {
                data.insert("cache_put".to_string(), json!(k.to_string()));
            }
        }

        data
    }
}

impl Telemetry {
    pub fn new(honeycomb_key: String) -> Self {
        let honeycomb_client = libhoney::init(libhoney::Config {
            options: libhoney::client::Options {
                api_key: honeycomb_key,
                dataset: "dag-cache".to_string(),
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options::default(),
        });

        // publishing requires &mut so just mutex-wrap it, lmao (FIXME)
        Telemetry(Mutex::new(honeycomb_client))
    }
}

impl TelemetryCapability for Telemetry {
    fn report(&self, event: Event) -> () {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut client = self.0.lock().unwrap();
        let mut ev = client.new_event();
        ev.add(event.into_data());
        let res = ev.send(&mut client); // todo check res? idk lmao (FIXME)
        println!("event send res: {:?}", res);
    }
}

// TODO: integrate with honeycomb trace system
// // note: probably needs to be sent in at somewhat-believable timestamps
// fn test<C: HasTelemetryCap + Sync + Send + 'static>(caps: Arc<C>, sid: u32, tid: u32, pid: Option<u32>) {
//     let mut data: HashMap<String, Value> = HashMap::new();
//     data.insert("name".to_string(), json!("dag-cache"));
//     data.insert("service_name".to_string(), json!("dag-cache-svc"));
//     // data.insert("duration_ms".to_string(), json!(1337));
//     data.insert("trace.span_id".to_string(), json!(sid));
//     data.insert("trace.trace_id".to_string(), json!(tid));
//     data.insert("trace.parent_id".to_string(), pid.map( |e| json!(e)).unwrap_or(json!(null)));
//     caps.report_telemetry(data);
// }
