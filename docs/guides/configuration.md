# Configuration and operations

The default home is `~/.kicker`. Choose another with `--home PATH` or
`KICKER_HOME`; a CLI flag takes precedence. The application creates a private
home when needed. `new` writes `config.toml`. Reads/edits use `config.lock`;
the server also holds `serve.lock`. There is no database, payload spool or
persistent job state. Only one server may use a home at a time.

Use [`config.example.toml`](../../config.example.toml) as a field reference.
Registration through `new` is preferred. When copying the example, choose an
existing absolute working directory, replace the demonstration secret, and use
directory/file modes `0700`/`0600`. Symlink homes/configs and non-private or
foreign-owned files are rejected.

## Server settings

`version = 1` selects the configuration format. `[server]` has these defaults:

| Field | Default | Meaning |
| --- | --- | --- |
| `bind` | `127.0.0.1:8787` | HTTP listener; `serve --bind` overrides it |
| `base_path` | `/` | Webhook prefix; `/` gives `/<hook-id>`, `/hooks` gives `/hooks/<hook-id>` |
| `max_payload_bytes` | `26214400` | Maximum body bytes per request (25 MiB) |
| `max_buffered_bytes` | `67108864` | Body bytes per hook, including active/retrying work (64 MiB) |
| `shutdown_timeout_secs` | `30` | Drain time before cancelling remaining work |
| `max_in_flight_requests` | `4` | Requests reading bodies or verifying/admitting |
| `body_timeout_secs` | `10` | Separate HTTP header and body read deadlines |

All settings are read once at startup. **Restart the server after any edit,
including `new`, `delete` or its `remove` alias.** A running process retains its
startup configuration, even if the file is changed, deleted, malformed or locked.
The CLI prints a restart reminder after registration/deletion.

Readers hold a shared config lock; CLI updates use an exclusive lock and atomic
replacement. Lock acquisition times out after five seconds. Invalid config is
rejected on the next startup. Unknown fields, duplicate/nil IDs, unsupported
providers and invalid targets are rejected without printing secret values.
`delete` validates the resulting snapshot and can remove an invalid hook from
an otherwise valid, parseable configuration. Syntax/type errors and unknown
fields require manual correction.

## Webhook URLs

For example, to serve a dedicated webhook domain directly at its root:

```toml
[server]
base_path = "/"
```

Set `base_path = "/hooks"` to retain the former `/hooks/<hook-id>` URLs.
Prefixes can contain multiple static segments, such as `/api/events`. A trailing
slash is ignored. Segments allow ASCII letters, digits, `-`, `.`, `_` and `~`;
empty segments, `.`/`..`, query strings and router capture syntax are rejected.

`list` and the path printed by `new` always use the configuration file, not
the running process. After a base-path edit, they immediately show the new path
while the service keeps listening on its old path until restart. Update the
sender's webhook URL when applying a path change, or set `/hooks` to preserve it.

With a public origin of `https://hooks.example.com`, the default delivery URL is
`https://hooks.example.com/<hook-id>`. Forward the configured path and original
body unchanged through your HTTPS reverse proxy. See GitHub's
[signature validation](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries)
and [delivery best practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks).

## Hook settings

Each `[[hooks]]` entry has `id`, `type`, a provider-owned `[hooks.verification]`
table, a `[hooks.target]` table, and these execution settings:

| Field | Default | Meaning |
| --- | --- | --- |
| `timeout_secs` | `300` | Deadline for each execution attempt |
| `queue_size` | `256` | Waiting deliveries for this hook; one active/retrying delivery is separate |
| `retry_interval_secs` | `60` | Fixed delay after failure before the next attempt |
| `max_retries` | `3` | Additional attempts after the initial attempt; `0` disables retries |

`new` writes all defaults to TOML without additional prompts. Queue size, retry
interval and attempt timeout must be positive. Omitted fields in old files use
the defaults. Legacy `[server].max_jobs` and `max_parallel_hooks` are accepted
but ignored and omitted on the next CLI write; queue policies now belong to
hooks. `max_buffered_bytes` now applies independently to each hook.

- `kind = "cmd"`: `command` is passed to `/bin/sh -c`, with absolute `cwd`.
- `kind = "http"`: `url` is an HTTP(S) URL without userinfo or fragment. Optional
  `[hooks.target.headers]` entries contain header values. Query parameters are
  allowed. Header names are case-insensitive and duplicates are rejected.

The working directory must exist at registration. If it later disappears,
execution fails for that hook with a `spawn` reason; other hooks remain usable.

Command environment and HTTP metadata headers:

| Environment | HTTP header |
| --- | --- |
| `KICKER_HOOK_ID` | `X-Kicker-Hook-Id` |
| `KICKER_PROVIDER` | `X-Kicker-Provider` |
| `KICKER_EVENT` | `X-Kicker-Event` |
| `KICKER_DELIVERY_ID` | `X-Kicker-Delivery-Id` |
| `KICKER_CONTENT_TYPE` | `X-Kicker-Content-Type` |

The original signature and other incoming headers are not forwarded. The raw
body is unchanged, including whitespace, Unicode and trailing newlines.

## HTTP responses

| Status | Meaning |
| --- | --- |
| `202` | Verified and admitted; execution is asynchronous |
| `200` | Verified GitHub `ping`; no job started |
| `400` | Invalid/missing/duplicate verification headers, signature or JSON |
| `404` | Unknown or malformed hook ID/path |
| `405` | Unsupported method on a hook route |
| `408` | Body was not fully received before its read deadline |
| `413` | Request body exceeds the configured limit |
| `503` | Request slots full, this hook's queue/byte budget full, or shutdown in progress |

