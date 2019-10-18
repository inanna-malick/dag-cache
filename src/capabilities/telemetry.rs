use crate::capabilities::{Event, TelemetryCapability};
use ::libhoney::{json, Value};
use chrono::{DateTime, Utc};
use libhoney::FieldHolder;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Telemetry(Mutex<libhoney::Client<libhoney::transmission::Transmission>>);

// impl here b/c it's specific to libhoney/not a generic capability thing
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

impl Telemetry {
    pub fn report_data(&self, data: HashMap<String, Value>) -> () {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut client = self.0.lock().unwrap();
        let mut ev = client.new_event();
        ev.add(data);
        let res = ev.send(&mut client); // todo check res? (FIXME)
        println!("event send res: {:?}", res);
    }
}

impl TelemetryCapability for Telemetry {
    fn report(&self, event: Event) -> () {
        let mut data = event.into_data();
        let now: DateTime<Utc> = Utc::now();
        data.insert(
            "timestamp".to_string(),
            json!(format!("{}", now.to_rfc3339())),
        );

        self.report_data(data)
    }
}
