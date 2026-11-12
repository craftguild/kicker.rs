//! Embed the common verified identity and SBOM without recursive Cargo execution.
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("package")?);
    craftguild_cli_build::emit(&root, "kicker")?;
    Ok(())
}
