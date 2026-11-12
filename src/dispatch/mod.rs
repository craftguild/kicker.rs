mod http;
mod process;
mod shell;

pub use crate::job::Job;
use crate::model::Target;
use reqwest::Client;
use std::{io::ErrorKind, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Io {
        stage: &'static str,
        kind: ErrorKind,
    },
    Exit {
        code: Option<i32>,
        signal: Option<i32>,
    },
    HttpStatus(u16),
    HttpTransport(&'static str),
    InvalidMetadata,
    TaskPanic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    Failed(Failure),
    TimedOut,
    Cancelled,
}

impl Outcome {
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::TimedOut
                | Self::Failed(
                    Failure::Io { .. }
                        | Failure::Exit { .. }
                        | Failure::HttpStatus(_)
                        | Failure::HttpTransport(_)
                )
        )
    }

    pub fn state(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed(_) => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Failed(Failure::Io { stage, .. }) => Some(stage),
            Self::Failed(Failure::Exit { .. }) => Some("exit_status"),
            Self::Failed(Failure::HttpStatus(_)) => Some("http_status"),
            Self::Failed(Failure::HttpTransport(reason)) => Some(reason),
            Self::Failed(Failure::InvalidMetadata) => Some("invalid_metadata"),
            Self::Failed(Failure::TaskPanic) => Some("task_panic"),
            Self::TimedOut => Some("deadline"),
            Self::Cancelled => Some("cancelled"),
            Self::Succeeded => None,
        }
    }
}

pub fn http_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()?)
}

pub async fn execute(job: &Job, client: &Client, stop: &CancellationToken) -> Outcome {
    if stop.is_cancelled() {
        return Outcome::Cancelled;
    }
    match &job.target {
        Target::Cmd { command, cwd } => shell::run(job, command, cwd, stop).await,
        Target::Http { url, headers } => http::run(job, client, url, headers, stop).await,
    }
}
