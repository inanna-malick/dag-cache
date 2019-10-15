use crate::capabilities::telemetry::Telemetry;
use ::libhoney::{json, Value};
use chrono::{DateTime, Utc};
use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_core::span::Current;


// for tracking current span
thread_local! {
    static CURRENT_SPAN: RefCell<Vec<Id>> = RefCell::new(vec!());
}

struct SpanData {
    trace_id: u128,
    parent_id: Option<Id>,
    initialized_at: DateTime<Utc>,
    metadata: &'static Metadata<'static>,
    values: HashMap<String, Value>,
}

impl SpanData {
    fn into_values(self, id: Id) -> HashMap<String, Value> {
        let mut values = self.values;
        values.insert( // magic honeycomb string (trace.span_id)
            "trace.span_id".to_string(),
            json!(format!("span-{}", id.into_u64())),
        );
        values.insert( // magic honeycomb string (trace.trace_id)
            "trace.trace_id".to_string(),
            json!(format!("trace-{}", &self.trace_id)),
        );
        values.insert( // magic honeycomb string (trace.parent_id)
            "trace.parent_id".to_string(),
            self.parent_id
                .map(|pid| json!(format!("span-{}", pid.into_u64())))
                .unwrap_or(json!(null)),
        );

        values.insert(
            "level".to_string(),
            json!(format!("{}", self.metadata.level())),
        );

        values.insert(
            "timestamp".to_string(),
            json!(format!("{}", self.initialized_at.to_rfc3339())),
        );


        values.insert("name".to_string(), json!(self.metadata.name()));
        values.insert("target".to_string(), json!(self.metadata.target())); // not honeycomb-special but tracing-provided

        values.insert("service_name".to_string(), json!("dummy-svc".to_string()));


        values
    }
}

struct RefCt<T> {
    ref_ct: u64,
    inner: T,
}

impl<T> Deref for RefCt<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target { &self.inner }
}

impl<T> DerefMut for RefCt<T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}

// just clone values into telemetry-appropriate hash map
struct HoneycombVisitor<'a>(&'a mut HashMap<String, Value>);

impl<'a> Visit for HoneycombVisitor<'a> {
    // TODO: special visitors for various formats that honeycomb.io supports
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // todo: more granular, per-type, etc
        // TODO: mb don't store 1x field name per span, instead use fmt-style trick w/ field id's by reading metadata..
        let s = format!("{:?}", value); // using 'telemetry.' namespace to disambiguate from system-level names
        self.0.insert(format!("telemetry.{}", field.name()), json!(s));
    }
}

trait TelemetryThingy {
    // event or span atributes
    fn t_record(&self, visitor: &mut dyn Visit);
    fn t_metadata(&self) -> &'static Metadata<'static>;
    fn t_is_root(&self) -> bool;
    fn t_parent(&self) -> Option<&Id>;
}

impl<'a> TelemetryThingy for Attributes<'a> {
    fn t_record(&self, visitor: &mut dyn Visit) { self.record(visitor) }
    fn t_metadata(&self) -> &'static Metadata<'static> { self.metadata() }
    fn t_is_root(&self) -> bool { self.is_root() }
    fn t_parent(&self) -> Option<&Id> { self.parent() }
}

impl<'a> TelemetryThingy for Event<'a> {
    fn t_record(&self, visitor: &mut dyn Visit) { self.record(visitor) }
    fn t_metadata(&self) -> &'static Metadata<'static> { self.metadata() }
    fn t_is_root(&self) -> bool { self.is_root() }
    fn t_parent(&self) -> Option<&Id> { self.parent() }
}

pub struct TelemetrySubscriber {
    telem: Telemetry,
    // TODO: more optimal repr? is mutex bad in this path? idk, find out
    spans: Mutex<HashMap<Id, RefCt<SpanData>>>,
}

impl TelemetrySubscriber {
    pub fn new(telem: Telemetry) -> Self {
        TelemetrySubscriber {
            spans: Mutex::new(HashMap::new()),
            telem,
        }
    }

