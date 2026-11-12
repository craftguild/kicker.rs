mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bytes::Bytes;
use common::*;
use kicker::{config::Store, model::ServerConfig, provider::*, queue::Dispatcher};
use std::sync::Arc;
use tower::ServiceExt;

struct Example;
impl WebhookProvider for Example {
    fn kind(&self) -> &'static str {
        "example"
    }
    fn configure(&self, ui: &mut dyn SetupUi) -> anyhow::Result<toml::Table> {
        Ok(toml::Table::from_iter([
            ("token".into(), ui.secret("Example token")?.into()),
            ("hint".into(), ui.text("Example hint")?.into()),
        ]))
    }
    fn validate_config(&self, config: &toml::Table) -> anyhow::Result<()> {
        anyhow::ensure!(
            config.get("token").and_then(toml::Value::as_str) == Some("example-token"),
            "invalid example configuration"
        );
        Ok(())
    }
    fn verify(
        &self,
        _: &toml::Table,
        request: &WebhookRequest,
    ) -> Result<VerificationOutcome, VerifyError> {
        if request
            .headers
            .get("x-example-token")
            .and_then(|value| value.to_str().ok())
            != Some("example-token")
        {
            return Err(VerifyError);
        }
        Ok(VerificationOutcome::Respond {
            status: StatusCode::OK,
            body: request.body.clone(),
        })
    }
}

struct Ui;
impl SetupUi for Ui {
    fn secret(&mut self, _: &str) -> anyhow::Result<String> {
        Ok("example-token".into())
    }
    fn text(&mut self, _: &str) -> anyhow::Result<String> {
        Ok("hint".into())
    }
    fn header(&mut self) -> anyhow::Result<Option<(String, String)>> {
        Ok(None)
    }
}

#[tokio::test]
async fn provider_traits_add_service_setup_and_challenge_without_changing_the_server() {
    let root = tempfile::tempdir().unwrap();
    let providers = Providers::new(vec![Arc::new(Example)]).unwrap();
    let store = Store::new(root.path().join("home"), providers);
    let target = hook(root.path(), "touch never").target;
    let id = kicker::cli::register(&store, "example".into(), target, &mut Ui).unwrap();
    let queue = Dispatcher::new(ServerConfig::default()).unwrap();
    let app = kicker::server::router(store, queue.clone(), 1024).unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("/{id}"))
        .header("x-example-token", "example-token")
        .body(Body::from("challenge"))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(body, Bytes::from_static(b"challenge"));
    assert_eq!(queue.counts(), (0, 0));
    assert!(!root.path().join("never").exists());
}
