# Design decisions

Kicker is one Rust package with a reusable library and a thin binary. CLI/config,
providers, admission/dispatch and HTTP serving have separate modules. Providers
own service-specific setup and verification; targets own shell/HTTP execution.
The [initial v1 review](review-v1.md) records the earlier review and fixes.
The following decisions describe the current implementation.

`terminal` owns offline help/version/SBOM before configuration, logging or Tokio
startup. The [standards adoption decision](../adr/0001-standards-v0.5.md) defines
build ownership and local completion. `queue/worker` owns a delivery's retry loop; `queue/state` owns capacity
and transition records, including the intermediate retry states.

## Configuration boundary

Hooks are registration settings, not durable workflow entities. Adding/removing
entries updates a single TOML snapshot. There is no enabled/disabled lifecycle
or persistent transition history for registrations. This is a configuration
registry, not a business Header-Detail aggregate.

`new` finishes prompts before acquiring the write lock. An exclusive config lock
protects load/update/validation/fsync/atomic rename. The server reads and validates
one snapshot before starting. Registration/deletion and all manual edits require
a server restart; admission performs no configuration-file I/O. The current
process retains its original hooks, secrets, targets and policies after disk edits.
`list` and registration output read the current file, rather than service state.
The shared path builder joins `server.base_path` (default `/`) and the hook ID
for both CLI output and HTTP routing. A base-path edit changes CLI output
immediately; the live route changes only after restart. Prefix validation rejects
router captures and malformed segments before router construction.

Jobs copy only execution settings and verified payload, not verification secrets.
Working-directory existence is checked at registration and later failures belong
to that job. Edits validate the resulting snapshot, allowing deletion to repair a
parseable invalid registration.

## Transient job lifecycle

Each hook owns a FIFO of pending deliveries, created on first admission. Its
`queue_size` limits waiting jobs (default 256); one active delivery is separate.
There is no configurable concurrency within a hook, or global job/concurrency
limit. The scheduler starts each ready hook independently. The existing server
`max_buffered_bytes` bounds accepted body bytes per hook, including active work,
so a hook cannot consume another hook's byte capacity. Admission checks the
hook's pending count and bytes atomically before `202`; no queued work is evicted.

One runner owns scheduling and its task set. Each hook runs one delivery at a
time, including retries. Other hooks keep progressing during its retry waits.
Completion releases the active slot and body bytes. A task panic closes/cancels
the dispatcher, and dropping the runner closes admission and settles accounting.

A job keeps one UUID, hook ID, original body and provider metadata across attempts.
Its allowed states are:

- Initial admission: no previous state -> `queued` (attempt 0).
- Dequeue: `queued` -> `running` (attempt 1).
- Retryable attempt failure with retries left: `running` -> `retry_waiting`.
- Retry delay completed: `retry_waiting` -> `running` (attempt incremented).
- Completion/exhaustion: `running` -> `succeeded | failed | timed_out`.
- Shutdown: `queued | running | retry_waiting` -> `cancelled`.

Queue state methods own these transitions under one mutex. Every transition logs
a transition UUID, job/hook/delivery identity, attempt, previous/new states,
optional safe failure reason and UTC time. Log append order resolves equal UTC
timestamps within each stream. INFO/WARN use stdout; ERROR uses stderr, including
failed attempts, retry waits after failure and timeouts. Event fields/job identity
allow records from both streams to be correlated.
Across streams, use attempt number and the defined state sequence to order a
job's transitions when UTC timestamps tie. Jobs are explicitly transient;
operational history lasts for the process/log lifetime and has no durable table
or recovery ledger.
This preserves the chosen TOML-only, memory-queue contract. External log retention
belongs to the process manager. Duplicate delivery IDs remain distinct jobs.

`max_retries = 3` means at most four attempts. The fixed `retry_interval_secs = 60`
delay starts after a failed attempt finishes, including shell cleanup. Command
spawn/wait/nonzero exit, HTTP transport/non-2xx and timeout failures retry;
cancellation, invalid metadata and internal task failures do not. A timeout
bounds each attempt, not the entire retry lifetime. Shutdown cancels retry waits
when its existing drain deadline expires; it does not wait indefinitely for
all retries or preserve them across restart.

## Server ownership

`server/admission` owns request body limits/deadlines, verification and admission.
A semaphore bounds simultaneous body buffers and blocking verification tasks;
its permit remains with the verification task if the request future is dropped.
Startup/storage lock acquisition is bounded. The running admission path uses
only the immutable snapshot. These bounds are lifecycle/resource safeguards,
not a throughput target.

`server/transport` owns the listener and a task set of HTTP/1 connections. Dropping
it closes connection tasks, including incomplete requests. The lifecycle
supervisor owns transport, runner and shutdown watcher handles. A failed runner
or listener triggers shutdown and an error result instead of leaving a live
server that cannot dispatch. Graceful shutdown closes admission before draining
the runner; forced cancellation settles active command groups.

## Trust boundaries

The server gives raw transport bytes to a provider. GitHub verifies HMAC-SHA256
with constant-time comparison before JSON decoding. The original body is retained
through execution. Provider errors return generic `400` without reflecting input.
GitHub event and delivery headers are metadata, not fields cryptographically
covered by the body signature. They never choose a target or modify a command.

Only private local configuration chooses shell source, cwd, HTTP destination and
outgoing credentials. Request content never becomes shell source or target URL.
Incoming signatures/secrets are not forwarded. HTTP redirect following is off,
and HTTPS uses certificate validation. Shell execution owns its process group
and stdin writer. Remaining descendants receive TERM then KILL on normal exit,
timeout or cancellation; dropping execution immediately kills the group.
Ordinary completion follows the shell's exit status after group cleanup. Failure
logs contain safe categories and numeric status codes, not raw target errors.
