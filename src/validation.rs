use crate::{model::*, provider::Providers};
use anyhow::{Result, ensure};
use axum::http::{HeaderName, HeaderValue};
use std::collections::HashSet;

pub fn header(name: &str, value: &str) -> Result<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| anyhow::anyhow!("invalid header name"))?;
    let reserved = [
        "host",
        "content-length",
        "content-type",
        "transfer-encoding",
        "connection",
        "keep-alive",
        "te",
        "trailer",
        "upgrade",
        "proxy-authorization",
        "proxy-authenticate",
    ];
    ensure!(
        !reserved.contains(&name.as_str()) && !name.as_str().starts_with("x-kicker-"),
        "reserved HTTP header"
    );
    HeaderValue::from_str(value).map_err(|_| anyhow::anyhow!("invalid header value"))?;
    Ok(())
}

pub fn target(target: &Target) -> Result<()> {
    match target {
        Target::Cmd { command, cwd } => {
            ensure!(
                !command.trim().is_empty() && !command.contains('\0'),
                "command must be nonempty and contain no NUL"
            );
            ensure!(
                cwd.is_absolute() && !cwd.as_os_str().as_encoded_bytes().contains(&0),
                "cwd must be an absolute path without NUL"
            );
        }
        Target::Http { url, headers } => {
            let url =
                reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!("invalid HTTP target URL"))?;
            ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none(),
                "HTTP target must be HTTP(S), without userinfo or fragment"
            );
            let mut names = HashSet::new();
            for (name, value) in headers {
                header(name, &value.0)?;
                ensure!(
                    names.insert(name.to_ascii_lowercase()),
                    "duplicate HTTP header"
                );
            }
        }
    }
    Ok(())
}

pub fn registration_target(value: &Target) -> Result<()> {
    target(value)?;
    if let Target::Cmd { cwd, .. } = value {
        ensure!(
            cwd.is_dir(),
            "cwd must be an existing directory at registration"
        );
    }
    Ok(())
}

pub fn server(server: &ServerConfig) -> Result<()> {
    let base = server.base_path.trim_end_matches('/');
    ensure!(
        server.base_path.starts_with('/')
            && !server.base_path.contains("//")
            && base.split('/').skip(1).all(|segment| {
                !matches!(segment, "" | "." | "..")
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
            }),
        "base_path must be an absolute path with static URL-safe segments"
    );
    ensure!(
        server.max_payload_bytes > 0 && server.max_buffered_bytes >= server.max_payload_bytes,
        "invalid payload byte limits"
    );
    ensure!(
        server.max_in_flight_requests > 0 && server.max_in_flight_requests <= 65536,
        "invalid max_in_flight_requests"
    );
    ensure!(
        server.body_timeout_secs > 0,
        "body_timeout_secs must be positive"
    );
    Ok(())
}

pub fn execution(hook: &HookConfig) -> Result<()> {
    ensure!(!hook.id.is_nil(), "nil hook ID");
    ensure!(
        !hook.kind.is_empty() && !hook.kind.contains('\0'),
        "invalid provider kind"
    );
    ensure!(hook.timeout_secs > 0, "hook timeout must be positive");
    ensure!(hook.queue_size > 0, "hook queue_size must be positive");
    ensure!(
        hook.retry_interval_secs > 0,
        "hook retry_interval_secs must be positive"
    );
    target(&hook.target)
}

pub fn config(config: &Config, providers: &Providers) -> Result<()> {
    ensure!(config.version == 1, "unsupported config version");
    server(&config.server)?;
    let mut ids = HashSet::new();
    for hook in &config.hooks {
        ensure!(
            !hook.id.is_nil() && ids.insert(hook.id),
            "nil or duplicate hook ID"
        );
        execution(hook)?;
        providers
            .get(&hook.kind)?
            .validate_config(&hook.verification)?;
    }
    Ok(())
}
