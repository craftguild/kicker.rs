# Adopt the fixed standards v0.5.0 engine

Status: Accepted for local integration, 2026-11-08.

## Decision

Use standards v0.5.0 at
`69ef0ba960e4d61bff3f363c07f8d49c4d53d77e`. The initializer only creates the
fixed reference and thin entries; explicit `make setup` obtains its full engine.
Normal commands use that cached engine without downloading or updating it.
Keep normative baseline 0.1.0, config schema 2, reference schema 3 and the
entry/execution protocols separate from the engine release version.

Kicker remains one package and one executable, version 0.1.0, with Rust 1.95.0
for builds, tests and CI and a declared minimum of 1.95. The shared CLI,
build library and transitive core use the same full standards revision. The
engine owns build context, dependency selection, embedded identity/SBOM,
compilation, receipts, installation, installers, packaging and release rules.
Make and CI dispatch to the repository's fixed entry. No product build hooks,
copied rule bundle, loader fallback or common build failure matrix are needed.

Native and cross outputs follow
`<target-dir>/<target-triple>/<debug|release>/kicker`. `make` builds release;
`make release` is the shared release operation, no longer a build alias.
The package manifest is the only version authority; Cargo.lock and `v` tags
belong to its release group. No version or release notes are applied here.

Root help, bare invocation and version use the shared offline CLI formatting.
Add `sbom` for the exact embedded CycloneDX document. These paths complete before
Tokio, configuration, logging or serving. Keep the existing banner; subcommand
help, parser errors, list/setup output and operational JSON logs stay unbranded.
Output/flush failures return a failure status.

## Product boundaries

Keep config-v1, private atomic TOML, provider setup and HMAC validation, original
payload bytes, queue capacity, retries and Unix process-group ownership unchanged.
The existing [design](../design/design.md) defines transient job transitions and their
process-lifetime structured history. Hook registrations remain a configuration
registry. This build change adds neither durable business state nor business
line items; no database or Header-Detail redesign applies.

New artifact acceptance scenarios and assertions live in Rust and execute the
receipt's actual copied executable, including temporary copy-only installation.
Scripts only arrange tools/environment and invoke tests. Existing unchanged
Python terminal smoke scenarios retain their historical scope; this decision
does not start a repository-wide test rewrite. Keep the Rust domain and process
tests. Standards owns generic build and receipt regression matrices.

## Local completion and later publication

Local completion requires: exact pin/managed entries/common crates; connected
Make/CI/quality/release; source-only export with explicit setup, offline prime
and an actual build; debug/release artifacts, exact SBOM, offline metadata and
temporary install; final-source product checks; English guidance, dependencies,
license status, installer freshness and implementation Code assessment; evidence
in Beads; and the authorized operation boundary. Tests and design documents are
excluded from the 400 implementation Code-line limit.

Beads owns current task, check and review evidence. Historical reviews are not
current-source verification. Extra hosts, hosted CI, real GitHub deliveries and
public HTTPS deployment require separate evidence. The repository currently
declares no license; preserve that status pending the owner's decision and do
not infer a license grant from dependencies or public visibility. There is no
confirmed private vulnerability-reporting route.

This integration does not stage, commit, tag, publish, release, install into the
real home or operate an existing service. An actual signing or verification
failure pauses all goal work. The active user request defines the local handoff boundary.

## Published v0.6.0 adoption amendment (2026-11-11)

The user authorized the standards authentication release and explicit adoption
for this initial CLI delivery. Upgrade through the published v0.6.0 initializer
to full revision `72cf076962dc8401b2e4d1d84933c8a818ff390c`, then align the
common Rust Git declarations and lockfile with that same revision. This replaces
the v0.5.0 pin in the current identity/completion criteria above; earlier decisions,
results and actual generator-producer records keep their original provenance.
Normative baseline 0.1.0, reference schema 3, configuration schema 2 and both
compatibility protocols remain unchanged. Rust 1.95.0 and all product gates stay
required. Init preserves product recipes, configuration and local task state.

The shared release removes its implicit aggregate command deadline while retaining
explicit watchdogs, cancellation and owned-child cleanup. This does not establish
a diagnosis for an earlier hosted-runner shutdown. Requalify the current product
source, actual artifacts and native CI rather than reuse older-source passes.
Ship the CLI for Linux x86_64 GNU and macOS arm64 through CI-built Release assets,
with target checksums and SBOMs, a generated installer and README instructions.
Verify real download/install/uninstall in owned temporary destinations.

The public CLI installer retains the default curl transport. CLI delivery does
not publish npm/crates registry packages or qualify additional SDK/OS payloads.

The active release request separately authorizes signed commits/tags, pushes and
GitHub publication. Preserve the original local-integration boundary as history;
this amendment does not claim any unexecuted qualification or introduce a business
lifecycle, transition-history table or Header-Detail model.

## 2026-11-11 runner and license amendment

The owner authorized WarpBuild runners for every workflow, including native CLI
delivery and installed-asset acceptance. Linux uses Ubuntu 24.04 x86_64 and
macOS uses macOS 26 arm64 on ephemeral WarpBuild VMs. This replaces earlier
GitHub-hosted-only restrictions; explicit tool provisioning, isolated processes,
product gates and deadlines remain in force. No local service is provisioned by
this decision. First-party code is now MIT OR Apache-2.0, at the recipient's
option; third-party licenses and historical producer observations retain their
original scope.

## 2026-11-11 renamed standards release

The owner authorized adoption of published `craftguild/standards.rs` v0.6.1 at
`6852b210a9a614920bc5be2187f2705f46db978d`. The actual published initializer re-created the
managed entry after its previous owned bytes were verified and archived.
The engine and common Cargo crates use that same full revision. Product
configuration and instructions remain intact. No old repository-name fallback
is provided. The normative baseline stays 0.1.0; product Rust stays 1.95.

## 2026-11-13 standards v0.6.2 release preparation

Use published standards v0.6.2 at `be63eefb44a850a01f4cc065c66fff2fe558ee18` through the verified initializer.
The fixed engine reference and common Cargo Git dependencies select that same full revision, with rules baseline 0.1.0 and product Rust 1.95.
This amendment aligns the shared release preparation across CraftGuild products while preserving earlier decisions and historical producer observations.
