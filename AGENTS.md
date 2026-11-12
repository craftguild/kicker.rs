<!-- craftguild:common:start -->
Use Craftguild standards 0.1.0 with the fixed engine in standards/engine.json.
Run `make prime` (or `python3 standards/launcher.py prime`) before adoption.
Read relevant topics with `make prime PRIME_ARGS="--topic NAME"`.
Follow the repo-local craftguild-standardize Skill. Run `make check`.
Update explicitly with `craftguild-standards init [--force]`; follow its dependency revision guidance.
<!-- craftguild:common:end -->

## Shared agent files

`AGENTS.md` is the repository instruction authority. The two shared skills in
`.agents/skills/` cover Beads and the pinned Craftguild entry. Tool-specific
`.claude/`, `.codex/`, `.cursor/` and editor settings are local and ignored.
Beads data and configuration stay entirely local in ignored `.beads/`; do not
publish or synchronize them. Run `bd prime` for the local task workflow.

# kicker.rs product guidance

Read docs/design/design.md, docs/guides/configuration.md and docs/adr/0001-standards-v0.5.md.
Use Rust 1.95.0 for normal builds, tests and CI. Keep config-v1, provider and raw
payload contracts, per-hook FIFO/retry transitions, private config and Unix
process-group ownership unchanged. Operational logs stay JSON on their existing
stdout/stderr channels. New or changed product scenarios/assertions belong in
Rust; scripts only arrange the environment and invoke commands. Unchanged legacy
terminal smoke remains scoped to the existing scenarios.

Standards owns generic build/receipt/failure-matrix tests. Keep product domain,
process and actual copied-artifact checks here. Run make check and explicit make
acceptance for the intended profiles. Do not use an ignored ordinary test result
as artifact acceptance. Use Beads for current task/check/review evidence; no new
Markdown task or review ledger. Historical reviews are limited to their source.
Use engineering-principles and the implementation-only code-size skill; tests,
test-only helpers and design documents are excluded from Code400.

Preserve existing staged content. Git staging, commits, tags, pushes, releases,
installation into the real home and existing service operations require active
user authorization. Pause all goal work on an actual OpenPGP signing or
verification failure. The active request defines the local handoff boundary.
