use super::{Failure, Job, Outcome};
use crate::model::Secret;
use reqwest::{
    Client,
    header::{HeaderName, HeaderValue},
};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub(super) async fn run(
    job: &Job,
    client: &Client,
    url: &str,
    headers: &BTreeMap<String, Secret>,
    stop: &CancellationToken,
) -> Outcome {
    let mut request = client
        .post(url)
        .body(job.payload.body.clone())
        .header("Content-Type", &job.payload.content_type)
        .header("X-Kicker-Hook-Id", job.hook_id.to_string());
    for (key, value) in job.metadata() {
        request = request.header(format!("X-Kicker-{}", key.replace('_', "-")), value);
    }
    for (name, secret) in headers {
        let (Ok(name), Ok(mut value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&secret.0),
        ) else {
            return Outcome::Failed(Failure::InvalidMetadata);
        };
        value.set_sensitive(true);
        request = request.header(name, value);
    }
    tokio::select! {
        biased;
        () = stop.cancelled() => Outcome::Cancelled,
        result = tokio::time::timeout(job.timeout, request.send()) => match result {
            Ok(Ok(response)) if response.status().is_success() => Outcome::Succeeded,
            Ok(Ok(response)) => Outcome::Failed(Failure::HttpStatus(response.status().as_u16())),
            Ok(Err(error)) => {
                let reason = if error.is_timeout() {
                    "http_timeout"
                } else if error.is_connect() {
                    "http_connect"
                } else if error.is_builder() {
                    "http_request"
                } else {
                    "http_transport"
                };
                Outcome::Failed(Failure::HttpTransport(reason))
            }
            Err(_) => Outcome::TimedOut,
        }
    }
}
