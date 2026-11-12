mod common;
use common::*;
use kicker::{
    dispatch::{self, Outcome},
    model::ServerConfig,
    queue::Dispatcher,
};
use std::{fs, time::Duration};
use tokio_util::sync::CancellationToken;

#[test]
fn disappeared_working_directory_does_not_break_other_hooks() {
    let root = tempfile::tempdir().unwrap();
    let gone = root.path().join("gone");
    fs::create_dir(&gone).unwrap();
    let store = store(root.path());
    store.add(hook(&gone, "true")).unwrap();
    let healthy = hook(root.path(), "true");
    store.add(healthy.clone()).unwrap();
    fs::remove_dir(&gone).unwrap();
    assert!(
        store.read().is_ok(),
        "a runtime cwd failure must not invalidate other registrations"
    );
    assert!(
        store
            .read()
            .unwrap()
            .hooks
            .iter()
            .any(|hook| hook.id == healthy.id)
    );
}

#[test]
fn invalid_hook_can_be_deleted_to_repair_a_parseable_configuration() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = hook(root.path(), "true");
    store.add(hook.clone()).unwrap();
    let path = store.home.join("config.toml");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("type = \"github\"", "type = \"unavailable-provider\"");
    fs::write(path, text).unwrap();
    assert!(store.read().is_err());
    assert!(
        store.delete(hook.id).is_ok(),
        "deletion must validate after removing the invalid registration"
    );
    assert!(store.read().unwrap().hooks.is_empty());
}

fn running(pid: i32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&output.stdout);
    !state.trim().is_empty() && !state.trim().starts_with('Z')
}

struct Cleanup(i32);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.0),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[tokio::test]
async fn normal_shell_completion_does_not_leave_background_children() {
    let root = tempfile::tempdir().unwrap();
    let job = job(hook(root.path(), "sleep 60 & echo $! > child.pid"), b"{}");
    let result = dispatch::execute(
        &job,
        &dispatch::http_client().unwrap(),
        &CancellationToken::new(),
    )
    .await;
    let pid = fs::read_to_string(root.path().join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let _cleanup = Cleanup(pid);
    assert_eq!(result, Outcome::Succeeded);
    assert!(
        !running(pid),
        "job completion must include its process-group cleanup"
    );
}

#[tokio::test]
async fn dropping_a_shell_future_also_cleans_its_process_group() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("child.pid");
    let job = job(
        hook(root.path(), "sleep 60 & echo $! > child.pid; wait"),
        b"{}",
    );
    let task = tokio::spawn(async move {
        dispatch::execute(
            &job,
            &dispatch::http_client().unwrap(),
            &CancellationToken::new(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid = fs::read_to_string(path).unwrap().trim().parse().unwrap();
    let _cleanup = Cleanup(pid);
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !running(pid),
        "aborting an executor must not orphan its descendants"
    );
}

#[test]
fn dispatcher_constructor_rejects_invalid_request_limits() {
    assert!(
        Dispatcher::new(ServerConfig {
            max_in_flight_requests: 0,
            ..Default::default()
        })
        .is_err()
    );
}
