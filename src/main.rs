mod api;
mod auth;
mod cli;
mod commands;
mod config;
mod error;
mod util;

use clap::Parser;
use cli::{Cli, Command};
use error::CliError;
use serde_json::json;
use std::ffi::OsStr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = parse_cli_or_exit();
    init_tracing(cli.verbose);

    let result = match cli.cmd {
        Command::Login { force, tenant } => commands::login::run(force, &tenant).await,
        Command::Logout => commands::logout::run().await,
        Command::Whoami => commands::whoami::run(cli.json).await,
        Command::ListChats { limit } => commands::list_chats::run(limit, cli.json).await,
        Command::Resolve { target } => commands::resolve::run(&target, cli.json).await,
        Command::Read { target, limit } => commands::read::run(&target, limit, cli.json).await,
        Command::Send {
            dry_run,
            chat,
            message,
        } => commands::send::run(&chat, &message, dry_run, cli.json).await,
    };

    if let Err(error) = result {
        if cli.json {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&error.to_json())
                    .unwrap_or_else(|_| error.to_string())
            );
        } else {
            eprintln!("{error}");
        }
        std::process::exit(error.to_exit_code());
    }
}

fn parse_cli_or_exit() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if error.exit_code() == 0 => {
            print!("{error}");
            std::process::exit(0);
        }
        Err(error) if wants_json() => {
            let exit_code = error.exit_code();
            let cli_error = CliError::structured(
                "cli_parse_error",
                "invalid command line",
                json!({
                    "clap_kind": format!("{:?}", error.kind())
                }),
                exit_code,
            );
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&cli_error.to_json())
                    .unwrap_or_else(|_| cli_error.to_string())
            );
            std::process::exit(exit_code);
        }
        Err(error) => error.exit(),
    }
}

fn wants_json() -> bool {
    std::env::args_os().any(|arg| arg == OsStr::new("--json"))
}

fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .init();
}
