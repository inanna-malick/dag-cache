use crate::capabilities::telemetry::Telemetry;
use ::libhoney::{json, Value};
use chrono::{DateTime, Utc};
use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_core::span::Current;

// for tracking current span
thread_local! {
    static CURRENT_SPAN: RefCell<Option<Id>> = RefCell::new(None);
}

struct SpanData {
    ref_ct: u64,
    trace_id: u128,
    parent_id: Option<Id>,
    initialized_at: DateTime<Utc>,
    metadata: &'static Metadata<'static>,
    values: HashMap<String, Value>,
}

// just clone values into telemetry-appropriate hash map
struct HoneycombVisitor<'a>(&'a mut HashMap<String, Value>);

impl<'a> Visit for HoneycombVisitor<'a> {
    // TODO: special visitors for various formats that honeycomb.io supports
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // todo: more granular, per-type, etc
        // TODO: mb don't store 1x field name per span, instead use fmt-style trick w/ field id's by reading metadata..
        let s = format!("telemetry.{:?}", value); // using 'telemetry.' namespace to disambiguate from system-level names
        self.0.insert(field.name().to_string(), json!(s));
    }
}

pub struct TelemetrySubscriber {
    telem: Telemetry,
    // TODO: more optimal repr? is mutex bad in this path? idk, find out
    spans: Mutex<HashMap<Id, SpanData>>,
}

impl TelemetrySubscriber {
    fn get_current_span_raw(&self) -> Option<Id> { CURRENT_SPAN.with(|c| c.borrow().clone()) }
    fn set_current_span_raw(&self, cid: Option<Id>) { CURRENT_SPAN.with(|c| *c.borrow_mut() = cid) }

}

impl Subscriber for TelemetrySubscriber {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // not impl'd - site for future optimizations (eg log lvl filtering, etc)
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

        values.insert("name".to_string(), json!(span.metadata().name())); // honeycomb-special and also tracing-provided
        values.insert("target".to_string(), json!(span.metadata().target())); // not honeycomb-special but tracing-provided
        values.insert("service_name".to_string(), json!("dummy-svc".to_string()));

        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut spans = self.spans.lock().unwrap();

        // todo: use is_root() to see if trace root (if true.. idk, don't look at parent()? - no, look at on parent() = None)
        // todo: can get parent from span.parent() (optional, false if root _or_ if threadlocal current span should be parent)
        let (trace_id, parent_id) = if let Some(parent_id) = span.parent() {
            // explicit parent
            // error if parent not in map (need to grab it to get trace id)
            let parent = &spans[&parent_id];
            (parent.trace_id, Some(parent_id.clone()))
        } else if span.is_root() {
            // don't bother checking thread local if span is explicitly root according to this fn
            let trace_id = rand::thread_rng().gen();
            (trace_id, None)
        } else if let Some(parent_id) = self.get_current_span_raw() {
            // possible implicit parent from threadlocal ctx
            // TODO: check with, idk, eliza or similar (or run experiment) to see if this is correct
            let parent = &spans[&parent_id];
            (parent.trace_id, Some(parent_id))
        } else {
            // no parent span, thus this is a root span
            let trace_id = rand::thread_rng().gen();
            (trace_id, None)
        };

        // FIXME: what if span id already exists in map? should I handle? assume no overlap possible b/c random?
        // ASSERTION: there should be no collisions here
        // insert attributes from span into map
        spans.insert(
            id.clone(),
            SpanData {
                ref_ct: 1,
                initialized_at: now,
                metadata: span.metadata(),
                trace_id,
                parent_id,
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
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {
        // no-op for now, iirc honeycomb doesn't support this yet
    }

    // record event (publish directly to telemetry, not a span)
    fn event(&self, event: &Event) {}

    // used to maintain current span threadlocal, probably
    fn enter(&self, span: &Id) {
        self.set_current_span_raw(Some(span.clone())); // just set current to that
    }
    fn exit(&self, span: &Id) {
        // NOTE: don't bother looking at old current span id, just overwrite via lookup (todo: keep stack?)

        // succeed or die. failure is unrecoverable (mutex poisoned)
        let spans = self.spans.lock().unwrap();
        let parent_id = spans[span].parent_id.clone(); // EXPECTATION: span is always in map when exiting

        self.set_current_span_raw(parent_id); // set current span to parent of span we just exited (TODO: check if req'd)
    }

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

            let now = Utc::now();
            let elapsed = now.timestamp() - dropped.initialized_at.timestamp();

            //todo: add metadata eg timestamp, etc
            let mut values = dropped.values;

            values.insert(
                "trace.span_id".to_string(),
                json!(format!("span-{}", id.into_u64())),
            );
            values.insert(
                "trace.trace_id".to_string(),
                json!(format!("trace-{}", &dropped.trace_id)),
            );
            values.insert(
                "trace.parent_id".to_string(),
                dropped
                    .parent_id
                    .map(|pid| json!(format!("span-{}", pid.into_u64())))
                    .unwrap_or(json!(null)),
            );

            values.insert("duration_ms".to_string(), json!(elapsed));

            values.insert(
                "timestamp".to_string(),
                json!(format!("{}", dropped.initialized_at.to_rfc3339())),
            );

            self.telem.report_data(values);
            true
        } else {
            false
        }
    }

    fn current_span(&self) -> Current {
        if let Some(id) = self.get_current_span_raw() {
            // succeed or die. failure is unrecoverable (mutex poisoned) (TODO: learn better patterns: less mutexes)
            let spans = self.spans.lock().unwrap();
            if let Some(meta) = spans.get(&id).map(|span| span.metadata) {
                return Current::new(id, meta);
            }
        }
        Current::none()
    }
}

// fn test(
//     caps: &Telemetry,
//     name: &str,
//     ms: u64,
//     sid: &str,         // span id <- TODO: base 58 lmao hell yeah b58 gang
//     tid: &str,         // trace id
//     pid: Option<&str>, // parent id
// ) {
//     // span as stored in stack on capabilities (cloned anyway for each trace-equivalent)
//     // same span can be parent of multiple concurrent actions, each of which can step into new spans
//     // immutable data, yay

//     let mut data: HashMap<String, Value> = HashMap::new();
//     data.insert("name".to_string(), json!(name));
//     data.insert("service_name".to_string(), json!("dag-cache-svc"));
//     data.insert("duration_ms".to_string(), json!(ms));
//     data.insert("trace.span_id".to_string(), json!(sid));
//     data.insert("trace.trace_id".to_string(), json!(tid));
//     data.insert(
//         "trace.parent_id".to_string(),
//         pid.map(|e| json!(e)).unwrap_or(json!(null)),
//     );
//     caps.report_data(data);
// }
