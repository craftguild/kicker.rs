use super::*;
use crate::model::ServerConfig;

#[tokio::test]
async fn unexpected_executor_exit_stops_http_and_fails_the_server() {
    let dispatcher = Dispatcher::new(ServerConfig::default()).unwrap();
    let closing = CancellationToken::new();
    let web_stop = closing.clone();
    let web = tokio::spawn(async move {
        web_stop.cancelled().await;
        Ok(())
    });
    let worker = tokio::spawn(async { Ok(()) });
    let result = supervise(
        web,
        worker,
        dispatcher.clone(),
        closing.clone(),
        1,
        std::future::pending(),
    )
    .await;
    assert!(result.is_err());
    assert!(closing.is_cancelled());
    assert_eq!(dispatcher.counts(), (0, 0));
}

#[tokio::test]
async fn executor_failure_does_not_leak_internal_error_text() {
    let dispatcher = Dispatcher::new(ServerConfig::default()).unwrap();
    let closing = CancellationToken::new();
    let token = closing.clone();
    let web = tokio::spawn(async move {
        token.cancelled().await;
        Ok(())
    });
    let worker = tokio::spawn(async { Err(anyhow!("SECRET-FAILURE-SOURCE")) });
    let error = supervise(web, worker, dispatcher, closing, 1, std::future::pending())
        .await
        .unwrap_err();
    assert!(!format!("{error:#}").contains("SECRET"));
}

#[tokio::test]
async fn unexpected_http_failure_closes_and_drains_the_executor() {
    let dispatcher = Dispatcher::new(ServerConfig::default()).unwrap();
    let worker = tokio::spawn(dispatcher.clone().run());
    let web = tokio::spawn(async { Err(io::Error::other("transport error")) });
    let result = supervise(
        web,
        worker,
        dispatcher.clone(),
        CancellationToken::new(),
        1,
        std::future::pending(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(dispatcher.counts(), (0, 0));
}

#[tokio::test]
async fn explicit_shutdown_allows_clean_executor_exit() {
    let dispatcher = Dispatcher::new(ServerConfig::default()).unwrap();
    let closing = CancellationToken::new();
    let token = closing.clone();
    let worker = tokio::spawn(dispatcher.clone().run());
    let web = tokio::spawn(async move {
        token.cancelled().await;
        Ok(())
    });
    supervise(web, worker, dispatcher, closing, 1, async {})
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_shutdown_watcher_stops_the_server() {
    let dispatcher = Dispatcher::new(ServerConfig::default()).unwrap();
    let closing = CancellationToken::new();
    let token = closing.clone();
    let worker = tokio::spawn(dispatcher.clone().run());
    let web = tokio::spawn(async move {
        token.cancelled().await;
        Ok(())
    });
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        supervise(web, worker, dispatcher, closing, 1, async {
            panic!("simulated shutdown watcher failure");
        }),
    )
    .await
    .unwrap();
    assert!(result.is_err());
}
