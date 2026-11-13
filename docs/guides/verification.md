# Build and verification

Use Rust 1.95.0, Make, Python 3.11+, a native linker, rustfmt, Clippy and tokei 15.0.0. The repository fixes standards v0.6.2 at `be63eefb44a850a01f4cc065c66fff2fe558ee18`. Its init-only command is optional for a checkout that already contains the reference and entries. Explicitly run `make setup`, then `cargo +1.95.0 fetch --locked`; these are the preparation steps allowed to obtain the engine and locked Cargo inputs. Ordinary operations use the existing cache and do not install dependencies or toolchains.

## Required local commands

```sh
make prime
make check
make acceptance PROFILE=debug
make acceptance PROFILE=release
make installer INSTALLER_ARGS=--check
```

`check` builds the native debug artifact and executes the existing Rust domain,
provider, queue, dispatch, process/lifecycle and log-routing tests, the unchanged
Python pseudo-terminal/HTTP smoke test, rustfmt and Clippy. It also checks the
shared policies, source roles, installer freshness and implementation Code sizes.
Tests and actual test-only helpers are excluded from implementation Code400.

`acceptance` builds the selected profile, verifies the common receipt, and runs
`tests/artifact_acceptance.rs` explicitly through Cargo. Its ignored ordinary
Cargo result is not acceptance: the explicit run must pass with no ignored case.
It executes a copy of the receipt's real binary with an empty PATH, temporary
HOME and malformed local configuration; checks root/subcommand branding, version
identity and exact embedded CycloneDX bytes/components; verifies closed-stdout
failure and no metadata filesystem writes; and compares a temporary copy-only
installation with the built bytes. Product assertions and fixture data are Rust.
The small Python dispatcher only selects the verified receipt/environment and
invokes Cargo, then verifies the receipt again.

`make` selects release; `make PROFILE=debug` selects debug. All native/cross output
uses `target/<target-triple>/<debug|release>/kicker`, or the corresponding
`CARGO_TARGET_DIR`. There is no `target/release` fallback. Existing legacy smoke
can also be invoked with an explicit actual binary:

```sh
python3 tests/smoke.py --binary target/aarch64-apple-darwin/release/kicker
```

Replace the triple with the host being tested. Standalone operation and stream
ownership are described in [configuration](configuration.md#running-and-supervising-kickerrs).
No process-manager checkout or running service is a build/test prerequisite.

## Evidence and boundaries

Beads owns current source, command, receipt, platform and review evidence.
Pre-adoption results, including older Linux qualification, retain their historical
scope and do not qualify changed source.
The [initial review](../design/review-v1.md) is likewise historical.

Tests use temporary directories, synthetic credentials, loopback listeners and
owned child processes. They do not modify GitHub settings or existing services.
Public HTTPS routing, actual GitHub delivery, additional OS/hosts, hosted CI and
independent review require separate evidence. No production throughput benchmark
is asserted. Real release, signing, clean-HEAD packages, publication and install
into the actual home/system are outside this local integration.

Project licensing and a private vulnerability-reporting route remain unspecified.
The [adoption decision](../adr/0001-standards-v0.5.md) preserves existing config-v1,
HMAC/raw-byte, transient queue/history, retry and process-group contracts.
