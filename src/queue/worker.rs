use super::Inner;
use crate::dispatch::{self, Job, Outcome};

pub(super) async fn run(job: &Job, inner: &Inner) -> Outcome {
    for retry in 0..=job.max_retries {
        let outcome = dispatch::execute(job, &inner.client, &inner.stop).await;
        if !outcome.retryable() || retry == job.max_retries {
            return outcome;
        }
        if !inner
            .state
            .lock()
            .expect("queue lock")
            .retry_wait(job.hook_id, outcome)
        {
            return Outcome::Cancelled;
        }
        tokio::select! {
            biased;
            () = inner.stop.cancelled() => return Outcome::Cancelled,
            () = tokio::time::sleep(job.retry_interval) => {}
        }
        if !inner
            .state
            .lock()
            .expect("queue lock")
            .retry_start(job.hook_id)
        {
            return Outcome::Cancelled;
        }
    }
    unreachable!("at least one execution attempt")
}
