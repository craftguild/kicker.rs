use crate::{
    model::{Config, HookConfig},
    provider::Providers,
    validation,
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

const CONFIG_LIMIT: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct Store {
    pub home: PathBuf,
    pub providers: Providers,
}

fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    check_private(&file.metadata()?, false)?;
    Ok(file)
}

fn check_private(metadata: &fs::Metadata, directory: bool) -> Result<()> {
    ensure!(
        metadata.uid() == nix::unistd::geteuid().as_raw() && metadata.mode() & 0o077 == 0,
        "kicker files must be owned by the current user and private (directory 0700, files 0600)"
    );
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "unexpected kicker file type"
    );
    Ok(())
}

impl Store {
    pub fn new(home: PathBuf, providers: Providers) -> Self {
        Self { home, providers }
    }

    pub fn ensure_home(&self) -> Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.home)?;
        let metadata = fs::symlink_metadata(&self.home)?;
        check_private(&metadata, true)
    }

    fn lock(&self, exclusive: bool, cancelled: &dyn Fn() -> bool) -> Result<File> {
        self.ensure_home()?;
        let file = private_file(&self.home.join("config.lock"))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            ensure!(!cancelled(), "configuration operation cancelled");
            let result = if exclusive {
                file.try_lock()
            } else {
                file.try_lock_shared()
            };
            match result {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock) => {
                    ensure!(Instant::now() < deadline, "configuration lock timed out");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn serve_lock(&self) -> Result<File> {
        self.ensure_home()?;
        let file = private_file(&self.home.join("serve.lock"))?;
        file.try_lock()
            .context("another serve instance is already using this home")?;
        Ok(file)
    }

    fn load(&self) -> Result<Config> {
        let path = self.home.join("config.toml");
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(error) => return Err(error.into()),
        };
        check_private(&file.metadata()?, false)?;
        let mut bytes = Vec::new();
        file.take(CONFIG_LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= CONFIG_LIMIT,
            "config.toml is too large"
        );
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid config.toml encoding"))?;
        let config: Config = toml::from_str(text)
            .map_err(|_| anyhow::anyhow!("invalid config.toml syntax or fields"))?;
        Ok(config)
    }

    /// Hold the shared lock through decoding, validation and the caller callback.
    pub fn read_with<T>(&self, read: impl FnOnce(&Config) -> Result<T>) -> Result<T> {
        self.read_interruptible(|| false, read)
    }

    pub fn read_interruptible<T>(
        &self,
        cancelled: impl Fn() -> bool,
        read: impl FnOnce(&Config) -> Result<T>,
    ) -> Result<T> {
        let _lock = self.lock(false, &cancelled)?;
        let config = self.load()?;
        validation::config(&config, &self.providers)?;
        ensure!(!cancelled(), "configuration operation cancelled");
        read(&config)
    }

    pub fn read(&self) -> Result<Config> {
        if !self.home.try_exists()? {
            return Ok(Config::default());
        }
        self.read_with(|config| Ok(config.clone()))
    }

    pub fn edit(&self, update: impl FnOnce(&mut Config) -> Result<()>) -> Result<()> {
        let _lock = self.lock(true, &|| false)?;
        let mut config = self.load()?;
        update(&mut config)?;
        validation::config(&config, &self.providers)?;
        let text = toml::to_string_pretty(&config)
            .map_err(|_| anyhow::anyhow!("could not serialize config"))?;
        ensure!(
            text.len() as u64 <= CONFIG_LIMIT,
            "config.toml is too large"
        );
        let mut temporary = tempfile::NamedTempFile::new_in(&self.home)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temporary.write_all(text.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.home.join("config.toml"))
            .map_err(|error| error.error)?;
        File::open(&self.home)?.sync_all()?;
        Ok(())
    }

    pub fn add(&self, hook: HookConfig) -> Result<()> {
        validation::registration_target(&hook.target)?;
        self.edit(move |config| {
            config.hooks.push(hook);
            Ok(())
        })
    }

    pub fn delete(&self, id: Uuid) -> Result<()> {
        self.edit(|config| {
            let index = config
                .hooks
                .iter()
                .position(|hook| hook.id == id)
                .context("hook not found")?;
            config.hooks.remove(index);
            Ok(())
        })
    }
}
