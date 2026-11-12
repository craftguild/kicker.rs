#![allow(dead_code)]
use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, Uri},
};
use bytes::Bytes;
use hmac::{Hmac, Mac};
use kicker::{
    config::Store,
    dispatch::Job,
    model::{HookConfig, Target},
    provider::{Providers, VerifiedPayload, WebhookRequest},
};
use sha2::Sha256;
use uuid::Uuid;

pub const SECRET: &str = "test secret not for production";
pub const DELIVERY: &str = "72d3162e-cc78-11e3-81ab-4c9367dc0958";

pub fn store(root: &std::path::Path) -> Store {
    Store::new(root.join("home"), Providers::builtins())
}

pub fn hook(root: &std::path::Path, command: &str) -> HookConfig {
    HookConfig {
        id: Uuid::new_v4(),
        kind: "github".into(),
        verification: toml::Table::from_iter([("secret".into(), SECRET.into())]),
        target: Target::Cmd {
            command: command.into(),
            cwd: root.into(),
        },
        timeout_secs: 10,
        queue_size: 256,
        retry_interval_secs: 60,
        max_retries: 3,
    }
}

pub fn signature(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

pub fn headers(body: &[u8], event: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (key, value) in [
        ("content-type", "application/json; charset=utf-8".into()),
        ("x-github-event", event.into()),
        ("x-github-delivery", DELIVERY.into()),
        ("x-hub-signature-256", signature(body)),
    ] {
        headers.insert(
            axum::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    headers
}

pub fn input(body: &[u8], event: &str) -> WebhookRequest {
    WebhookRequest {
        method: Method::POST,
        uri: Uri::from_static("/test"),
        headers: headers(body, event),
        body: Bytes::copy_from_slice(body),
    }
}

pub fn request(id: Uuid, body: &[u8], event: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/{id}"))
        .body(Body::from(body.to_vec()))
        .unwrap();
    *request.headers_mut() = headers(body, event);
    request
}

pub fn job(hook: HookConfig, body: &[u8]) -> Job {
    Job::new(
        &hook,
        VerifiedPayload {
            body: Bytes::copy_from_slice(body),
            content_type: "application/json".into(),
            event: "push".into(),
            delivery_id: DELIVERY.into(),
        },
    )
    .unwrap()
}
