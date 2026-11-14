//! Common operational log envelope; event-specific fields belong to `fields`.

use serde::Serialize;
use serde_json::{Map, Value};
use std::fmt;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{
        FmtContext, FormatEvent, FormatFields,
        format::Writer,
        time::{FormatTime, SystemTime},
    },
    registry::LookupSpan,
};

#[derive(Serialize)]
struct Record<'a> {
    timestamp: String,
    level: &'a str,
    target: &'a str,
    message: String,
    fields: Map<String, Value>,
}

/// JSON formatter with a fixed envelope and free event fields in `fields`.
pub struct JsonLog;

impl<S, N> FormatEvent<S, N> for JsonLog
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = match fields.0.remove("message") {
            Some(Value::String(message)) => message,
            Some(value) => value.to_string(),
            None => String::new(),
        };
        let mut timestamp = String::new();
        SystemTime.format_time(&mut Writer::new(&mut timestamp))?;
        let record = Record {
            timestamp,
            level: event.metadata().level().as_str(),
            target: event.metadata().target(),
            message,
            fields: fields.0,
        };
        let json = serde_json::to_string(&record).map_err(|_| fmt::Error)?;
        writeln!(writer, "{json}")
    }
}

#[derive(Default)]
struct Fields(Map<String, Value>);

impl Fields {
    fn insert(&mut self, field: &Field, value: Value) {
        self.0.insert(field.name().to_owned(), value);
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value.into());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert(field, value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, value.into());
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        self.insert(field, value.into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field, format!("{value:?}").into());
    }
}