GitHub validation requires `X-Hub-Signature-256`, `X-GitHub-Event`,
`X-GitHub-Delivery` (a UUID), and `application/json` (parameters such as charset
are accepted). JSON must be an object. All non-ping events are dispatched;
configure event subscriptions at GitHub.

## Execution and shutdown

### Queues and retries

Each hook has an independent FIFO queue and processes one delivery at a time.
Retries retain that hook's active slot, preserving order. Other hooks can run
concurrently, without a shared job-count or hook-concurrency cap. No queued work
is silently evicted. A full hook queue/byte budget returns `503` before `202`.
A request rejected this way is not saved and does not enter Kicker's retry loop;
it must be sent again by the sender.

Nonzero command exits, spawn/wait failures, HTTP transport errors, non-2xx
responses and timeouts retry. The delay starts after the attempt and cleanup
finish. The default is three retries, for at most four total attempts. Any 2xx
response or successful command ends retries. Every attempt uses the same job ID,
raw body, target and metadata. Cancellation and internal failures are not retried.
Retries can repeat side effects; handlers must implement any needed idempotency.

### Shell commands

`--call-cmd` is one shell string passed to `/bin/sh -c`. Kicker does not split it
into an argument array or check shell syntax at registration. Quote it for the
shell where you run `new`; pipes, redirects, quoting and variables inside that
string follow `/bin/sh` semantics at execution time.

The shell's final exit status determines success. For example, `false | cat`
can return zero even though an earlier pipeline command failed. Kicker does not
automatically enable `set -e` or `pipefail`. A syntax error is an execution
failure and follows the configured retry policy.

Stdin writing and child waiting happen concurrently; a zero exit succeeds even
if the command ignores stdin. Shell children get a separate process group.
After any shell exit, remaining group members receive TERM, then KILL after
five seconds if needed. The job keeps its hook slot until cleanup completes,
including on success. Cleanup can take up to five seconds after an execution
timeout. Dropping execution immediately kills its group and aborts its stdin
writer. Background children are not independent jobs.

### HTTP forwarding

HTTP connection setup has a five-second timeout. The hook's timeout applies to
each request through receipt of response headers; response bodies are not read.
Any 2xx status succeeds, and redirects are not followed. The static destination
and outgoing headers come from local configuration, not the incoming payload.

### Shutdown

SIGINT/SIGTERM stops new admission and drains accepted jobs, including retry
waits. After the drain deadline, pending jobs are cancelled, retry waits are
interrupted and active HTTP requests are dropped. Running command groups receive
TERM and, if still present after five seconds, KILL. The shell is reaped.
An HTTP recipient may continue work already received; cancelling the client
cannot undo remote effects. Commands that deliberately detach from their process
group are outside this lifecycle.

The HTTP listener, its connections, the scheduler and shutdown watcher have one
supervisor. Unexpected scheduler/listener exits stop the server with an error
so a process manager can restart it. Incomplete bodies and HTTP connections
are closed during shutdown. Admission has no config-lock waits.

## Logs

Kicker's operational records are JSON with INFO/WARN on stdout and ERROR on
stderr. Failed attempts, failure-driven retry waits and timeouts are ERROR;
normal lifecycle transitions and shutdown cancellation are INFO. TRACE/DEBUG
would route to stdout, but are disabled by the fixed `kicker=info` filter.
`RUST_LOG` does not override that filter.

Transition records include UTC time, a transition UUID, job/hook/delivery IDs,
attempt number and previous/new states (including `retry_waiting`). Failures
include a safe reason and, where available, exit code, signal, HTTP status or
I/O error kind. Kicker's own records do not include bodies, secrets, command
strings, destination URLs or authentication values.

Child stdout/stderr are inherited unchanged, without automatic hook/job ID
prefixes or secret masking. Both streams can contain child text alongside
Kicker's JSON. CLI results, setup prompts and startup/parser errors are also
plain text. There is no per-request access log for signature rejection or full
queues; use the HTTP response to identify rejected deliveries.

kicker.rs does not save or rotate log files itself. The operator owns stream
capture, file permissions and retention. A supervisor or shell redirection can
capture stdout/stderr; preserve their existing routing and remember that child
output is passed through unchanged.

## Running and supervising kicker.rs

Use the binary from your source build or installation. After registering hooks
with the same private configuration home, run it in the foreground (this example
assumes `kicker` is on PATH):

```sh
kicker --home /absolute/private/kicker serve --bind 127.0.0.1:8787
```

Use the same `--home` for `new`, `list`, `delete` and `serve`. Alternatively,
explicitly pass `KICKER_HOME` to the process. Without either setting, configuration
is under that account's `~/.kicker`. Run only one server per configuration home;
its hooks and queues belong to that process.

Route the public HTTPS webhook path to the loopback listener with your separately
configured reverse proxy. The command runs independently of a particular process
manager. Automatic restart, boot startup, log capture and retention belong to an
operator-selected supervisor; kicker.rs has no built-in service installer.

SIGINT/SIGTERM begins the configured drain. For the default 30-second drain,
allow at least 45 seconds before an external supervisor forces termination:
up to five seconds of owned process-group cleanup, HTTP closure and margin are
additional to the drain. Increase that budget when increasing
`server.shutdown_timeout_secs`. Retry waits are part of the drain and are
cancelled when its deadline expires. Accepted work is not persisted across restart.

After changing the binary or configuration, stop the foreground server and start
it again with the same home. The live process keeps its original validated
configuration snapshot. Keep shell-child process-group cleanup with kicker.rs;
external supervision should manage the server rather than sweep unrelated
processes by name.
