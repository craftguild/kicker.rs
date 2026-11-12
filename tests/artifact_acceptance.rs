//! Acceptance of verified shipped bytes, explicitly selected by make acceptance.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

#[derive(PartialEq, Eq)]
enum Entry {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, current: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let value = if kind.is_symlink() {
                Entry::Link(fs::read_link(&path).unwrap())
            } else if kind.is_dir() {
                visit(root, &path, entries);
                Entry::Directory
            } else {
                Entry::File(fs::read(&path).unwrap())
            };
            entries.insert(path.strip_prefix(root).unwrap().to_owned(), value);
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

fn command(binary: &Path, runtime: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .current_dir(runtime)
        .env_clear()
        .env("PATH", "")
        .env("HOME", runtime.join("home"))
        .env("KICKER_HOME", runtime.join("home"))
        .env("GIT_DIR", runtime.join("no-git"))
        .env("HOST", "unrelated-host")
        .stdin(Stdio::null());
    command
}

fn succeeded(output: &Output, arguments: &[&str]) {
    assert!(
        output.status.success(),
        "{arguments:?}: status={}, {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(binary: &Path, runtime: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = command(binary, runtime, arguments).output().unwrap();
    succeeded(&output, arguments);
    assert!(
        output.stderr.is_empty(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn variable(name: &str, value: impl AsRef<OsStr>) -> OsString {
    let mut argument = OsString::from(name);
    argument.push("=");
    argument.push(value);
    argument
}

fn assert_installation(root: &Path, binary: &Path, stage: &Path, context: &Value) {
    let mut install = Command::new("make");
    install
        .current_dir(root)
        .arg("install")
        .arg(variable("DESTDIR", stage))
        .arg("PREFIX=/opt/craftguild")
        .arg(variable("PROFILE", context["profile"].as_str().unwrap()))
        .arg(variable(
            "CARGO_TARGET_DIR",
            context["target_dir"].as_str().unwrap(),
        ));
    if context["explicit_target"] == true {
        install.arg(variable("TARGET", context["target"].as_str().unwrap()));
    }
    let installed = install.output().unwrap();
    succeeded(&installed, &["make", "install"]);
    assert!(
        fs::read(stage.join("opt/craftguild/bin/kicker")).unwrap() == fs::read(binary).unwrap(),
        "Installed bytes differ from the verified artifact"
    );
}

#[test]
#[ignore = "make acceptance supplies the verified engine-built artifact and selected profile"]
fn copied_cli_preserves_offline_metadata_and_installation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let path = std::env::var_os("KICKER_ACCEPTANCE_RECEIPT").expect("Run make acceptance");
    let receipt: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let context = &receipt["context"];
    let prepared = &receipt["prepared"];
    let build = &prepared["build"];
    let binary = Path::new(context["output"].as_str().unwrap());
    let expected_sbom = fs::read(root.join(prepared["sbom"].as_str().unwrap())).unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().canonicalize().unwrap();
    let runtime = directory.join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::create_dir(runtime.join("home")).unwrap();
    let copied = runtime.join("kicker");
    fs::copy(binary, &copied).unwrap();
    fs::write(
        runtime.join("home/config.toml"),
        "invalid product configuration [",
    )
    .unwrap();
    fs::write(runtime.join("craftguild.toml"), "invalid configuration [").unwrap();
    let before = snapshot(&runtime);
    let banner = fs::read(root.join("assets/banner.txt")).unwrap();
    assert!(run(&copied, &runtime, &[]).starts_with(&banner));
    assert!(
        std::str::from_utf8(&run(&copied, &runtime, &["--help"]))
            .unwrap()
            .contains("craftguild/kicker.rs")
    );
    assert!(
        !std::str::from_utf8(&run(&copied, &runtime, &["serve", "--help"]))
            .unwrap()
            .contains(std::str::from_utf8(&banner).unwrap())
    );
    let version = run(&copied, &runtime, &["--version"]);
    let human = std::str::from_utf8(&version).unwrap();
    assert!(human.contains(&format!("kicker {}", build["version"].as_str().unwrap())));
    for field in ["commit", "compiler", "target", "profile"] {
        assert!(
            human.contains(build[field].as_str().unwrap()),
            "Human version lacks {field}"
        );
    }
    assert!(
        run(&copied, &runtime, &["sbom"]) == expected_sbom,
        "Embedded SBOM bytes differ"
    );
    let sbom: Value = serde_json::from_slice(&expected_sbom).unwrap();
    assert_eq!(sbom["bomFormat"], "CycloneDX");
    let properties = sbom["metadata"]["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["name"].as_str().unwrap(), p["value"].as_str().unwrap()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        serde_json::from_str::<Value>(properties["craftguild:build"]).unwrap(),
        *build
    );
    let components = sbom["components"].as_array().unwrap();
    for name in ["axum", "reqwest", "hmac", "tokio"] {
        assert!(components.iter().any(|c| c["name"] == name));
    }
    assert!(
        !components
            .iter()
            .any(|c| c["name"] == "craftguild-cli-build")
    );
    for arguments in [&["--help"][..], &["--version"][..], &["sbom"][..]] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let output = command(&copied, &runtime, arguments)
            .stdout(Stdio::from(writer))
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "Closed stdout {arguments:?}");
    }
    assert!(
        snapshot(&runtime) == before,
        "Offline metadata commands changed runtime files"
    );

    assert_installation(&root, binary, &directory.join("stage"), context);
}
