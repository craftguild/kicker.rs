mod state;
mod worker;
use crate::{
    dispatch::{self, Failure, Job, Outcome},
    model::ServerConfig,
    validation,
};
use anyhow::{Result, ensure};
use state::State;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Notify, task::JoinSet};
use tokio_util::sync::CancellationToken;

struct Inner {
    state: Mutex<State>,
    notify: Notify,
    limits: ServerConfig,
    client: reqwest::Client,
    stop: CancellationToken,
    started: AtomicBool,
}

#[derive(Clone)]
pub struct Dispatcher(Arc<Inner>);

#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Closed,
    Full,
}

struct RunnerGuard(Arc<Inner>);
impl Drop for RunnerGuard {
    fn drop(&mut self) {
        self.0.stop.cancel();
        self.0.state.lock().expect("queue lock").abort_runner();
    }
}

impl Dispatcher {
    pub fn new(limits: ServerConfig) -> Result<Self> {
        validation::server(&limits)?;
        Ok(Self(Arc::new(Inner {
            state: Mutex::new(State::new()),
            notify: Notify::new(),
            limits,
            client: dispatch::http_client()?,
            stop: CancellationToken::new(),
            started: AtomicBool::new(false),
        })))
    }

    pub fn try_enqueue(&self, job: Job) -> Result<(), AdmissionError> {
        self.0
            .state
            .lock()
            .expect("queue lock")
            .admit(job, &self.0.limits)?;
        self.0.notify.notify_one();
        Ok(())
    }

    pub fn close(&self) {
        self.0.state.lock().expect("queue lock").closed = true;
        self.0.notify.notify_one();
    }

    pub fn cancel(&self) {
        self.0.state.lock().expect("queue lock").cancel_pending();
        self.0.stop.cancel();
        self.0.notify.notify_one();
    }

    pub fn counts(&self) -> (usize, usize) {
        let state = self.0.state.lock().expect("queue lock");
        state.counts()
    }

    /// Scheduling has one owner. Dropping that owner closes admission and cancels work.
    pub async fn run(self) -> Result<()> {
        ensure!(
            self.0
                .started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "dispatcher runner already started"
        );
        let _guard = RunnerGuard(self.0.clone());
        let mut tasks = JoinSet::new();
        let mut identities = HashMap::new();
        loop {
            {
                let mut state = self.0.state.lock().expect("queue lock");
                while let Some(job) = state.next() {
                    let hook = job.hook_id;
                    let inner = self.0.clone();
                    let task = tasks.spawn(async move { worker::run(&job, &inner).await });
                    identities.insert(task.id(), hook);
                }
                if state.drained() {
                    ensure!(!state.failed, "executor task failed");
                    return Ok(());
                }
            }
            tokio::select! {
                Some(result) = tasks.join_next_with_id(), if !tasks.is_empty() => {
                    let (id, outcome) = match result {
                        Ok((id, outcome)) => (id, outcome),
                        Err(error) => (error.id(), Outcome::Failed(Failure::TaskPanic)),
                    };
                    let hook = identities.remove(&id).expect("known job task");
                    self.0.state.lock().expect("queue lock").finish(hook, outcome);
                    if outcome == Outcome::Failed(Failure::TaskPanic) {
                        self.0.state.lock().expect("queue lock").failed = true;
                        self.cancel();
                    }
                }
                () = self.0.notify.notified() => {}
            }
        }
    }
}
