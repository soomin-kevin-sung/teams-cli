mod api;
mod auth;
mod cli;
mod commands;
mod config;
mod error;
mod util;

use clap::Parser;
use cli::{AliasCommand, CacheCommand, Cli, Command, DebugCommand, PostCommand};
use error::CliError;
use serde_json::json;
use std::ffi::OsStr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = parse_cli_or_exit();
    init_tracing(cli.verbose);

    let result = match cli.cmd {
        Command::Login { force, tenant } => commands::login::run(force, &tenant, cli.json).await,
        Command::Logout => commands::logout::run(cli.json).await,
        Command::Whoami => commands::whoami::run(cli.json).await,
        Command::ListChats {
            limit,
            include_preview,
        } => commands::list_chats::run(limit, include_preview, cli.json).await,
        Command::SearchChats { query, limit } => {
            commands::search_chats::run(&query, limit, cli.json).await
        }
        Command::Resolve { target } => commands::resolve::run(&target, cli.json).await,
        Command::Read {
            target,
            limit,
            since,
            before,
        } => {
            commands::read::run(
                &target,
                limit,
                since.as_deref(),
                before.as_deref(),
                cli.json,
            )
            .await
        }
        Command::Send {
            dry_run,
            stdin,
            confirm_thread_id,
            chat,
            message,
        } => {
            commands::send::run(
                &chat,
                message.as_deref(),
                stdin,
                confirm_thread_id.as_deref(),
                dry_run,
                cli.json,
            )
            .await
        }
        Command::Post { cmd } => match cmd {
            PostCommand::Channel {
                dry_run,
                stdin,
                card_json,
                confirm_thread_id,
                channel,
                message,
            } => {
                commands::post::channel(commands::post::ChannelOptions {
                    channel: &channel,
                    message: message.as_deref(),
                    read_stdin: stdin,
                    card_json: card_json.as_deref(),
                    confirm_thread_id: confirm_thread_id.as_deref(),
                    dry_run,
                    json_output: cli.json,
                })
                .await
            }
        },
        Command::Alias { cmd } => match cmd {
            AliasCommand::List => commands::alias::list(cli.json).await,
            AliasCommand::Set { name, thread_id } => {
                commands::alias::set(&name, &thread_id, cli.json).await
            }
            AliasCommand::Remove { name } => commands::alias::remove(&name, cli.json).await,
        },
        Command::Cache { cmd } => match cmd {
            CacheCommand::Info => commands::cache::info(cli.json).await,
            CacheCommand::Refresh { limit } => commands::cache::refresh(limit, cli.json).await,
            CacheCommand::Clear => commands::cache::clear(cli.json).await,
        },
        Command::Debug { cmd } => match cmd {
            DebugCommand::RawChats { limit } => commands::debug::raw_chats(limit, cli.json).await,
            DebugCommand::RawMessages { thread_id, limit } => {
                commands::debug::raw_messages(&thread_id, limit, cli.json).await
            }
            DebugCommand::SendHtml { target, html } => {
                commands::debug::send_html(&target, &html, cli.json).await
            }
        },
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
