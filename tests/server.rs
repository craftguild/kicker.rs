mod common;
use axum::http::StatusCode;
use common::*;
use kicker::{model::ServerConfig, queue::Dispatcher, server::router};
use tower::ServiceExt;

#[tokio::test]
async fn rejection_ping_duplicate_delivery_and_capacity_never_bypass_admission() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let mut hook = hook(root.path(), "touch should-not-run");
    hook.queue_size = 2;
    store.add(hook.clone()).unwrap();
    let limits = ServerConfig {
        max_jobs: 2,
        max_parallel_hooks: 1,
        ..Default::default()
    };
    let queue = Dispatcher::new(limits).unwrap();
    let app = router(store.clone(), queue.clone(), 100).unwrap();
    let mut bad = request(hook.id, b"{}", "push");
    bad.headers_mut().remove("x-hub-signature-256");
    assert_eq!(
        app.clone().oneshot(bad).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(request(hook.id, b"{}", "ping"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(queue.counts(), (0, 0));
    assert!(!root.path().join("should-not-run").exists());
    for _ in 0..2 {
        assert_eq!(
            app.clone()
                .oneshot(request(hook.id, b"{}", "push"))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
    }
    assert_eq!(queue.counts(), (2, 4));
    assert_eq!(
        app.clone()
            .oneshot(request(hook.id, b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        app.clone()
            .oneshot(request(uuid::Uuid::new_v4(), b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.oneshot(request(hook.id, &[b' '; 101], "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    queue.cancel();
    assert_eq!(queue.counts(), (0, 0));
}

#[tokio::test]
async fn registration_deletion_and_manual_edits_require_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    let original = router(store.clone(), queue.clone(), 1024).unwrap();
    let hook = hook(root.path(), "cat > received.json");
    store.add(hook.clone()).unwrap();
    assert_eq!(
        original
            .oneshot(request(hook.id, b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let active = router(store.clone(), queue.clone(), 1024).unwrap();
    let body = "{ \"text\":\"日本語\" }\n".as_bytes();
    assert_eq!(
        active
            .clone()
            .oneshot(request(hook.id, body, "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    store.delete(hook.id).unwrap();
    assert_eq!(
        active
            .clone()
            .oneshot(request(hook.id, body, "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let restarted = router(store.clone(), queue.clone(), 1024).unwrap();
    assert_eq!(
        restarted
            .oneshot(request(hook.id, body, "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    std::fs::write(store.home.join("config.toml"), "invalid = [").unwrap();
    assert!(router(store, queue.clone(), 1024).is_err());
    assert_eq!(
        active
            .oneshot(request(hook.id, body, "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    queue.close();
    queue.clone().run().await.unwrap();
    assert_eq!(
        std::fs::read(root.path().join("received.json")).unwrap(),
        body
    );
    assert_eq!(queue.counts(), (0, 0));
}
