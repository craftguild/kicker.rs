# Initial production v1 self-review (historical)

This records the first review, not the current operating contract. Later changes
added per-hook retries, restart-only configuration, configurable root-based URLs
and severity-based log streams. The original findings and measurements below
are retained as historical evidence. Use [configuration](../guides/configuration.md),
[design](design.md) and [verification](../guides/verification.md) for current behavior
and the scope of later validation.

Reviewed on 2026-11-03. Scope: the CLI, TOML storage, GitHub verification,
HTTP admission, job scheduling, shell/HTTP execution and shutdown. The review
focuses on correctness, ownership, responsibility boundaries and maintenance.
No throughput or latency target was set, and no benchmark claim is made.

## Findings and fixes

| Priority | Finding | Resolution and evidence |
| --- | --- | --- |
| P1 | Normal shell exit left background children alive. Dropping execution killed only the shell and detached the stdin writer. | Execution owns a process-group guard and stdin-task guard. Normal exit settles descendants with TERM/KILL; unexpected drop immediately kills the group and aborts the writer. Two regressions cover normal exit and dropped execution; the existing timeout test covers TERM-resistant children. |
| P1 | Blocking config locks and incomplete HTTP requests could outlive shutdown. Request bodies were buffered before an admission bound. | Lock acquisition is bounded and cancellation-aware. Request permits cover body reading through verification; body/header reads have deadlines. HTTP transport owns all connection tasks. Admission tests cover timeout, saturation, cancellation and incomplete native connections. The release smoke test holds an external config lock through SIGTERM and process exit. |
| P1 | The server did not supervise scheduler completion, so it could keep listening after execution stopped. | One supervisor owns HTTP, scheduler and shutdown-watcher tasks. Unexpected completion fails the server and closes admission; dropping the supervisor cancels its children. Five supervisor tests cover clean/unexpected exits and watcher failure. |
| P1 | Removing a registered working directory invalidated every hook. An invalid registration could not be deleted because validation ran before mutation. | Registration checks directory existence; later missing-directory failures belong to that job. Reads still validate configuration structure. Edits validate the resulting snapshot, allowing deletion to repair an invalid hook. Two regressions cover isolation and repair. |
| P2 | A directly constructed dispatcher accepted zero concurrency, and cloned dispatchers could start multiple schedulers. | Constructor validation is shared with TOML validation. One runner per dispatcher lifetime is enforced. Runner-drop cleanup closes admission and releases accounting. Tests cover invalid limits, duplicate runners and dropped runners. |
| P2 | Jobs retained full registration data, including provider verification secrets. | `Job::new` builds a private, immutable execution snapshot containing only target settings, timeout, identity and verified payload. Provider settings stay in configuration/verification. Existing live-deletion tests preserve accepted-job snapshot behavior. |
| P2 | Execution failures were all logged as `failed`, with transition ownership spread across tasks. | The scheduler owns completion accounting and transitions. Typed outcomes expose safe reasons, exit/signal codes, HTTP status and I/O kind without raw errors or target URLs. Each transition has a UUID, job identity, previous/new states and UTC time. The native smoke test checks transition chains, exit-code diagnostics and redaction. |

Five regression tests were first run against the original implementation and
failed: missing-directory isolation, deletion repair, normal-exit child cleanup,
dropped-execution cleanup, and invalid scheduler limits. The HTTP/supervision
findings also came from inspecting ownership and cancellation paths, then adding
focused tests; they were not all reproduced against the original implementation.

## Responsibility assessment

| Boundary | Owner | Reason for the boundary |
| --- | --- | --- |
| CLI and interactive setup | `cli`, `provider/setup` | Argument interpretation and terminal UI stay outside serving/execution. |
| Configuration data and persistence | `model`, `validation`, `config` | Structural rules are reusable; private locking, decoding and atomic replacement stay together. |
| Service-specific verification | `provider` trait and `provider/github` | Adding a provider does not alter HTTP admission or targets. |
| Execution snapshot | `job` | Execution cannot mutate registration data or retain verification secrets. |
| HTTP request admission | `server/admission` | One operation owns body limits, verification, cancellation and the `202` decision. |
| Listener and connection ownership | `server/transport` | All native HTTP connection tasks share a cancellation/drop boundary. |
| Server lifecycle | `server/lifecycle` | One supervisor owns drain, failure propagation and task cleanup. |
| Queue invariants and history | `queue/state` | Capacity, FIFO selection and transitions change under one mutex. |
| Scheduler task ownership | `queue` | One runner starts/joins work and commits completion. |
| Target execution | `dispatch/http`, `dispatch/shell`, `dispatch/process` | HTTP transport policy is separate from shell process/stdin ownership and group cleanup. |

The splits follow operations and resource lifetimes. Configuration storage stays
in one file because its lock/read/validate/write sequence forms one consistency
boundary. There is no generic workflow framework, repository abstraction or
dynamic plugin loader to maintain.

The design baselines are recorded in [design decisions](design.md). Registrations
are a TOML configuration registry, without business line items or a durable
lifecycle. Jobs are explicitly transient. Their state transitions and operational
history last for the process/log lifetime; no database or durable job ledger is
introduced. This preserves the chosen memory-only queue contract.

Tokei 15.0.0 measured all 20 implementation files. The largest is `config.rs`
at 175 code lines. Tests and design documents are excluded; the lifecycle unit
tests live under `tests/unit`. All implementation files are below 400 code lines,
and each measured file has a defined responsibility. Size alone is not the
readiness criterion.

## Compatibility and operating limits

At the time of this review, CLI commands, the provider trait, hook paths and
original-body forwarding remained compatible. New request settings had defaults,
allowing existing v1 TOML files to load.
An incomplete body can now return `408`, and concurrent body/verification work
can return `503`. Remaining background processes are terminated on shell exit;
commands must finish required work before returning.

For library callers, construct jobs with `Job::new` instead of mutable struct
literals, handle the `Result` returned by `Dispatcher::run`, and match typed
failure outcomes. `server::router` remains available; `router_with_config` adds
request settings and a shutdown token for embedded use.

The contract at the time included a volatile queue, no automatic retries or delivery
deduplication, no job-status API, local TOML secrets, manually configured GitHub
webhooks and HTTPS termination at a reverse proxy. A `202` acknowledges admission,
not durable completion. Remote HTTP work and deliberately detached processes
cannot be undone by cancelling a job. These are documented product limits,
not guarantees added by this review.

The review found no unresolved correctness or responsibility issue blocking an
initial self-hosted deployment under that contract. This conclusion applies to
the reviewed version; the current [verification](../guides/verification.md) records later
coverage. Public TLS routing and real GitHub delivery were not exercised.
