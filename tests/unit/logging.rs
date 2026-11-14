use super::severity_writer;
use kicker::logging::JsonLog;
use serde_json::Value;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn severity_routes_each_event_once_with_the_common_envelope() {
    let stdout = Arc::new(Mutex::new(Vec::<u8>::new()));
    let stderr = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (out_writer, err_writer) = (stdout.clone(), stderr.clone());
    let subscriber = tracing_subscriber::fmt()
        .event_format(JsonLog)
        .with_writer(severity_writer(
            move || Capture(out_writer.clone()),
            move || Capture(err_writer.clone()),
        ))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::trace!(marker = "trace", "stream check");
        tracing::debug!(marker = "debug", "stream check");
        tracing::info!(marker = "info", "stream check");
        tracing::warn!(marker = "warn", "stream check");
        tracing::error!(marker = "error", "stream check");
    });
    let decode = |stream: &Arc<Mutex<Vec<u8>>>| -> Vec<Value> {
        let bytes = stream.lock().unwrap();
        std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    let out = decode(&stdout);
    let err = decode(&stderr);
    assert_eq!(
        out.iter()
            .map(|event| event["level"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["TRACE", "DEBUG", "INFO", "WARN"]
    );
    assert_eq!(err.len(), 1);
    assert_eq!(err[0]["level"], "ERROR");
    assert_eq!(err[0]["fields"]["marker"], "error");
    for event in out.iter().chain(&err) {
        assert_eq!(event["message"], "stream check");
        assert_eq!(event.as_object().unwrap().len(), 5);
        assert!(event["target"].is_string());
        assert!(event["fields"].is_object());
        assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
    }
}

#[test]
fn free_fields_preserve_types_and_escaping_without_changing_the_envelope() {
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .event_format(JsonLog)
        .with_writer(move || Capture(writer.clone()))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("no extra fields");
        tracing::warn!(
            count = 42_u64,
            exit_code = -7_i64,
            elapsed = 0.5_f64,
            retry = true,
            reason = Some("missing_header"),
            absent = None::<&str>,
            detail = "line one\n\"日本語\"\\line two",
            display = %"display value",
            debug = ?Some(3),
            level = "event-specific level",
            "failed {}\n{}", "check", "next line"
        );
        tracing::info!(count = 1_u64);
    });
    let bytes = captured.lock().unwrap();
    let lines: Vec<_> = std::str::from_utf8(&bytes).unwrap().lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "embedded newlines must stay in one JSON record"
    );
    let records: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for record in &records {
        assert_eq!(
            record
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["fields", "level", "message", "target", "timestamp"]
        );
        assert!(record["message"].is_string());
    }
    assert_eq!(records[0]["message"], "no extra fields");
    assert_eq!(records[0]["fields"], serde_json::json!({}));
    assert_eq!(records[1]["level"], "WARN");
    assert_eq!(records[1]["message"], "failed check\nnext line");
    assert_eq!(
        records[1]["fields"],
        serde_json::json!({
            "count": 42,
            "exit_code": -7,
            "elapsed": 0.5,
            "retry": true,
            "reason": "missing_header",
            "detail": "line one\n\"日本語\"\\line two",
            "display": "display value",
            "debug": "Some(3)",
            "level": "event-specific level"
        })
    );
    assert_eq!(records[2]["message"], "");
    assert_eq!(records[2]["fields"]["count"], 1);
}