    fn peek_current_span(&self) -> Option<Id> {
        CURRENT_SPAN.with(|c| c.borrow().last().cloned())
    }
    fn pop_current_span(&self) -> Option<Id> {
        CURRENT_SPAN.with(|c| c.borrow_mut().pop())
    }
    fn push_current_span(&self, id: Id) {
        CURRENT_SPAN.with(|c| c.borrow_mut().push(id))
    }

    // get (trace_id, parent_id). will generate a new trace id if none are available
    fn build_span<T: TelemetryThingy>(&self, t: &T) -> (Id, SpanData) {
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
        t.t_record(&mut visitor);

        // succeed or die. failure is unrecoverable (mutex poisoned)
        let spans = self.spans.lock().unwrap();

        let (trace_id, parent_id) = if let Some(parent_id) = t.t_parent() {
            // explicit parent
            values.insert(
                "parent_source".to_string(),
                json!("explicit".to_string()),
            );
            // error if parent not in map (need to grab it to get trace id)
            let parent = &spans[&parent_id];
            (parent.trace_id, Some(parent_id.clone()))
        } else if t.t_is_root() {
            // don't bother checking thread local if span is explicitly root according to this fn
            values.insert(
                "parent_source".to_string(),
                json!("explicit_root".to_string()),
            );

            let trace_id = rand::thread_rng().gen();
            (trace_id, None)
        } else if let Some(parent_id) = self.peek_current_span() {
            // possible implicit parent from threadlocal ctx
            // TODO: check with, idk, eliza or similar (or run experiment) to see if this is correct
            values.insert(
                "parent_source".to_string(),
                json!("implicit_parent".to_string()),
            );

            let parent = &spans[&parent_id];
            (parent.trace_id, Some(parent_id))
        } else {
            // no parent span, thus this is a root span
            let trace_id = rand::thread_rng().gen();
            values.insert(
                "parent_source".to_string(),
                json!("no_parent".to_string()),
            );

            (trace_id, None)
        };

        (
            id,
            SpanData {
                initialized_at: now,
                metadata: t.t_metadata(),
                trace_id,
                parent_id,
                values,
            },
        )
    }
}


// TODO: concept: don't assign trace ids implicitly for new spans (no trace id for, eg, tokio noise).
// TODO: concept: trace ids generated at web framework edge _or_ passed in for multi-application traces
impl Subscriber for TelemetrySubscriber {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() >= &tracing::Level::INFO // simple impl so I can see my app logs as distinct from all the tokio noise
    }

    fn new_span(&self, span: &Attributes) -> Id {
        let (id, new_span) = self.build_span(span);

        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut spans = self.spans.lock().unwrap();

        // FIXME: what if span id already exists in map? should I handle? assume no overlap possible b/c random?
        // ASSERTION: there should be no collisions here
        // insert attributes from span into map
        spans.insert(
            id.clone(),
            RefCt {
                ref_ct: 1,
                inner: new_span,
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
    fn event(&self, event: &Event) {
        // report as span with zero-length interval
        let (id, new_span) = self.build_span(event);

        let values = new_span.into_values(id);

        self.telem.report_data(values);
    }

    fn enter(&self, span: &Id) {
        self.push_current_span(span.clone());
    }
    fn exit(&self, span: &Id) {
        // NOTE: assert popped span id == expected (provided span id)
        self.pop_current_span();
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
                    spans.remove(&id).map(|e| e.inner) // returns option already, no need for Some wrapper
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
            let elapsed = now.timestamp_subsec_millis() - dropped.initialized_at.timestamp_subsec_millis();

            let mut values = dropped.into_values(id);

            values.insert("duration_ms".to_string(), json!(elapsed)); // NOTE: is fucked

            self.telem.report_data(values);
            true
        } else {
            false
        }
    }

    fn current_span(&self) -> Current {
        if let Some(id) = self.peek_current_span() {
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
