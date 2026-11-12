mod common;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use common::*;
use kicker::{
    dispatch::{self, Failure, Outcome},
    model::{Secret, Target},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn shell_receives_exact_stdin_metadata_and_cwd_without_payload_interpolation() {
    let root = tempfile::tempdir().unwrap();
    let hook = hook(
        root.path(),
        "cat > payload; printf '%s' \"$KICKER_EVENT\" > event; printf '%s' \"$KICKER_DELIVERY_ID\" > delivery",
    );
    let raw = br#"{ "text": "$(touch injected); ' ; > anything" }
"#;
    let job = job(hook, raw);
    assert_eq!(
        dispatch::execute(
            &job,
            &dispatch::http_client().unwrap(),
            &CancellationToken::new()
        )
        .await,
        Outcome::Succeeded
    );
    assert_eq!(std::fs::read(root.path().join("payload")).unwrap(), raw);
    assert_eq!(
        std::fs::read_to_string(root.path().join("event")).unwrap(),
        "push"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("delivery")).unwrap(),
        DELIVERY
    );
    assert!(!root.path().join("injected").exists());
    assert!(!root.path().join("anything").exists());
    let pipeline = common::job(
        common::hook(root.path(), "cat | tr a-z A-Z > upper"),
        b"hello",
    );
    assert_eq!(
        dispatch::execute(
            &pipeline,
            &dispatch::http_client().unwrap(),
            &CancellationToken::new()
        )
        .await,
        Outcome::Succeeded
    );
    assert_eq!(std::fs::read(root.path().join("upper")).unwrap(), b"HELLO");
}

#[tokio::test]
async fn http_posts_exact_body_auth_and_metadata_without_forwarding_signature_or_following_redirects()
 {
    type Seen = Arc<Mutex<Vec<(HeaderMap, Bytes)>>>;
    async fn receive(State(seen): State<Seen>, headers: HeaderMap, body: Bytes) -> StatusCode {
        seen.lock().unwrap().push((headers, body));
        StatusCode::NO_CONTENT
    }
    let seen: Seen = Arc::default();
    let app = Router::new()
        .route("/target", post(receive))
        .route(
            "/redirect",
            post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/target")]) }),
        )
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(axum::serve(listener, app).into_future());
    let root = tempfile::tempdir().unwrap();
    let mut config = hook(root.path(), "true");
    config.target = Target::Http {
        url: format!("{base}/target"),
        headers: BTreeMap::from([("authorization".into(), Secret("Bearer test-only".into()))]),
    };
    let raw = "{\"text\":\"日本語\"}\r\n".as_bytes();
    let id = config.id;
    let original = config.clone();
    let job = common::job(config, raw);
    let client = dispatch::http_client().unwrap();
    let stop = CancellationToken::new();
    assert_eq!(
        dispatch::execute(&job, &client, &stop).await,
        Outcome::Succeeded
    );
    {
        let seen = seen.lock().unwrap();
        let (headers, body) = &seen[0];
        assert_eq!(body, raw);
        assert_eq!(headers["authorization"], "Bearer test-only");
        assert_eq!(headers["x-kicker-hook-id"], id.to_string());
        assert_eq!(headers["x-kicker-event"], "push");
        assert_eq!(headers["x-kicker-delivery-id"], DELIVERY);
        assert!(!headers.contains_key("x-hub-signature-256"));
    }
    let mut redirected = original;
    if let Target::Http { url, .. } = &mut redirected.target {
        *url = format!("{base}/redirect");
    }
    let job = common::job(redirected, raw);
    assert_eq!(
        dispatch::execute(&job, &client, &stop).await,
        Outcome::Failed(Failure::HttpStatus(307))
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    stop.cancel();
    assert_eq!(
        dispatch::execute(&job, &client, &stop).await,
        Outcome::Cancelled
    );
    server.abort();
}

#[tokio::test]
async fn shell_timeouts_terminate_the_process_group_and_nonzero_exits_fail() {
    let root = tempfile::tempdir().unwrap();
    let client = dispatch::http_client().unwrap();
    let stop = CancellationToken::new();
    assert_eq!(
        dispatch::execute(&job(hook(root.path(), "exit 7"), b"{}"), &client, &stop).await,
        Outcome::Failed(Failure::Exit {
            code: Some(7),
            signal: None
        })
    );
    let mut hook = hook(
        root.path(),
        "trap '' TERM; sh -c 'trap \"\" TERM; exec sleep 60' & echo $! > child.pid; wait",
    );
    hook.timeout_secs = 1;
    assert_eq!(
        dispatch::execute(&job(hook, b"{}"), &client, &stop).await,
        Outcome::TimedOut
    );
    let pid = std::fs::read_to_string(root.path().join("child.pid")).unwrap();
    for _ in 0..20 {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        if state.trim().is_empty() || state.trim().starts_with('Z') {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("descendant still running after process-group timeout");
}
