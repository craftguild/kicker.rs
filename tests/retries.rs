mod common;
use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use common::*;
use kicker::{
    model::{ServerConfig, Target},
    queue::{AdmissionError, Dispatcher},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct Attempt {
    body: Bytes,
    time: Instant,
}
type Seen = Arc<Mutex<HashMap<String, Vec<Attempt>>>>;

async fn receive(State(seen): State<Seen>, Path(path): Path<String>, body: Bytes) -> StatusCode {
    let mut seen = seen.lock().unwrap();
    let calls = seen.entry(path.clone()).or_default();
    calls.push(Attempt {
        body,
        time: Instant::now(),
    });
    match path.as_str() {
        "success" => StatusCode::CREATED,
        "flaky" if calls.len() >= 3 => StatusCode::NO_CONTENT,
        "redirect" => StatusCode::TEMPORARY_REDIRECT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[tokio::test]
async fn http_retries_are_per_hook_bounded_delayed_and_stop_on_any_2xx() {
    let seen: Seen = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let target = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/{path}", post(receive))
                .with_state(seen.clone()),
        )
        .into_future(),
    );
    let root = tempfile::tempdir().unwrap();
    let queue = Dispatcher::new(ServerConfig {
        max_parallel_hooks: 1,
        ..Default::default()
    })
    .unwrap();
    for (path, retries, interval) in [
        ("failure", 3, 1),
        ("flaky", 5, 1),
        ("custom", 1, 2),
        ("success", 3, 1),
        ("redirect", 0, 1),
    ] {
        let mut hook = hook(root.path(), "true");
        hook.max_retries = retries;
        hook.retry_interval_secs = interval;
        hook.target = Target::Http {
            url: format!("{base}/{path}"),
            headers: BTreeMap::new(),
        };
        queue.try_enqueue(job(hook, b"original bytes\r\n")).unwrap();
    }
    queue.close();
    tokio::time::timeout(Duration::from_secs(10), queue.clone().run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(queue.counts(), (0, 0));
    let seen = seen.lock().unwrap();
    for (path, count, interval) in [
        ("failure", 4, 1),
        ("flaky", 3, 1),
        ("custom", 2, 2),
        ("success", 1, 1),
        ("redirect", 1, 1),
    ] {
        let attempts = &seen[path];
        assert_eq!(attempts.len(), count, "{path}");
        for attempt in attempts {
            assert_eq!(attempt.body, b"original bytes\r\n".as_slice());
        }
        for pair in attempts.windows(2) {
            assert!(pair[1].time.duration_since(pair[0].time) >= Duration::from_secs(interval));
        }
    }
    target.abort();
}

#[tokio::test]
async fn retry_wait_preserves_fifo_and_leaves_other_hooks_and_pending_capacity_independent() {
    let (events, mut receive) = mpsc::unbounded_channel();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let captured = seen.clone();
    let app = Router::new().route(
        "/",
        post(move |body: Bytes| {
            let events = events.clone();
            let seen = captured.clone();
            async move {
                let value = String::from_utf8(body.to_vec()).unwrap();
                let mut seen = seen.lock().unwrap();
                seen.push(value.clone());
                events.send(value.clone()).unwrap();
                if value == "first" && seen.iter().filter(|value| *value == "first").count() == 1 {
                    StatusCode::BAD_GATEWAY
                } else {
                    StatusCode::OK
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let target = tokio::spawn(axum::serve(listener, app).into_future());
    let root = tempfile::tempdir().unwrap();
    let mut a = hook(root.path(), "true");
    a.target = Target::Http {
        url,
        headers: BTreeMap::new(),
    };
    a.queue_size = 1;
    a.retry_interval_secs = 1;
    let mut b = a.clone();
    b.id = uuid::Uuid::new_v4();
    let queue = Dispatcher::new(ServerConfig {
        max_parallel_hooks: 1,
        ..Default::default()
    })
    .unwrap();
    queue.try_enqueue(job(a.clone(), b"first")).unwrap();
    let worker = tokio::spawn(queue.clone().run());
    assert_eq!(receive.recv().await.unwrap(), "first");
    queue.try_enqueue(job(a.clone(), b"second")).unwrap();
    assert_eq!(
        queue.try_enqueue(job(a, b"overflow")),
        Err(AdmissionError::Full)
    );
    queue.try_enqueue(job(b, b"other")).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(800), receive.recv())
            .await
            .unwrap()
            .unwrap(),
        "other"
    );
    queue.close();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), ["first", "other", "first", "second"]);
    assert_eq!(queue.counts(), (0, 0));
    target.abort();
}

#[tokio::test]
async fn command_failures_retry_with_identical_stdin_and_stop_after_success() {
    let root = tempfile::tempdir().unwrap();
    let mut hook = hook(
        root.path(),
        "n=0; if test -f count; then n=$(cat count); fi; n=$((n + 1)); printf '%s' \"$n\" > count; cat >> bodies; test \"$n\" -ge 3",
    );
    hook.retry_interval_secs = 1;
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    queue.try_enqueue(job(hook, b"exact\n")).unwrap();
    queue.close();
    tokio::time::timeout(Duration::from_secs(6), queue.clone().run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("count")).unwrap(),
        "3"
    );
    assert_eq!(
        std::fs::read(root.path().join("bodies")).unwrap(),
        b"exact\nexact\nexact\n"
    );
    assert_eq!(queue.counts(), (0, 0));
}

#[tokio::test]
async fn shutdown_cancels_a_sixty_second_retry_wait_and_pending_work_promptly() {
    let root = tempfile::tempdir().unwrap();
    let hook = hook(root.path(), "echo attempt >> attempts; exit 7");
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    queue.try_enqueue(job(hook.clone(), b"first")).unwrap();
    queue.try_enqueue(job(hook, b"second")).unwrap();
    let worker = tokio::spawn(queue.clone().run());
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("attempts").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    queue.cancel();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("attempts")).unwrap(),
        "attempt\n"
    );
    assert_eq!(queue.counts(), (0, 0));
}
