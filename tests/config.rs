mod common;
use clap::Parser;
use common::*;
use kicker::{
    cli::{Cli, register},
    model::{Secret, Target},
    provider::SetupUi,
    validation,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::Arc,
};

#[test]
fn atomic_private_configuration_survives_concurrent_updates_and_failures() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(store(root.path()));
    assert!(store.read().unwrap().hooks.is_empty());
    assert!(!store.home.exists());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let hook = hook(root.path(), "true");
            std::thread::spawn(move || store.add(hook).unwrap())
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(store.read().unwrap().hooks.len(), 8);
    assert_eq!(
        fs::metadata(&store.home).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let path = store.home.join("config.toml");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let existing = store.read().unwrap().hooks.remove(0);
    let before = fs::read(&path).unwrap();
    assert!(store.add(existing.clone()).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    store.delete(existing.id).unwrap();
    assert!(store.delete(existing.id).is_err());
    let guard = store.serve_lock().unwrap();
    assert!(store.serve_lock().is_err());
    drop(guard);
    assert!(store.serve_lock().is_ok());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read().is_err());
}

#[test]
fn malformed_configuration_and_symlinks_do_not_expose_or_overwrite_secrets() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    store.ensure_home().unwrap();
    let path = store.home.join("config.toml");
    fs::write(&path, format!("secret = \"{SECRET}\"\ninvalid = [")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let error = store.read().err().unwrap();
    assert!(!format!("{error:#}").contains(SECRET));
    fs::remove_file(&path).unwrap();
    let victim = root.path().join("untouched");
    fs::write(&victim, "untouched").unwrap();
    symlink(&victim, &path).unwrap();
    assert!(store.add(hook(root.path(), "true")).is_err());
    assert_eq!(fs::read_to_string(victim).unwrap(), "untouched");
}

struct Ui {
    cancelled: bool,
}
impl SetupUi for Ui {
    fn secret(&mut self, _: &str) -> anyhow::Result<String> {
        Ok(SECRET.into())
    }
    fn text(&mut self, _: &str) -> anyhow::Result<String> {
        unreachable!()
    }
    fn header(&mut self) -> anyhow::Result<Option<(String, String)>> {
        if self.cancelled {
            anyhow::bail!("cancelled")
        }
        self.cancelled = true;
        Ok(Some(("Authorization".into(), "Bearer test-token".into())))
    }
}

#[test]
fn cancelled_setup_is_not_saved_and_header_overrides_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let target = Target::Http {
        url: "https://example.com/target".into(),
        headers: BTreeMap::new(),
    };
    assert!(register(&store, "github".into(), target, &mut Ui { cancelled: true }).is_err());
    assert!(!store.home.exists());
    for name in [
        "Content-Length",
        "Host",
        "content-type",
        "X-Kicker-Event",
        "Connection",
        "Bad\nHeader",
    ] {
        assert!(validation::header(name, "value").is_err());
    }
    assert!(validation::header("Authorization", "Bearer good").is_ok());
    assert!(validation::header("Authorization", "bad\r\nvalue").is_err());
    assert_eq!(format!("{:?}", Secret("sensitive".into())), "[REDACTED]");
}

#[test]
fn cli_requires_exactly_one_target_and_supports_shell_strings() {
    assert!(Cli::try_parse_from(["kicker", "new", "--type", "github"]).is_err());
    assert!(
        Cli::try_parse_from([
            "kicker",
            "new",
            "--type",
            "github",
            "--call-cmd",
            "true",
            "--call-http",
            "http://localhost"
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "kicker",
            "new",
            "--type",
            "github",
            "--call-cmd",
            "cat | tr a-z A-Z > output"
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "kicker",
            "new",
            "--type",
            "github",
            "--call-http",
            "https://example.com",
            "--cwd",
            "/tmp"
        ])
        .is_err()
    );
}

#[test]
fn registration_writes_queue_retry_defaults_and_old_files_use_them() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let id = register(
        &store,
        "github".into(),
        Target::Cmd {
            command: "true".into(),
            cwd: root.path().into(),
        },
        &mut Ui { cancelled: true },
    )
    .unwrap();
    let path = store.home.join("config.toml");
    let text = fs::read_to_string(&path).unwrap();
    for value in [
        "queue_size = 256",
        "retry_interval_secs = 60",
        "max_retries = 3",
    ] {
        assert!(text.contains(value));
    }
    assert!(!text.contains("max_jobs"));
    assert!(!text.contains("max_parallel_hooks"));
    let legacy = text
        .replace("queue_size = 256\n", "")
        .replace("retry_interval_secs = 60\n", "")
        .replace("max_retries = 3\n", "")
        .replace(
            "[server]\n",
            "[server]\nmax_jobs = 0\nmax_parallel_hooks = 0\n",
        );
    fs::write(&path, legacy).unwrap();
    let hook = store.read().unwrap().hooks.remove(0);
    assert_eq!(hook.id, id);
    assert_eq!(
        (hook.queue_size, hook.retry_interval_secs, hook.max_retries),
        (256, 60, 3)
    );
    let before = fs::read(&path).unwrap();
    for invalid in [0, 1] {
        assert!(
            store
                .edit(|config| {
                    if invalid == 0 {
                        config.hooks[0].queue_size = 0;
                    } else {
                        config.hooks[0].retry_interval_secs = 0;
                    }
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    store
        .edit(|config| {
            config.hooks[0].max_retries = 0;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.read().unwrap().hooks[0].max_retries, 0);
}
