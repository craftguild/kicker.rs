use super::severity_writer;
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
fn severity_routes_each_event_once_without_changing_json_fields() {
    let stdout = Arc::new(Mutex::new(Vec::<u8>::new()));
    let stderr = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (out_writer, err_writer) = (stdout.clone(), stderr.clone());
    let subscriber = tracing_subscriber::fmt()
        .json()
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
        assert_eq!(event["fields"]["message"], "stream check");
        assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
    }
}
