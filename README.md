<h1 align="center">kicker.rs</h1>

<p align="center">
  <a href="https://github.com/Essk/ai-contribution-level">
    <img src="https://raw.githubusercontent.com/Essk/ai-contribution-level/main/badges/level-3.svg" alt="AI Contribution">
  </a>
</p>

kicker.rs verifies GitHub webhooks and sends the verified body to a configured shell
command or HTTP endpoint. It is a small Rust application for local
automation, with a private TOML configuration and independent in-memory queues
for each hook.

> [!WARNING]
> This project has not reached version 1.0 and may be unstable.
> Its behavior and specifications may change, and upgrades may require manual migration.
> Our team uses it in internal production systems to refine its design and implementation, but it may not meet your requirements.
> Please keep these limitations in mind when using it.

## Quick start

Release binaries are available for Linux x86_64 GNU and macOS arm64 from
[GitHub Releases](https://github.com/craftguild/kicker.rs/releases/tag/v0.1.0).
The installer needs POSIX sh, curl, and `sha256sum` or `shasum`. It verifies the
downloaded checksum and installs to `~/.local/bin` by default.

Install the CLI, then register a local command:

```sh
curl -fsSL https://raw.githubusercontent.com/craftguild/kicker.rs/v0.1.0/scripts/install.sh | sh -s -- --version 0.1.0
BIN="$HOME/.local/bin/kicker"
"$BIN" new --type github --call-cmd 'cat > received.json'
"$BIN" list
"$BIN" serve
```

The prompt generates a secret and shows it once, or accepts an existing secret
without echoing it. Registration prints a hook ID and its URL path. Configure
that path on your public HTTPS reverse proxy in GitHub's webhook settings, select
`application/json` and use the same secret. A valid non-ping delivery returns
`202` after admission and sends its original bytes to the configured command.
GitHub `ping` returns `200` without dispatching work.

Pass `--bindir /absolute/bin` after `--version 0.1.0` to choose another directory.
Use `--uninstall` instead of `--version 0.1.0` to remove the command; pass the same
`--bindir` for a custom destination. Add that directory to PATH for bare commands.
Each release includes target-specific `SHA256SUMS` files and a CLI SBOM.

## System requirements

Linux or macOS with `/bin/sh` and the programs used by your configured commands.
kicker.rs needs a private writable configuration directory, not a database or an
external queue. The compiled command needs no Rust, Python, Git or standards
installation at runtime.

kicker.rs serves HTTP on `127.0.0.1:8787` by default. Public GitHub delivery requires
an HTTPS reverse proxy configured separately. Outgoing HTTPS uses Rustls
certificate validation.

## Build requirements

Use Rust/Cargo **1.95.0**, a native linker, Make, Git, curl and Python **3.11+**.
The Rust 2024 package declares minimum Rust **1.95**. Cargo.lock fixes crates.io
dependencies and the common standards libraries. No Node.js or FIGlet tool is
needed.

Explicit `make setup` restores the repository-pinned standards **v0.6.2** engine on macOS arm64 or Linux x86_64 GNU. Normal build/check commands require that verified cache and prepared Cargo inputs; they do not download tools or update an engine. Checks additionally need rustfmt, Clippy and tokei **15.0.0**.

```sh
make check
make acceptance PROFILE=debug
make acceptance PROFILE=release
```

`check` retains the Rust product tests and existing pseudo-terminal/HTTP smoke.
Acceptance explicitly tests the receipt's built binary in Rust, including its
embedded SBOM and temporary copy-only install. See
[verification](docs/guides/verification.md) for commands and evidence boundaries.

Build the CLI from source after preparing the build tools:

```sh
make setup
cargo +1.95.0 fetch --locked
make
```

## Usage

```sh
kicker new --type github --call-cmd './deploy.sh' --cwd /absolute/project
kicker new --type github --call-http https://example.com/endpoint
kicker list
kicker delete <hook-id>
kicker serve --bind 127.0.0.1:8787
kicker sbom > kicker.cdx.json
```

Shell source, working directory, destinations and optional hidden HTTP header
values come from local configuration. Verified payload bytes are passed through
stdin or the HTTP body, never substituted into shell source. `list` omits secrets,
commands and destination credentials. Restart the server after configuration edits.

Configuration is `~/.kicker/config.toml`; `--home` overrides `KICKER_HOME`, which
overrides the default. The [configuration guide](docs/guides/configuration.md)
explains routes, retries, status codes and standalone operation. The
[provider guide](docs/guides/providers.md) covers library extension;
[documentation](docs/README.md) indexes the design and ADR.

Build output is `target/<target-triple>/<debug|release>/kicker`.
`PROFILE`, `TARGET` and `CARGO_TARGET_DIR` select the matching build/install
artifact. `make install BINDIR=/absolute/bin` only copies an existing verified
binary. `make release` is shared version/note preparation, not a build alias.
It does not itself publish a release.

## Known limitations

- Only GitHub JSON webhooks are built in. GitHub-side registration is manual.
- `202` acknowledges admission, not durable completion. Crashes can lose jobs.
- Retries and redeliveries can repeat effects; there is no delivery-ID deduplication.
- There is no job-status API, native TLS, Windows support or built-in service installer.
- Secrets are plaintext in a private TOML file. Existing files/directories must
  belong to the current user and deny group/other access.
- Shell descendants remain owned by the command's process group and are cleaned
  up on exit, timeout or cancellation; deliberate detachment is outside that lifecycle.

## Community

The source is [kicker.rs repository](https://github.com/craftguild/kicker.rs).
Read [AGENTS.md](AGENTS.md) before contributing. No dedicated support channel
or contribution policy is currently documented.

## Security

No private vulnerability-reporting route is currently documented. Do not publish
webhook secrets, payloads or outgoing credentials in a public report.

## License

First-party code is licensed under either the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.
[NOTICE.md](NOTICE.md) records dependency and banner provenance.
