# Dependency and license notices

Kicker's first-party code is licensed under MIT OR Apache-2.0. See LICENSE-MIT
and LICENSE-APACHE for the full terms. Dependencies retain their own licenses.

Cargo.lock fixes the Rust dependency graph, including the standards common crates
at `6852b210a9a614920bc5be2187f2705f46db978d` over public HTTPS Git. `kicker sbom`
prints the embedded CycloneDX document for the actual selected runtime components
and their declared licenses. Build-only `craftguild-cli-build` is not shipped as
a runtime component. These crates do not require a global standards command at
runtime. Review dependency notices for any later binary distribution.

The embedded Small Slant banner's attribution and generation provenance are in
[assets/README.md](assets/README.md). No FIGlet engine/font or Node.js package is
vendored or linked into Kicker.
