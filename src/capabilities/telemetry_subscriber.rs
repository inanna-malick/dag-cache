use crate::capabilities::telemetry::Telemetry;
use ::libhoney::{json, Value};
use chashmap::CHashMap;
use chrono::{DateTime, Utc};
use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_core::span::Current;


// TODO/FIXME: failing due to (likely) some kind of deadlock bs. not sure how to fix. mb keep tracing ids outside main data structure?

// for tracking current span
thread_local! {
    static CURRENT_SPAN: RefCell<Vec<Id>> = RefCell::new(vec!());
}

struct SpanData {
    trace_id: Option<String>, // option used to impl lazy eval
    parent_id: Option<Id>,
    initialized_at: DateTime<Utc>,
    metadata: &'static Metadata<'static>,
    values: HashMap<String, Value>,
}

impl SpanData {
    fn into_values(self, trace_id: String, id: Id) -> HashMap<String, Value> {
        let mut values = self.values;
        values.insert(
            // magic honeycomb string (trace.span_id)
            "trace.span_id".to_string(),
            json!(format!("span-{}", id.into_u64())),
        );
        values.insert(
            // magic honeycomb string (trace.trace_id)
            "trace.trace_id".to_string(),
            // using explicit trace id passed in from ctx (req'd for lazy eval)
            json!(trace_id),
        );
        values.insert(
            // magic honeycomb string (trace.parent_id)
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
            json!(self.initialized_at.to_rfc3339()),
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
        self.0
            .insert(format!("telemetry.{}", field.name()), json!(s));
    }
}

pub struct TelemetrySubscriber {
    telem: Telemetry,
    // TODO: more optimal repr? is mutex bad in this path? idk, find out
    spans: CHashMap<Id, RefCt<SpanData>>,
}

fn gen_trace_id() -> String {
    let trace_id: u128 = rand::thread_rng().gen();
    format!("trace-{}", trace_id)
}

impl TelemetrySubscriber {
    pub fn new(telem: Telemetry) -> Self {
        TelemetrySubscriber {
            spans: CHashMap::new(),
            telem,
        }
    }

    // FIXME: site of future stack overflow (maybe)
    // can't think of a better way to manage ownership - want to retain mut borrow over all spans in path while traversing
    /// this function provides lazy initialization of trace ids (only generated when req'd to observe honeycomb event/span)
    fn get_or_gen_trace_id(&self, span: &mut SpanData) -> String {
        match &span.trace_id {
            Some(tid) => tid.clone(),
            None => {
                let new_trace_id = match &span.parent_id {
                    Some(parent_id) => {
                        // recurse
                        match self.spans.get_mut(parent_id) {
                            Some(mut span) => self.get_or_gen_trace_id(&mut span),
                            None => {
                                println!("didn't expect span parent lookup to fail, gen trace id for parent span with id {:?}", &parent_id);
                                // FIXME - maybe use a specific placeholder value for trace_id to track this case?
                                gen_trace_id()
                            }
                        }
                    }
                    None => {
                        // found root span with no trace id, generate trace_id
                        gen_trace_id()
                    }
                };
                span.trace_id = Some(new_trace_id.clone());
                new_trace_id
            }
        }
    }

    fn peek_current_span(&self) -> Option<Id> { CURRENT_SPAN.with(|c| c.borrow().last().cloned()) }
    fn pop_current_span(&self) -> Option<Id> { CURRENT_SPAN.with(|c| c.borrow_mut().pop()) }
    fn push_current_span(&self, id: Id) { CURRENT_SPAN.with(|c| c.borrow_mut().push(id)) }

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

        let parent_id = if let Some(parent_id) = t.t_parent() {
            // explicit parent
            Some(parent_id.clone())
        } else if t.t_is_root() {
            // don't bother checking thread local if span is explicitly root according to this fn
            None
        } else if let Some(parent_id) = self.peek_current_span() {
            // implicit parent from threadlocal ctx
            Some(parent_id)
        } else {
            // no parent span, thus this is a root span
            None
        };

        println!(
            "enter span (or 0-width event span) with  name {}",
            t.t_metadata().name()
        );

        (
            id,
            SpanData {
                initialized_at: now,
                metadata: t.t_metadata(),
                trace_id: None, // always lazy
                parent_id,
                values,
            },
        )
    }
}

// TODO: concept: don't assign trace ids implicitly for new spans (no trace id for, eg, tokio noise).
// TODO: concept: trace ids generated at web framework edge _or_ passed in for multi-application traces
impl Subscriber for TelemetrySubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() == &tracing::Level::INFO
            || metadata.level() == &tracing::Level::WARN
            || metadata.level() == &tracing::Level::ERROR
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let (id, new_span) = self.build_span(span);

        println!("initialize span with id {:?}", &id);

        // FIXME: what if span id already exists in map? should I handle? assume no overlap possible b/c random?
        // ASSERTION: there should be no collisions here
        // insert attributes from span into map
        self.spans.insert(
            id.clone(),
            RefCt {
                ref_ct: 1,
                inner: new_span,
            },
        );

        id
    }

    // record additional values on span map
    fn record(&self, span: &Id, values: &Record<'_>) {
        if let Some(mut span_data) = self.spans.get_mut(&span) {
            values.record(&mut HoneycombVisitor(&mut span_data.values)); // record any new values
        }
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    // record event (publish directly to telemetry, not a span)
    fn event(&self, event: &Event<'_>) {
        // report as span with zero-length interval
        let (span_id, mut new_span) = self.build_span(event);

        let trace_id = self.get_or_gen_trace_id(&mut new_span);
        let values = new_span.into_values(trace_id, span_id);

        self.telem.report_data(values);
    }

    fn enter(&self, span: &Id) { self.push_current_span(span.clone()); }
    fn exit(&self, _span: &Id) { self.pop_current_span(); }

    fn clone_span(&self, id: &Id) -> Id {
        // ref count ++
        // should always be present
        println!("clone span with id {:?}", &id);
        if let Some(mut span_data) = self.spans.get_mut(id) {
            span_data.ref_ct += 1; // increment ref ct
        }
        id.clone() // type sig of this function seems to compel cloning of id (&X -> X)
    }

    fn try_close(&self, id: Id) -> bool {
        println!("try close for span with id {:?}", &id);

        let dropped_span: Option<SpanData> = {
            if let Some(mut span_data) = self.spans.get_mut(&id) {
                span_data.ref_ct -= 1; // decrement ref ct

                if span_data.ref_ct == 0 {
                    println!("drop span with id {:?}", &id);
                    self.spans.remove(&id).map(|e| e.inner) // returns option already, no need for Some wrapper
                } else {
                    println!("span with id {:?} has {} remaining refs, not removing", &id, span_data.ref_ct);
                    None
                }
            } else {
                None
            }
        };

        if let Some(mut dropped) = dropped_span {
            // debug logging, not using tracing structured log b/c reentrant
            println!(
                "zero outstanding refs to span w/ id {:?}, sending to honeycomb",
                &id
            );

            let now = Utc::now();
            let elapsed =
                now.timestamp_subsec_millis() - dropped.initialized_at.timestamp_subsec_millis();

            let trace_id = self.get_or_gen_trace_id(&mut dropped);
            let mut values = dropped.into_values(trace_id, id);

            values.insert("duration_ms".to_string(), json!(elapsed));

            self.telem.report_data(values);
            true
        } else {
            false
        }
    }

    fn current_span(&self) -> Current {
        if let Some(id) = self.peek_current_span() {
            if let Some(meta) = self.spans.get(&id).map(|span| span.metadata) {
                return Current::new(id, meta);
            }
        }
        Current::none()
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
