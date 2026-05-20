mod api;
mod auth;
mod cli;
mod commands;
mod config;
mod error;
mod util;

use clap::Parser;
use cli::{Cli, Command};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let result = match cli.cmd {
        Command::Login { force, tenant } => commands::login::run(force, &tenant).await,
        Command::Logout => commands::logout::run().await,
        Command::Whoami => commands::whoami::run(cli.json).await,
        Command::ListChats { limit } => commands::list_chats::run(limit, cli.json).await,
        Command::Send { chat, message } => commands::send::run(&chat, &message, cli.json).await,
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.to_exit_code());
    }
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
