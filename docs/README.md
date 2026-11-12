# kicker.rs documentation

Start with the [project README](../README.md) to build and register a hook.

## Guides

- [Configuration and operations](guides/configuration.md): private files, routes,
  status codes, retries, shutdown and foreground supervision.
- [Adding a provider](guides/providers.md): verification/setup traits and library use.
- [Build and verification](guides/verification.md): required local commands and
  actual Rust artifact acceptance, with explicit qualification limits.

## Design

- [Design decisions](design/design.md): configuration ownership, transient job
  transitions/history, admission and child-process lifetimes.
- [Initial review](design/review-v1.md): historical findings and fixes; this does
  not qualify changed source.

## Architecture decisions

[ADR 0001](adr/0001-standards-v0.5.md) fixes standards v0.6.1, Rust 1.95 and the
local completion boundary. Its number and Accepted status are preserved.
Read [AGENTS.md](../AGENTS.md) before implementation. Beads owns current tasks,
check evidence and independent re-review; design documents are not task ledgers.
