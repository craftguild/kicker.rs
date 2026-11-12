# Adding a webhook provider

Implement `kicker::provider::WebhookProvider` and register it through
`Providers::new(Vec<Arc<dyn WebhookProvider>>)`. The binary currently uses
`Providers::builtins()` with GitHub; add a built-in there or construct your own
registry when embedding the library. Update the CLI's supported-type help text
when adding a built-in. Rebuild the binary and restart to use it; there is no
dynamic plugin loader.

The trait methods are:

- `kind`: stable provider type used by `new --type` and TOML.
- `configure`: use `SetupUi::secret` and `SetupUi::text` to collect provider
  settings and return a `toml::Table`. Shared CLI code handles HTTP target headers.
- `validate_config`: reject unknown/missing/invalid provider settings without
  including their secret contents in errors.
- `verify`: inspect `WebhookRequest` (method, URI, headers, original `Bytes`) and
  return `VerificationOutcome` or `VerifyError`.

`VerificationOutcome::Dispatch(VerifiedPayload)` contains the body, content type,
event and delivery ID. Retain the original body bytes. Metadata must be suitable
for environment variables and HTTP headers. Do not put secrets in metadata.
`VerificationOutcome::Respond { status, body }` supports connection checks or
provider challenges that should not create jobs. The server does not special-case
GitHub ping handling.

The server validates provider settings once at startup and keeps that snapshot.
Changing verification settings or adding/removing hooks requires restart. Shared
registration code writes queue/retry defaults; providers do not prompt for those
settings. Hook URLs come from the configured base path, not the provider type.

Verification is synchronous and must not perform blocking network I/O. A future
provider needing remote key discovery should make that an explicit, bounded
design extension rather than blocking the HTTP executor. Never select shell
source or an HTTP destination from incoming request content.

Test through the trait registry and generic router. Cover the provider's published
signature vectors, tampering, malformed headers, raw-byte preservation, setup
cancellation and acknowledgement-only events. Tests can provide a fake `SetupUi`;
the terminal/end-to-end test also exercises the real `TerminalUi` implementation.

When embedding, `server::router` and `router_with_config` return `Result<Router>`
and load a configuration snapshot at construction. Use a fresh dispatcher and
router when applying a new snapshot so queue policies and admission settings
share one lifetime.
