use super::{Failure, Job, Outcome, process::ProcessGroup};
use std::{os::unix::process::ExitStatusExt, path::Path, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;

struct StdinWriter(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for StdinWriter {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn run(job: &Job, command: &str, cwd: &Path, stop: &CancellationToken) -> Outcome {
    let mut builder = Command::new("/bin/sh");
    builder
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .process_group(0)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("KICKER_HOOK_ID", job.hook_id.to_string());
    for (key, value) in job.metadata() {
        builder.env(format!("KICKER_{key}"), value);
    }
    let mut child = match builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Outcome::Failed(Failure::Io {
                stage: "spawn",
                kind: error.kind(),
            });
        }
    };
    let mut group = ProcessGroup::new(child.id().expect("spawned child has an ID"));
    let mut stdin = child.stdin.take().expect("piped stdin");
    let body = job.payload.body.clone();
    let mut writer = StdinWriter(tokio::spawn(async move {
        let result = stdin.write_all(&body).await;
        let _ = stdin.shutdown().await;
        result
    }));
    let outcome = tokio::select! {
        biased;
        () = stop.cancelled() => Outcome::Cancelled,
        result = tokio::time::timeout(job.timeout, child.wait()) => match result {
            Ok(Ok(status)) if status.success() => Outcome::Succeeded,
            Ok(Ok(status)) => Outcome::Failed(Failure::Exit { code: status.code(), signal: status.signal() }),
            Ok(Err(error)) => Outcome::Failed(Failure::Io { stage: "wait", kind: error.kind() }),
            Err(_) => Outcome::TimedOut,
        }
    };
    writer.0.abort();
    let _ = (&mut writer.0).await;
    group.settle(&mut child).await;
    outcome
}
