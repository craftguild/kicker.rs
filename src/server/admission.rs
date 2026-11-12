use crate::{
    dispatch::Job,
    model::Config,
    provider::{Providers, VerificationOutcome, WebhookRequest},
    queue::Dispatcher,
};
use axum::{
    Router,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct App {
    hooks: HashMap<Uuid, crate::model::HookConfig>,
    providers: Providers,
    dispatcher: Dispatcher,
    max_body: usize,
    body_timeout: Duration,
    slots: Arc<Semaphore>,
    closing: CancellationToken,
}

async fn receive(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Ok(id) = Uuid::parse_str(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if app.closing.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(permit) = app.slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (parts, body) = request.into_parts();
    let result = tokio::select! {
        biased;
        () = app.closing.cancelled() => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        result = tokio::time::timeout(app.body_timeout, to_bytes(body, app.max_body)) => result,
    };
    let body = match result {
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
        Ok(Err(error))
            if error
                .source()
                .is_some_and(|cause| cause.is::<http_body_util::LengthLimitError>()) =>
        {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
        Ok(Err(_)) => return StatusCode::BAD_REQUEST.into_response(),
        Ok(Ok(body)) => body,
    };
    // The permit moves into the blocking operation: dropping the HTTP future
    // cannot let more verification work start while this one is still running.
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Response> {
        let _permit = permit;
        let Some(hook) = app.hooks.get(&id) else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        let request = WebhookRequest {
            method: parts.method,
            uri: parts.uri,
            headers: parts.headers,
            body,
        };
        let outcome = app
            .providers
            .get(&hook.kind)?
            .verify(&hook.verification, &request);
        if app.closing.is_cancelled() {
            return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
        match outcome {
            Err(_) => Ok(StatusCode::BAD_REQUEST.into_response()),
            Ok(VerificationOutcome::Respond { status, body }) => Ok((status, body).into_response()),
            Ok(VerificationOutcome::Dispatch(payload)) => {
                let job = Job::new(hook, payload)?;
                Ok(if app.dispatcher.try_enqueue(job).is_ok() {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                .into_response())
            }
        }
    })
    .await;
    match result {
        Ok(Ok(response)) => response,
        _ => {
            tracing::error!("admission unavailable");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

pub(super) fn router(
    config: Config,
    providers: Providers,
    dispatcher: Dispatcher,
    closing: CancellationToken,
) -> Router {
    let settings = config.server;
    Router::new()
        .route(&settings.hook_path("{id}"), post(receive))
        .with_state(Arc::new(App {
            hooks: config
                .hooks
                .into_iter()
                .map(|hook| (hook.id, hook))
                .collect(),
            providers,
            dispatcher,
            max_body: settings.max_payload_bytes,
            body_timeout: Duration::from_secs(settings.body_timeout_secs),
            slots: Arc::new(Semaphore::new(settings.max_in_flight_requests)),
            closing,
        }))
}
