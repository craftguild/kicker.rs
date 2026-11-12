use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, net::SocketAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub server: ServerConfig,
    pub hooks: Vec<HookConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            server: ServerConfig::default(),
            hooks: vec![],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub base_path: String,
    // Legacy global limits are read for compatibility, but are no longer applied.
    #[serde(skip_serializing)]
    pub max_jobs: usize,
    pub max_payload_bytes: usize,
    pub max_buffered_bytes: usize,
    #[serde(skip_serializing)]
    pub max_parallel_hooks: usize,
    pub shutdown_timeout_secs: u64,
    pub max_in_flight_requests: usize,
    pub body_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".parse().expect("static bind address"),
            base_path: "/".into(),
            max_jobs: 32,
            max_payload_bytes: 25 * 1024 * 1024,
            max_buffered_bytes: 64 * 1024 * 1024,
            max_parallel_hooks: 4,
            shutdown_timeout_secs: 30,
            max_in_flight_requests: 4,
            body_timeout_secs: 10,
        }
    }
}

impl ServerConfig {
    pub fn hook_path(&self, id: impl fmt::Display) -> String {
        format!("{}/{id}", self.base_path.trim_end_matches('/'))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookConfig {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    pub verification: toml::Table,
    pub target: Target,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_queue_size")]
    pub queue_size: usize,
    #[serde(default = "default_retry_interval")]
    pub retry_interval_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

pub fn default_queue_size() -> usize {
    256
}

pub fn default_retry_interval() -> u64 {
    60
}

pub fn default_max_retries() -> u32 {
    3
}

pub fn default_timeout() -> u64 {
    300
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Cmd {
        command: String,
        cwd: PathBuf,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, Secret>,
    },
}

impl Target {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Cmd { .. } => "cmd",
            Self::Http { .. } => "http",
        }
    }
}
