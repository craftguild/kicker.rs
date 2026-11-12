//! Offline help and version output with the embedded kicker.rs banner.

use crate::cli::Cli;
use clap::{CommandFactory, Parser, error::ErrorKind};
use std::{
    io::{self, Write},
    process::ExitCode,
};

pub(crate) const BANNER: &str = include_str!("../assets/banner.txt");
const BRAND: craftguild_cli::Brand<'static> = craftguild_cli::Brand {
    banner: BANNER,
    project: "kicker.rs",
};
#[allow(dead_code)]
mod captured {
    include!(concat!(env!("OUT_DIR"), "/craftguild.rs"));
}

pub fn sbom() -> io::Result<()> {
    craftguild_cli::sbom(&mut io::stdout().lock(), captured::SBOM)
}

pub fn exit(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            let _ = io::stderr().lock().write_all(b"kicker: output failed\n");
            ExitCode::FAILURE
        }
    }
}

pub fn parse() -> Result<Cli, ExitCode> {
    let mut arguments = std::env::args_os().collect::<Vec<_>>();
    if arguments.len() == 1 {
        arguments.push("--help".into());
    }
    Cli::try_parse_from(arguments).map_err(|error| {
        let text = error.to_string();
        let root_help = error.kind() == ErrorKind::DisplayHelp
            && (text == Cli::command().render_help().to_string()
                || text == Cli::command().render_long_help().to_string());
        let printed = if error.kind() == ErrorKind::DisplayVersion {
            craftguild_cli::version(&mut io::stdout().lock(), BRAND, captured::BUILD_INFO)
        } else if root_help {
            craftguild_cli::help(&mut io::stdout().lock(), BRAND, &text)
        } else {
            error.print()
        };
        if printed.is_err() {
            ExitCode::FAILURE
        } else {
            ExitCode::from(error.exit_code() as u8)
        }
    })
}
