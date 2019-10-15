use crate::capabilities::{Event, TelemetryCapability};
use ::libhoney::{json, Value};
use chrono::{DateTime, Utc};
use libhoney::FieldHolder;
use std::collections::HashMap;
use std::sync::Mutex;

use std::{thread, time};

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

// // encoded as base58, trace.trace_id
// struct TraceId(u128);
// // encoded as base58, trace.span_id
// struct SpanId(u128);

// // parent id is implicit (look up last element, take fst), trace is at top level and should be outside of system
// // let stack: Vec<(SpanId, Span)>

// struct Span {
//     // parent_id: u128, // trace.parent_id, implicit from stack order
//     duration_ms: u64,
//     name: String, // TODO: make this a type level enum - can also move
//                   //TODO: associated data, somehow
//                   // service_name: String, constant 'dag-cache'
// }

// // goal: trace via bracket pattern, eg entire body of function in `into(span)`
// // fn into(span: Span) -> impl FnOnce(TelemetryCtx) -> impl futures::Future<Output = usize> {
// async fn into<A, F: futures::Future<Output = A>, Fn: FnOnce(TelemetryCtx) -> F>(
//     ctx: TelemetryCtx,
//     interval: Interval,
//     f: Fn,
// ) -> A {
//     let ctx = ctx; // push span onto ctx here, also generate start timestamp

//     let res = f(ctx).await;

//     // get final timestamp, push span to telemetry collector

//     // rain says: look into moxie by adam perry

//     res
// }

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
        let res = Telemetry(Mutex::new(honeycomb_client));

        // WORKING: https://ui.honeycomb.io/lagrange-5-heavy-industries/datasets/dag-cache/trace/fc8DWCtEwz6
        // next todo: figure out how to interface with tracing _or_ just build capabilities into telemetry system? DIY (async-first) is probably easier than building my own tracing subscriber
        let tid1 = "1213461";
        let sid1 = "2321354";
        let sid2 = "4562623";
        let sid3 = "6781234";
        let one_second = time::Duration::from_millis(1000);

        test(&res, "outer1", 5000, sid1, tid1, None);
        thread::sleep(one_second);
        test(&res, "inner1", 3000, sid2, tid1, Some(sid1));
        thread::sleep(one_second);
        test(&res, "inner2", 1500, sid3, tid1, Some(sid1));
        thread::sleep(one_second);

        res
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

fn test(
    caps: &Telemetry,
    name: &str,
    ms: u64,
    sid: &str,         // span id <- TODO: base 58 lmao hell yeah b58 gang
    tid: &str,         // trace id
    pid: Option<&str>, // parent id
) {
    // span as stored in stack on capabilities (cloned anyway for each trace-equivalent)
    // same span can be parent of multiple concurrent actions, each of which can step into new spans
    // immutable data, yay

    let mut data: HashMap<String, Value> = HashMap::new();
    data.insert("name".to_string(), json!(name));
    data.insert("service_name".to_string(), json!("dag-cache-svc"));
    data.insert("duration_ms".to_string(), json!(ms));
    data.insert("trace.span_id".to_string(), json!(sid));
    data.insert("trace.trace_id".to_string(), json!(tid));
    data.insert(
        "trace.parent_id".to_string(),
        pid.map(|e| json!(e)).unwrap_or(json!(null)),
    );
    caps.report_data(data);
}
