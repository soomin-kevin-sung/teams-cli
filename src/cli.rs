use clap::{ArgAction, Parser, Subcommand};

pub const DISCLAIMER: &str =
    "Unofficial Microsoft Teams CLI. Uses undocumented web APIs; use at your own risk.";

#[derive(Parser, Debug)]
#[command(name = "teams", version, about = DISCLAIMER)]
pub struct Cli {
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Sign in via OAuth2 device-code flow.
    Login {
        /// Force re-login even if a session is cached.
        #[arg(long)]
        force: bool,
        /// AAD tenant id, GUID, or domain.
        #[arg(long, env = "TEAMS_TENANT", default_value = "organizations")]
        tenant: String,
    },
    /// Clear stored tokens and state.
    Logout,
    /// Print cached identity and token expiry information.
    Whoami,
    /// List recent group chats.
    ListChats {
        /// Limit number of chats.
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
    /// Send a text message to an existing 1:1 or group chat.
    Send {
        /// Chat thread id, alias, exact email, exact display name, or exact chat title.
        chat: String,
        /// Plaintext message body.
        message: String,
    },
}
