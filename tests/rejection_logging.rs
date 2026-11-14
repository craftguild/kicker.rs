mod common;

use axum::{body::to_bytes, http::StatusCode};
use common::*;
use kicker::{
    config::Store,
    model::ServerConfig,
    provider::{
        Providers, SetupUi, VerificationOutcome, VerifyError, WebhookProvider, WebhookRequest,
    },
    queue::Dispatcher,
    server,
};
use serde_json::Value;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn failures(captured: &Arc<Mutex<Vec<u8>>>) -> Vec<Value> {
    std::str::from_utf8(&captured.lock().unwrap())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|record| record["message"] == "webhook verification failed")
        .collect()
}

struct Rejecting(Option<CancellationToken>);

impl WebhookProvider for Rejecting {
    fn kind(&self) -> &'static str {
        "example"
    }

    fn configure(&self, _: &mut dyn SetupUi) -> anyhow::Result<toml::Table> {
        Ok(toml::Table::new())
    }

    fn validate_config(&self, _: &toml::Table) -> anyhow::Result<()> {
        Ok(())
    }

    fn verify(
        &self,
        _: &toml::Table,
        _: &WebhookRequest,
    ) -> Result<VerificationOutcome, VerifyError> {
        if let Some(closing) = &self.0 {
            closing.cancel();
        }
        Err(VerifyError::new("example_policy_denied"))
    }
}

// One global subscriber in this integration-test process captures spawn_blocking
// events without adding subscriber propagation to the product request path.
#[tokio::test]
async fn provider_failures_log_once_with_safe_fields_and_keep_responses_generic() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = captured.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .event_format(kicker::logging::JsonLog)
            .with_env_filter("kicker=info")
            .with_writer(move || Capture(writer.clone()))
            .finish(),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "touch should-not-run");
    store.add(hook.clone()).unwrap();
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    let app = server::router(store, queue.clone(), 1024).unwrap();
    let raw = br#"{"private":"PAYLOAD-DO-NOT-LOG"}"#;
    let mut missing = request(hook.id, raw, "push");
    missing.headers_mut().remove("x-github-delivery");
    let mut invalid = request(hook.id, raw, "push");
    invalid
        .headers_mut()
        .insert("x-github-delivery", "HEADER-DO-NOT-LOG".parse().unwrap());
    let malformed = request(hook.id, b"JSON-DO-NOT-LOG", "push");
    for (mut request, reason, field) in [
        (missing, "missing_header", Some("x-github-delivery")),
        (invalid, "invalid_header", Some("x-github-delivery")),
        (malformed, "invalid_json", None),
    ] {
        *request.uri_mut() = format!("/{}?private=URI-DO-NOT-LOG", hook.id)
            .parse()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", "AUTH-DO-NOT-LOG".parse().unwrap());
        let before = failures(&captured).len();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(queue.counts(), (0, 0));
        let records = failures(&captured);
        assert_eq!(records.len(), before + 1);
        let record = records.last().unwrap();
        assert_eq!(record["level"], "WARN");
        assert_eq!(record["fields"]["hook_id"], hook.id.to_string());
        assert_eq!(record["fields"]["provider"], "github");
        assert_eq!(record["fields"]["reason"], reason);
        assert_eq!(record["fields"].get("field").and_then(Value::as_str), field);
    }

    let before = failures(&captured).len();
    for (event, status) in [("ping", StatusCode::OK), ("push", StatusCode::ACCEPTED)] {
        assert_eq!(
            app.clone()
                .oneshot(request(hook.id, raw, event))
                .await
                .unwrap()
                .status(),
            status
        );
    }
    assert_eq!(failures(&captured).len(), before);
    queue.cancel();

    for cancel_during_verification in [false, true] {
        let closing = CancellationToken::new();
        let provider = Rejecting(cancel_during_verification.then(|| closing.clone()));
        let providers = Providers::new(vec![Arc::new(provider)]).unwrap();
        let store = Store::new(
            root.path()
                .join(format!("custom-{cancel_during_verification}")),
            providers,
        );
        let mut custom = common::hook(root.path(), "touch should-not-run");
        custom.kind = "example".into();
        custom.verification = toml::Table::new();
        store.add(custom.clone()).unwrap();
        let settings = ServerConfig::default();
        let queue = Dispatcher::new(settings.clone()).unwrap();
        let app = server::router_with_config(store, queue.clone(), &settings, closing).unwrap();
        let before = failures(&captured).len();
        let response = app.oneshot(request(custom.id, raw, "push")).await.unwrap();
        assert_eq!(
            response.status(),
            if cancel_during_verification {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(queue.counts(), (0, 0));
        let records = failures(&captured);
        assert_eq!(records.len(), before + 1);
        let record = records.last().unwrap();
        assert_eq!(record["level"], "WARN");
        assert_eq!(record["fields"]["provider"], "example");
        assert_eq!(record["fields"]["hook_id"], custom.id.to_string());
        assert_eq!(record["fields"]["reason"], "example_policy_denied");
        assert!(record["fields"].get("field").is_none());
    }

    assert!(!root.path().join("should-not-run").exists());
    let log = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let transitions: Vec<_> = records
        .iter()
        .filter(|record| record["message"] == "job transition")
        .collect();
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0]["fields"]["state"], "queued");
    assert_eq!(transitions[0]["fields"]["attempt"], 0);
    assert!(transitions[0]["fields"]["previous_state"].is_null());
    assert_eq!(transitions[1]["fields"]["state"], "cancelled");
    assert_eq!(transitions[1]["fields"]["previous_state"], "queued");
    assert_eq!(
        transitions[0]["fields"]["job_id"],
        transitions[1]["fields"]["job_id"]
    );
    for record in &records {
        assert_eq!(
            record.as_object().unwrap().keys().collect::<Vec<_>>(),
            records[0].as_object().unwrap().keys().collect::<Vec<_>>()
        );
        assert!(record["fields"].is_object());
    }
    for transition in transitions {
        assert_eq!(transition["level"], "INFO");
        assert_eq!(transition["fields"]["hook_id"], hook.id.to_string());
        assert_eq!(transition["fields"]["provider"], "github");
        assert_eq!(transition["fields"]["delivery_id"], DELIVERY);
        uuid::Uuid::parse_str(transition["fields"]["transition_id"].as_str().unwrap()).unwrap();
    }
    for private in [
        SECRET.to_owned(),
        signature(raw),
        signature(b"JSON-DO-NOT-LOG"),
        "PAYLOAD-DO-NOT-LOG".into(),
        "HEADER-DO-NOT-LOG".into(),
        "JSON-DO-NOT-LOG".into(),
        "URI-DO-NOT-LOG".into(),
        "AUTH-DO-NOT-LOG".into(),
        "touch should-not-run".into(),
    ] {
        assert!(!log.contains(&private), "private input appeared in logs");
    }
}
