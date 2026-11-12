#[cfg(not(unix))]
compile_error!(
    "kicker currently supports Linux and macOS (requires /bin/sh and Unix process groups)"
);

use tracing_subscriber::fmt::writer::{MakeWriter, MakeWriterExt};

fn severity_writer<Out, Err>(stdout: Out, stderr: Err) -> impl for<'a> MakeWriter<'a>
where
    Out: for<'a> MakeWriter<'a>,
    Err: for<'a> MakeWriter<'a>,
{
    stderr.with_max_level(tracing::Level::ERROR).or_else(stdout)
}

fn main() -> std::process::ExitCode {
    let cli = match kicker::terminal::parse() {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    if matches!(&cli.command, kicker::cli::Command::Sbom) {
        return kicker::terminal::exit(kicker::terminal::sbom());
    }
    run(cli)
}

#[tokio::main]
async fn run(cli: kicker::cli::Cli) -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_writer(severity_writer(std::io::stdout, std::io::stderr))
        .with_env_filter("kicker=info")
        .init();
    match cli.run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/logging.rs"]
mod logging_tests;
