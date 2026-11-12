use axum::{Router, body::Body};
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{io, time::Duration};
use tokio::{net::TcpListener, task::JoinSet};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// Own all connection tasks so dropping/aborting HTTP serving closes sockets too.
pub(super) async fn run(
    listener: TcpListener,
    app: Router,
    closing: CancellationToken,
    header_timeout_secs: u64,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            () = closing.cancelled() => break,
            result = listener.accept() => {
                let (stream, _) = result?;
                let app = app.clone();
                let closing = closing.clone();
                connections.spawn(async move {
                    let service = service_fn(move |request| app.clone().oneshot(request.map(Body::new)));
                    let mut builder = http1::Builder::new();
                    builder
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(header_timeout_secs));
                    let connection = builder.serve_connection(TokioIo::new(stream), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        result = &mut connection => {
                            if result.is_err() {
                                tracing::debug!("HTTP connection failed");
                            }
                        }
                        () = closing.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            let _ = connection.await;
                        }
                    }
                });
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if result.is_err() {
                    tracing::error!("HTTP connection task failed");
                }
            }
        }
    }
    drop(listener);
    while connections.join_next().await.is_some() {}
    Ok(())
}
