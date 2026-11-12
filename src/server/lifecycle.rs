use crate::queue::Dispatcher;
use anyhow::{Result, anyhow};
use std::{future::Future, io, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

struct Tasks {
    web: JoinHandle<io::Result<()>>,
    worker: JoinHandle<Result<()>>,
    signal: JoinHandle<()>,
    dispatcher: Dispatcher,
    closing: CancellationToken,
}

impl Drop for Tasks {
    fn drop(&mut self) {
        self.closing.cancel();
        self.dispatcher.cancel();
        self.signal.abort();
        self.web.abort();
        self.worker.abort();
    }
}

pub(super) async fn supervise(
    web: JoinHandle<io::Result<()>>,
    worker: JoinHandle<Result<()>>,
    dispatcher: Dispatcher,
    closing: CancellationToken,
    grace_secs: u64,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let signal = {
        let closing = closing.clone();
        let dispatcher = dispatcher.clone();
        tokio::spawn(async move {
            shutdown.await;
            closing.cancel();
            dispatcher.close();
        })
    };
    let mut tasks = Tasks {
        web,
        worker,
        signal,
        dispatcher,
        closing,
    };
    let (mut worker_done, mut web_done, mut signal_done) = (false, false, false);
    let mut failure = None;
    tokio::select! {
        biased;
        () = tasks.closing.cancelled() => {}
        result = &mut tasks.worker => {
            worker_done = true;
            if !tasks.closing.is_cancelled() || !matches!(result, Ok(Ok(()))) {
                failure = Some(anyhow!("dispatcher stopped unexpectedly"));
            }
        }
        result = &mut tasks.web => {
            web_done = true;
            if !tasks.closing.is_cancelled() || !matches!(result, Ok(Ok(()))) {
                failure = Some(anyhow!("HTTP server stopped unexpectedly"));
            }
        }
        result = &mut tasks.signal => {
            signal_done = true;
            if result.is_err() {
                failure = Some(anyhow!("shutdown watcher failed"));
            }
        }
    }
    tasks.closing.cancel();
    tasks.dispatcher.close();
    if !worker_done {
        let result =
            match tokio::time::timeout(Duration::from_secs(grace_secs), &mut tasks.worker).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::info!("shutdown grace expired; cancelling unfinished jobs");
                    tasks.dispatcher.cancel();
                    (&mut tasks.worker).await
                }
            };
        if !matches!(result, Ok(Ok(()))) {
            failure = Some(anyhow!("dispatcher failed"));
        }
    }
    if !web_done {
        match tokio::time::timeout(Duration::from_secs(2), &mut tasks.web).await {
            Ok(Ok(Ok(()))) => {}
            Ok(_) => {
                failure = Some(anyhow!("HTTP server failed"));
            }
            Err(_) => {
                tasks.web.abort();
                let _ = (&mut tasks.web).await;
            }
        }
    }
    if !signal_done {
        tasks.signal.abort();
        let _ = (&mut tasks.signal).await;
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/lifecycle.rs"]
mod tests;
