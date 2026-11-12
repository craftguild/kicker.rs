use super::AdmissionError;
use crate::{
    dispatch::{Failure, Job, Outcome},
    model::ServerConfig,
};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

struct Identity {
    id: Uuid,
    hook: Uuid,
    provider: String,
    delivery: String,
    bytes: usize,
    state: Option<&'static str>,
    attempt: u64,
}

impl Identity {
    fn of(job: &Job, state: Option<&'static str>) -> Self {
        Self {
            id: job.id,
            hook: job.hook_id,
            provider: job.provider.clone(),
            delivery: job.payload.delivery_id.clone(),
            bytes: job.payload.body.len(),
            state,
            attempt: 0,
        }
    }

    fn transition(&mut self, state: &'static str, outcome: Option<Outcome>) {
        let (mut exit_code, mut signal, mut http_status, mut io_kind) = (None, None, None, None);
        match outcome {
            Some(Outcome::Failed(Failure::Exit {
                code,
                signal: value,
            })) => {
                exit_code = code;
                signal = value;
            }
            Some(Outcome::Failed(Failure::HttpStatus(status))) => http_status = Some(status),
            Some(Outcome::Failed(Failure::Io { kind, .. })) => io_kind = Some(kind),
            _ => {}
        }
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!($level, transition_id = %Uuid::new_v4(), job_id = %self.id, hook_id = %self.hook,
                    provider = %self.provider, delivery_id = %self.delivery, previous_state = self.state, state,
                    attempt = self.attempt, reason = outcome.and_then(Outcome::reason), exit_code, signal,
                    http_status, io_kind = ?io_kind, "job transition")
            };
        }
        if matches!(outcome, Some(Outcome::Failed(_) | Outcome::TimedOut)) {
            emit!(tracing::Level::ERROR);
        } else {
            emit!(tracing::Level::INFO);
        }
        self.state = Some(state);
    }
}

struct HookQueue {
    capacity: usize,
    pending: VecDeque<Job>,
    active: Option<Identity>,
    bytes: usize,
}

pub(super) struct State {
    hooks: HashMap<Uuid, HookQueue>,
    pub closed: bool,
    pub failed: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
            closed: false,
            failed: false,
        }
    }

    pub fn admit(&mut self, job: Job, limits: &ServerConfig) -> Result<(), AdmissionError> {
        if self.closed {
            return Err(AdmissionError::Closed);
        }
        let queue = self.hooks.entry(job.hook_id).or_insert_with(|| HookQueue {
            capacity: job.queue_size,
            pending: VecDeque::new(),
            active: None,
            bytes: 0,
        });
        if queue.pending.len() >= queue.capacity
            || job.payload.body.len() > limits.max_payload_bytes
            || job.payload.body.len() > limits.max_buffered_bytes.saturating_sub(queue.bytes)
        {
            return Err(AdmissionError::Full);
        }
        queue.bytes += job.payload.body.len();
        Identity::of(&job, None).transition("queued", None);
        queue.pending.push_back(job);
        Ok(())
    }

    pub fn next(&mut self) -> Option<Job> {
        let queue = self
            .hooks
            .values_mut()
            .find(|queue| queue.active.is_none() && !queue.pending.is_empty())?;
        let job = queue.pending.pop_front().expect("pending job");
        let mut identity = Identity::of(&job, Some("queued"));
        identity.attempt = 1;
        identity.transition("running", None);
        queue.active = Some(identity);
        Some(job)
    }

    pub fn retry_wait(&mut self, hook: Uuid, outcome: Outcome) -> bool {
        let Some(identity) = self.active(hook) else {
            return false;
        };
        assert_eq!(identity.state, Some("running"));
        identity.transition("retry_waiting", Some(outcome));
        true
    }

    pub fn retry_start(&mut self, hook: Uuid) -> bool {
        let Some(identity) = self.active(hook) else {
            return false;
        };
        assert_eq!(identity.state, Some("retry_waiting"));
        identity.attempt += 1;
        identity.transition("running", None);
        true
    }

    fn active(&mut self, hook: Uuid) -> Option<&mut Identity> {
        self.hooks
            .get_mut(&hook)
            .and_then(|queue| queue.active.as_mut())
    }

    pub fn finish(&mut self, hook: Uuid, outcome: Outcome) {
        let queue = self.hooks.get_mut(&hook).expect("known hook");
        let mut identity = queue.active.take().expect("active hook");
        queue.bytes -= identity.bytes;
        identity.transition(outcome.state(), Some(outcome));
    }

    pub fn counts(&self) -> (usize, usize) {
        self.hooks.values().fold((0, 0), |(count, bytes), queue| {
            (
                count + queue.pending.len() + usize::from(queue.active.is_some()),
                bytes + queue.bytes,
            )
        })
    }

    pub fn cancel_pending(&mut self) {
        self.closed = true;
        for queue in self.hooks.values_mut() {
            while let Some(job) = queue.pending.pop_front() {
                queue.bytes -= job.payload.body.len();
                Identity::of(&job, Some("queued"))
                    .transition("cancelled", Some(Outcome::Cancelled));
            }
        }
    }

    pub fn abort_runner(&mut self) {
        self.cancel_pending();
        for queue in self.hooks.values_mut() {
            if let Some(mut identity) = queue.active.take() {
                identity.transition("cancelled", Some(Outcome::Cancelled));
            }
            queue.bytes = 0;
        }
    }

    pub fn drained(&self) -> bool {
        self.closed && self.counts().0 == 0
    }
}
