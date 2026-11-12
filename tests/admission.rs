mod common;
use axum::{
    body::{Body, Bytes},
    http::{Request, StatusCode},
};
use common::*;
use hyper::body::{Body as HttpBody, Frame};
use kicker::{model::ServerConfig, queue::Dispatcher, server};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

struct PendingBody(Arc<Notify>);
impl HttpBody for PendingBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.0.notify_one();
        Poll::Pending
    }
}

#[tokio::test]
async fn slow_bodies_are_bounded_and_slots_release_on_timeout() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "true");
    store.add(hook.clone()).unwrap();
    let settings = ServerConfig {
        max_in_flight_requests: 1,
        body_timeout_secs: 1,
        ..Default::default()
    };
    let queue = Dispatcher::new(settings.clone()).unwrap();
    let app = server::router_with_config(store, queue.clone(), &settings, CancellationToken::new())
        .unwrap();
    let notified = Arc::new(Notify::new());
    let request = Request::builder()
        .method("POST")
        .uri(format!("/{}", hook.id))
        .body(Body::new(PendingBody(notified.clone())))
        .unwrap();
    let pending = tokio::spawn(app.clone().oneshot(request));
    notified.notified().await;
    assert_eq!(
        app.clone()
            .oneshot(common::request(hook.id, b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        pending.await.unwrap().unwrap().status(),
        StatusCode::REQUEST_TIMEOUT
    );
    assert_eq!(
        app.oneshot(common::request(hook.id, b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    queue.cancel();
}

#[tokio::test]
async fn shutdown_interrupts_a_body_without_waiting_for_its_deadline() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "true");
    store.add(hook.clone()).unwrap();
    let settings = ServerConfig {
        body_timeout_secs: 60,
        ..Default::default()
    };
    let queue = Dispatcher::new(settings.clone()).unwrap();
    let closing = CancellationToken::new();
    let app = server::router_with_config(store, queue.clone(), &settings, closing.clone()).unwrap();
    let notified = Arc::new(Notify::new());
    let request = Request::builder()
        .method("POST")
        .uri(format!("/{}", hook.id))
        .body(Body::new(PendingBody(notified.clone())))
        .unwrap();
    let pending = tokio::spawn(app.oneshot(request));
    notified.notified().await;
    closing.cancel();
    let response = tokio::time::timeout(Duration::from_millis(500), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(queue.counts(), (0, 0));
}

#[test]
fn configuration_lock_wait_is_interruptible() {
    use std::{
        fs::OpenOptions,
        sync::atomic::{AtomicBool, Ordering},
    };
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    store.add(hook(root.path(), "true")).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.home.join("config.lock"))
        .unwrap();
    file.lock().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = store.read_interruptible(|| flag.load(Ordering::Acquire), |_| Ok(()));
        send.send(result.is_err()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(receive.try_recv().is_err());
    cancelled.store(true, Ordering::Release);
    let result = receive.recv_timeout(Duration::from_secs(1));
    drop(file);
    worker.join().unwrap();
    assert!(result.unwrap());
}

#[tokio::test]
async fn the_server_owns_and_closes_incomplete_http_connections() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let token = stop.clone();
    let server = tokio::spawn(server::run(
        listener,
        store,
        ServerConfig::default(),
        async move { token.cancelled().await },
    ));
    let client = reqwest::Client::new();
    client
        .get(format!("http://{address}/not-a-hook"))
        .send()
        .await
        .unwrap();
    let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
    connection
        .write_all(b"POST /pending HTTP/1.1\r\nHost: local\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(4), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut byte = [0u8];
    if let Ok(size) = tokio::time::timeout(Duration::from_secs(1), connection.read(&mut byte))
        .await
        .unwrap()
    {
        assert_eq!(size, 0);
    }
}
