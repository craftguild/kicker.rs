use crate::{
    model::{HookConfig, Target},
    provider::VerifiedPayload,
    validation,
};
use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

/// Immutable execution snapshot. Provider verification settings are not retained.
pub struct Job {
    pub(crate) id: Uuid,
    pub(crate) hook_id: Uuid,
    pub(crate) provider: String,
    pub(crate) target: Target,
    pub(crate) timeout: Duration,
    pub(crate) payload: VerifiedPayload,
    pub(crate) queue_size: usize,
    pub(crate) retry_interval: Duration,
    pub(crate) max_retries: u32,
}

impl Job {
    pub fn new(hook: &HookConfig, payload: VerifiedPayload) -> Result<Self> {
        validation::execution(hook)?;
        Ok(Self {
            id: Uuid::new_v4(),
            hook_id: hook.id,
            provider: hook.kind.clone(),
            target: hook.target.clone(),
            timeout: Duration::from_secs(hook.timeout_secs),
            payload,
            queue_size: hook.queue_size,
            retry_interval: Duration::from_secs(hook.retry_interval_secs),
            max_retries: hook.max_retries,
        })
    }

    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn hook_id(&self) -> Uuid {
        self.hook_id
    }

    pub fn metadata(&self) -> [(&'static str, &str); 4] {
        [
            ("PROVIDER", &self.provider),
            ("EVENT", &self.payload.event),
            ("DELIVERY_ID", &self.payload.delivery_id),
            ("CONTENT_TYPE", &self.payload.content_type),
        ]
    }
}
