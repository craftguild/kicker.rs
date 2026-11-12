mod admission;
mod lifecycle;
mod transport;

use crate::{
    config::Store,
    model::{Config, ServerConfig},
    provider::Providers,
    queue::Dispatcher,
    validation,
};
use anyhow::{Context, Result};
use axum::Router;
use std::{future::Future, net::SocketAddr};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Load one immutable snapshot for an embedded router.
pub fn router(store: Store, dispatcher: Dispatcher, max_payload_bytes: usize) -> Result<Router> {
    let mut config = store.read()?;
    config.server.max_payload_bytes = max_payload_bytes;
    validation::server(&config.server)?;
    Ok(admission::router(
        config,
        store.providers,
        dispatcher,
        CancellationToken::new(),
    ))
}

pub fn router_with_config(
    store: Store,
    dispatcher: Dispatcher,
    settings: &ServerConfig,
    closing: CancellationToken,
) -> Result<Router> {
    validation::server(settings)?;
    let mut config = store.read()?;
    config.server = settings.clone();
    Ok(admission::router(
        config,
        store.providers,
        dispatcher,
        closing,
    ))
}

pub async fn run(
    listener: TcpListener,
    store: Store,
    settings: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let mut config = store.read()?;
    config.server = settings;
    run_snapshot(listener, config, store.providers, shutdown).await
}

async fn run_snapshot(
    listener: TcpListener,
    config: Config,
    providers: Providers,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let settings = config.server.clone();
    let dispatcher = Dispatcher::new(settings.clone())?;
    let closing = CancellationToken::new();
    let app = admission::router(config, providers, dispatcher.clone(), closing.clone());
    let worker = tokio::spawn(dispatcher.clone().run());
    let web = tokio::spawn(transport::run(
        listener,
        app,
        closing.clone(),
        settings.body_timeout_secs,
    ));
    lifecycle::supervise(
        web,
        worker,
        dispatcher,
        closing,
        settings.shutdown_timeout_secs,
        shutdown,
    )
    .await
}

pub async fn serve(store: Store, bind: Option<SocketAddr>) -> Result<()> {
    let _lock = store.serve_lock()?;
    let config = store.read()?;
    let listener = TcpListener::bind(bind.unwrap_or(config.server.bind))
        .await
        .context("could not bind listener")?;
    tracing::info!(bind = %listener.local_addr()?, "listening");
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    run_snapshot(listener, config, store.providers, async move {
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    })
    .await
}
