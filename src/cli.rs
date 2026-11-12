use crate::{
    config::Store,
    model::{
        HookConfig, Secret, Target, default_max_retries, default_queue_size,
        default_retry_interval, default_timeout,
    },
    provider::{Providers, SetupUi, TerminalUi},
    server, validation,
};
use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "kicker",
    version,
    about = "Verify webhooks and kick shell commands or HTTP endpoints"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "KICKER_HOME",
        help = "Configuration directory (default: ~/.kicker)"
    )]
    pub home: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Print the exact embedded CycloneDX SBOM without reading configuration.
    Sbom,
    /// List hook IDs, paths and target types from config, without secret values.
    List,
    /// Register a local hook using interactive secret/header prompts.
    New(NewArgs),
    /// Accept POST <base_path>/<hook-id>; HTTPS can terminate at a reverse proxy.
    Serve {
        #[arg(long)]
        bind: Option<SocketAddr>,
    },
    /// Remove a hook from config; restart the server to apply the change.
    #[command(visible_alias = "remove")]
    Delete { hook_id: Uuid },
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("target").required(true).multiple(false).args(["call_cmd", "call_http"])))]
pub struct NewArgs {
    #[arg(long = "type", help = "Webhook provider (supported: github)")]
    pub kind: String,
    #[arg(long, help = "Shell command, executed by /bin/sh -c")]
    pub call_cmd: Option<String>,
    #[arg(long, help = "HTTP(S) endpoint receiving the verified body by POST")]
    pub call_http: Option<String>,
    #[arg(
        long,
        requires = "call_cmd",
        conflicts_with = "call_http",
        help = "Command directory (default: registration working directory)"
    )]
    pub cwd: Option<PathBuf>,
}

impl NewArgs {
    pub fn target(self) -> Result<(String, Target)> {
        let target = if let Some(command) = self.call_cmd {
            let cwd = self
                .cwd
                .unwrap_or(std::env::current_dir()?)
                .canonicalize()
                .context("invalid working directory")?;
            Target::Cmd { command, cwd }
        } else {
            Target::Http {
                url: self.call_http.context("a call target is required")?,
                headers: BTreeMap::new(),
            }
        };
        validation::target(&target)?;
        Ok((self.kind, target))
    }
}

pub fn register(
    store: &Store,
    kind: String,
    mut target: Target,
    ui: &mut dyn SetupUi,
) -> Result<Uuid> {
    validation::registration_target(&target)?;
    let provider = store.providers.get(&kind)?;
    let verification = provider.configure(ui)?;
    provider.validate_config(&verification)?;
    if let Target::Http { headers, .. } = &mut target {
        while let Some((name, value)) = ui.header()? {
            validation::header(&name, &value)?;
            ensure!(
                headers
                    .insert(name.to_ascii_lowercase(), Secret(value))
                    .is_none(),
                "duplicate HTTP header"
            );
        }
    }
    let id = Uuid::new_v4();
    store.add(HookConfig {
        id,
        kind,
        verification,
        target,
        timeout_secs: default_timeout(),
        queue_size: default_queue_size(),
        retry_interval_secs: default_retry_interval(),
        max_retries: default_max_retries(),
    })?;
    Ok(id)
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        if matches!(&self.command, Command::Sbom) {
            return crate::terminal::sbom().context("write SBOM");
        }
        let home = self.home.map(Ok).unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".kicker"))
                .context("HOME is not set; use --home")
        })?;
        let store = Store::new(home, Providers::builtins());
        match self.command {
            Command::Sbom => unreachable!("metadata completed before configuration"),
            Command::List => {
                println!("ID\tTYPE\tPATH\tTARGET");
                let config = store.read()?;
                for hook in config.hooks {
                    println!(
                        "{}\t{}\t{}\t{}",
                        hook.id,
                        hook.kind,
                        config.server.hook_path(hook.id),
                        hook.target.kind()
                    );
                }
            }
            Command::New(args) => {
                let (kind, target) = args.target()?;
                store.providers.get(&kind)?;
                TerminalUi::check_terminal()?;
                let id = register(&store, kind, target, &mut TerminalUi)?;
                let path = store.read()?.server.hook_path(id);
                println!("Registered {id}\nWebhook path: {path}");
                println!("Restart kicker serve to apply configuration changes.");
                println!(
                    "Set this path on your public HTTPS URL in GitHub Webhooks.\nChoose application/json and the configured secret. GitHub controls subscribed events."
                );
            }
            Command::Delete { hook_id } => {
                store.delete(hook_id)?;
                println!("Deleted {hook_id}");
                println!("Restart kicker serve to apply configuration changes.");
            }
            Command::Serve { bind } => server::serve(store, bind).await?,
        }
        Ok(())
    }
}
