mod github;
mod setup;

use axum::http::{HeaderMap, Method, StatusCode, Uri};
use bytes::Bytes;
pub use setup::{SetupUi, TerminalUi};
use std::{collections::BTreeMap, sync::Arc};
use thiserror::Error;

/// Complete transport input. Verification must use body bytes before decoding.
pub struct WebhookRequest {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone)]
pub struct VerifiedPayload {
    pub body: Bytes,
    pub content_type: String,
    pub event: String,
    pub delivery_id: String,
}

pub enum VerificationOutcome {
    Dispatch(VerifiedPayload),
    Respond { status: StatusCode, body: Bytes },
}

#[derive(Debug, Error)]
#[error("invalid webhook")]
pub struct VerifyError;

/// Compile-time provider extension point; no dynamically loaded code is needed.
pub trait WebhookProvider: Send + Sync {
    fn kind(&self) -> &'static str;
    fn configure(&self, ui: &mut dyn SetupUi) -> anyhow::Result<toml::Table>;
    fn validate_config(&self, config: &toml::Table) -> anyhow::Result<()>;
    fn verify(
        &self,
        config: &toml::Table,
        request: &WebhookRequest,
    ) -> Result<VerificationOutcome, VerifyError>;
}

#[derive(Clone)]
pub struct Providers(BTreeMap<String, Arc<dyn WebhookProvider>>);

impl Providers {
    pub fn new(providers: Vec<Arc<dyn WebhookProvider>>) -> anyhow::Result<Self> {
        let mut map = BTreeMap::new();
        for provider in providers {
            let name = provider.kind().to_owned();
            anyhow::ensure!(
                map.insert(name, provider).is_none(),
                "duplicate provider type"
            );
        }
        Ok(Self(map))
    }

    pub fn builtins() -> Self {
        Self::new(vec![Arc::new(github::GitHub)]).expect("unique built-in providers")
    }

    pub fn get(&self, kind: &str) -> anyhow::Result<&dyn WebhookProvider> {
        self.0
            .get(kind)
            .map(AsRef::as_ref)
            .ok_or_else(|| anyhow::anyhow!("unsupported provider type"))
    }
}
