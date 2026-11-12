mod common;
use axum::http::{StatusCode, Uri};
use common::*;
use kicker::{model::ServerConfig, queue::Dispatcher, server, validation};
use std::process::Command;
use tower::ServiceExt;

#[tokio::test]
async fn root_nested_and_trailing_slash_prefixes_route_only_the_configured_path() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "true");
    store.add(hook.clone()).unwrap();
    assert_eq!(store.read().unwrap().server.base_path, "/");
    for base_path in ["/", "/hooks", "/events/github.v1/", "/hooks/"] {
        store
            .edit(|config| {
                config.server.base_path = base_path.into();
                Ok(())
            })
            .unwrap();
        let queue = Dispatcher::new(store.read().unwrap().server).unwrap();
        let app = server::router(store.clone(), queue.clone(), 1024).unwrap();
        let prefix = base_path.trim_end_matches('/');
        let mut correct = request(hook.id, b"{}", "push");
        *correct.uri_mut() = format!("{prefix}/{}", hook.id).parse::<Uri>().unwrap();
        assert_eq!(
            app.clone().oneshot(correct).await.unwrap().status(),
            StatusCode::ACCEPTED
        );
        let mut incorrect = request(hook.id, b"{}", "push");
        *incorrect.uri_mut() = if prefix.is_empty() {
            format!("/hooks/{}", hook.id)
        } else {
            format!("/{}", hook.id)
        }
        .parse()
        .unwrap();
        assert_eq!(
            app.oneshot(incorrect).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        queue.cancel();
    }
}

#[tokio::test]
async fn list_uses_current_configuration_while_running_router_keeps_its_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "true");
    store.add(hook.clone()).unwrap();
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    let active = server::router(store.clone(), queue.clone(), 1024).unwrap();
    store
        .edit(|config| {
            config.server.base_path = "/hooks".into();
            Ok(())
        })
        .unwrap();
    let listing = Command::new(env!("CARGO_BIN_EXE_kicker"))
        .args(["--home", store.home.to_str().unwrap(), "list"])
        .output()
        .unwrap();
    assert!(listing.status.success());
    assert!(listing.stderr.is_empty());
    let text = String::from_utf8(listing.stdout).unwrap();
    let row = text.lines().nth(1).unwrap().split('\t').collect::<Vec<_>>();
    assert_eq!(
        row,
        [
            hook.id.to_string(),
            "github".into(),
            format!("/hooks/{}", hook.id),
            "cmd".into()
        ]
    );
    assert_eq!(
        active
            .clone()
            .oneshot(request(hook.id, b"{}", "push"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    let mut prefixed = request(hook.id, b"{}", "push");
    *prefixed.uri_mut() = format!("/hooks/{}", hook.id).parse().unwrap();
    assert_eq!(
        active.oneshot(prefixed).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    queue.cancel();
    let settings = store.read().unwrap().server;
    let queue = Dispatcher::new(settings).unwrap();
    let restarted = server::router(store, queue.clone(), 1024).unwrap();
    let mut prefixed = request(hook.id, b"{}", "push");
    *prefixed.uri_mut() = format!("/hooks/{}", hook.id).parse().unwrap();
    assert_eq!(
        restarted.oneshot(prefixed).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    queue.cancel();
}

#[test]
fn invalid_or_dynamic_base_paths_are_rejected_before_router_construction() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for path in [
        "", "hooks", "//hooks", "/a//b", "/a/../b", "/.", "/{id}", "/:id", "/{*path}", "/a?x=1",
        "/a#b", "/a b", "/a%2Fb", "/a\\b", "/a\n",
    ] {
        let settings = ServerConfig {
            base_path: path.into(),
            ..Default::default()
        };
        assert!(validation::server(&settings).is_err(), "{path:?}");
        let queue = Dispatcher::new(ServerConfig::default()).unwrap();
        assert!(
            server::router_with_config(store.clone(), queue, &settings, Default::default())
                .is_err()
        );
    }
}
