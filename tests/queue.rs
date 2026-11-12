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
    model::{ServerConfig, Target},
    queue::{AdmissionError, Dispatcher},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Seen {
    calls: Vec<(String, String)>,
    active: HashMap<String, usize>,
    maximum: usize,
    overlap: bool,
}

#[tokio::test]
async fn same_hook_is_fifo_other_hooks_overlap_and_redeliveries_are_not_deduplicated() {
    async fn receive(
        State(seen): State<Arc<Mutex<Seen>>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        let id = headers["x-kicker-hook-id"].to_str().unwrap().to_string();
        {
            let mut seen = seen.lock().unwrap();
            seen.calls
                .push((id.clone(), String::from_utf8(body.to_vec()).unwrap()));
            *seen.active.entry(id.clone()).or_default() += 1;
            let same = seen.active[&id];
            seen.maximum = seen.maximum.max(same);
            if seen.active.values().sum::<usize>() > 1 {
                seen.overlap = true;
            }
        }
        tokio::time::sleep(Duration::from_millis(60)).await;
        *seen.lock().unwrap().active.get_mut(&id).unwrap() -= 1;
        StatusCode::OK
    }
    let seen = Arc::new(Mutex::new(Seen::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/target", listener.local_addr().unwrap());
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/target", post(receive))
                .with_state(seen.clone()),
        )
        .into_future(),
    );
    let root = tempfile::tempdir().unwrap();
    let mut a = hook(root.path(), "true");
    let mut b = hook(root.path(), "true");
    a.target = Target::Http {
        url: url.clone(),
        headers: BTreeMap::new(),
    };
    b.target = a.target.clone();
    let queue = Dispatcher::new(ServerConfig {
        max_parallel_hooks: 1,
        ..Default::default()
    })
    .unwrap();
    for body in [b"1".as_slice(), b"2", b"2"] {
        queue.try_enqueue(job(a.clone(), body)).unwrap();
    }
    queue.try_enqueue(job(b, b"other")).unwrap();
    queue.close();
    tokio::time::timeout(Duration::from_secs(5), queue.clone().run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(queue.counts(), (0, 0));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.maximum, 1);
    assert!(seen.overlap);
    assert_eq!(
        seen.calls
            .iter()
            .filter(|(id, _)| id == &a.id.to_string())
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "2"]
    );
    server.abort();
}

#[tokio::test]
async fn count_and_byte_capacity_are_atomic_and_cancelled_pending_jobs_do_not_run() {
    let root = tempfile::tempdir().unwrap();
    let mut hook = hook(root.path(), "touch should-not-exist");
    hook.queue_size = 2;
    let settings = ServerConfig {
        max_jobs: 2,
        max_parallel_hooks: 1,
        max_payload_bytes: 3,
        max_buffered_bytes: 3,
        ..Default::default()
    };
    let queue = Dispatcher::new(settings).unwrap();
    queue.try_enqueue(job(hook.clone(), b"aa")).unwrap();
    assert_eq!(
        queue.try_enqueue(job(hook.clone(), b"bb")),
        Err(AdmissionError::Full)
    );
    queue.try_enqueue(job(hook.clone(), b"c")).unwrap();
    assert_eq!(queue.counts(), (2, 3));
    assert_eq!(
        queue.try_enqueue(job(hook.clone(), b"")),
        Err(AdmissionError::Full)
    );
    queue.cancel();
    assert_eq!(queue.counts(), (0, 0));
    assert_eq!(
        queue.try_enqueue(job(hook, b"x")),
        Err(AdmissionError::Closed)
    );
    queue.run().await.unwrap();
    assert!(!root.path().join("should-not-exist").exists());
}

#[tokio::test]
async fn exactly_one_runner_can_own_the_scheduler() {
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    let first = tokio::spawn(queue.clone().run());
    let second = tokio::spawn(queue.clone().run());
    queue.close();
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}

#[tokio::test]
async fn dropping_the_runner_closes_admission_and_releases_accounting() {
    let root = tempfile::tempdir().unwrap();
    let hook = hook(root.path(), "exec sleep 60");
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    queue.try_enqueue(job(hook.clone(), b"first")).unwrap();
    queue.try_enqueue(job(hook.clone(), b"second")).unwrap();
    let worker = tokio::spawn(queue.clone().run());
    tokio::task::yield_now().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert_eq!(queue.counts(), (0, 0));
    assert_eq!(
        queue.try_enqueue(job(hook, b"third")),
        Err(AdmissionError::Closed)
    );
}

#[test]
fn default_queue_capacity_and_byte_budgets_are_independent_per_hook() {
    let root = tempfile::tempdir().unwrap();
    let a = hook(root.path(), "true");
    let b = hook(root.path(), "true");
    let queue = Dispatcher::new(ServerConfig {
        max_jobs: 1,
        max_parallel_hooks: 1,
        max_payload_bytes: 1,
        max_buffered_bytes: 256,
        ..Default::default()
    })
    .unwrap();
    for _ in 0..256 {
        queue.try_enqueue(job(a.clone(), b"x")).unwrap();
    }
    assert_eq!(queue.try_enqueue(job(a, b"x")), Err(AdmissionError::Full));
    for _ in 0..256 {
        queue.try_enqueue(job(b.clone(), b"x")).unwrap();
    }
    assert_eq!(queue.try_enqueue(job(b, b"x")), Err(AdmissionError::Full));
    assert_eq!(queue.counts(), (512, 512));
    queue.cancel();
    assert_eq!(queue.counts(), (0, 0));
}
