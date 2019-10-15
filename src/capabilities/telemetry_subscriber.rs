use crate::capabilities::telemetry::Telemetry;
use ::libhoney::{json, Value};
use chrono::{DateTime, Utc};
use libhoney::FieldHolder;
use rand::Rng;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::{thread, time};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

struct SpanData {
    ref_ct: u64,
    initialized_at: DateTime<Utc>,
    values: HashMap<String, Value>,
}

pub struct TelemetrySubscriber {
    telem: Telemetry,
    spans: Mutex<HashMap<Id, SpanData>>, // TODO: more optimal repr
}

// just clone values into telemetry-appropriate hash map
struct HoneycombVisitor<'a>(&'a mut HashMap<String, Value>);

impl<'a> Visit for HoneycombVisitor<'a> {
    // TODO: special visitors for various formats that honeycomb.io supports
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // todo: more granular, per-type, etc
        let s = format!("{:?}", value); // temporary impl, probably don't need format here
                                        // TODO: mb don't store 1x field name per span, instead use fmt-style trick w/ field id's by reading metadata..
        self.0.insert(field.name().to_string(), json!(s));
    }
}

impl Subscriber for TelemetrySubscriber {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // not impl'd - site for future optimizations
        true
    }

    fn new_span(&self, span: &Attributes) -> Id {
        let now = Utc::now();
        // TODO: random local u32 + counter or similar(?) to provide unique Id's
        let mut u: u64 = 0;
        while u == 0 {
            // random gen until != 0 (disallowed)
            u = rand::thread_rng().gen();
        } // random u64 != 0 required
        let id = Id::from_u64(u);

        let mut values = HashMap::new();
        let mut visitor = HoneycombVisitor(&mut values);
        span.record(&mut visitor);

        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut spans = self.spans.lock().unwrap();
        // FIXME: what if span id already exists in map? should I handle? assume no overlap possible b/c random?
        // ASSERTION: there should be no collisions here
        // insert attributes from span into map
        spans.insert(
            id.clone(),
            SpanData {
                ref_ct: 1,
                initialized_at: now,
                values,
            },
        );

        id
    }

    // record additional values on span map
    fn record(&self, span: &Id, values: &Record) {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut spans = self.spans.lock().unwrap();
        if let Some(span_data) = spans.get_mut(&span) {
            values.record(&mut HoneycombVisitor(&mut span_data.values)); // record any new values
        }
    }
    fn record_follows_from(&self, span: &Id, follows: &Id) {
        // no-op for now, iirc honeycomb doesn't support this yet
    }

    // record event (publish directly to telemetry, not a span)
    fn event(&self, event: &Event) {}

    // used to maintain current span threadlocal, probably
    fn enter(&self, span: &Id) {}
    fn exit(&self, span: &Id) {}

    // fn register_callsite( // not impl'd - site for future optimizations
    //     &self,
    //     metadata: &'static Metadata<'static>
    // ) -> Interest { ... }
    fn clone_span(&self, id: &Id) -> Id {
        // ref count ++
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut spans = self.spans.lock().unwrap();
        // should always be present
        if let Some(span_data) = spans.get_mut(id) {
            span_data.ref_ct = span_data.ref_ct + 1; // increment ref ct
        }
        id.clone() // type sig of this function seems to compel cloning of id (&X -> X)
    }

    fn try_close(&self, id: Id) -> bool {
        let dropped_span: Option<SpanData> = {
            // succeed or die. failure is unrecoverable (mutex poisoned)
            // FIXME FIXME FIXME
            // FIXME FIXME FIXME: not unwind safe, should NOT panic here
            // FIXME FIXME FIXME
            let mut spans = self.spans.lock().unwrap();
            if let Some(span_data) = spans.get_mut(&id) {
                span_data.ref_ct = span_data.ref_ct - 1; // decrement ref ct

                if span_data.ref_ct <= 0 {
                    spans.remove(&id) // returns option already, no need for Some wrapper
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(dropped) = dropped_span {
            println!(
                "zero outstanding refs to span w/ id {:?}, sending to honeycomb",
                &id
            );
            //todo: add metadata eg timestamp, etc
            self.telem.report_data(dropped.values);
            true
        } else {
            false
        }
    }
    // NOTE: this is probably going to be req'd for any serious usage (threadlocal, ugh)
    //       but it might be a good idea to get things working w/o reference to it for v1
    //       manual passing around of parent span id and etc
    // fn current_span(&self) -> Current { ... }
    // unsafe fn downcast_raw(&self, id: TypeId) -> Option<*const ()> { ... } // wtf is this? probably (certainly) won't impl
}

// encoded as base58, trace.trace_id
struct TraceId(u128);
// encoded as base58, trace.span_id
struct SpanId(u64);

// parent id is implicit (look up last element, take fst), trace is at top level and should be outside of system
// let stack: Vec<(SpanId, Span)>

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
