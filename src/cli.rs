use clap::{ArgAction, Parser, Subcommand};

pub const DISCLAIMER: &str =
    "Unofficial Microsoft Teams CLI. Uses undocumented web APIs; use at your own risk.";

#[derive(Parser, Debug)]
#[command(name = "teams", version, about = DISCLAIMER)]
pub struct Cli {
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[arg(
        long,
        global = true,
        help = "Emit machine-readable JSON for supported commands and errors"
    )]
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
        /// Include last message previews in JSON output.
        #[arg(long)]
        include_preview: bool,
    },
    /// Search cached or recent chats by title, member name, email, or thread id.
    SearchChats {
        /// Text to search in chat title, id, member display name, or email.
        query: String,
        /// Limit number of candidates.
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Resolve a send target without sending a message.
    Resolve {
        /// Chat thread id, alias, self target (me/self/notes), exact email, exact display name, or exact chat title.
        target: String,
    },
    /// Read recent messages from an existing 1:1, group, or self notes chat.
    Read {
        /// Chat thread id, alias, self target (me/self/notes), exact email, exact display name, or exact chat title.
        target: String,
        /// Limit number of messages. Values above 100 are clamped.
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Only include messages at or after this RFC3339 timestamp.
        #[arg(long)]
        since: Option<String>,
        /// Only include messages before this RFC3339 timestamp.
        #[arg(long)]
        before: Option<String>,
    },
    /// Send a text message to an existing 1:1, group, or self notes chat.
    Send {
        /// Resolve and print the target without sending.
        #[arg(long)]
        dry_run: bool,
        /// Read plaintext message body from stdin instead of MESSAGE.
        #[arg(long)]
        stdin: bool,
        /// Refuse to send unless the resolved thread id exactly matches this value.
        #[arg(long)]
        confirm_thread_id: Option<String>,
        /// Chat thread id, alias, self target (me/self/notes), exact email, exact display name, or exact chat title.
        chat: String,
        /// Plaintext message body. Optional when --stdin is used.
        message: Option<String>,
    },
    /// Manage local aliases.
    Alias {
        #[command(subcommand)]
        cmd: AliasCommand,
    },
    /// Manage local chat cache.
    Cache {
        #[command(subcommand)]
        cmd: CacheCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum AliasCommand {
    /// List configured aliases.
    List,
    /// Set or replace an alias.
    Set {
        /// Alias name.
        name: String,
        /// Raw Teams thread id.
        thread_id: String,
    },
    /// Remove an alias.
    Remove {
        /// Alias name.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum CacheCommand {
    /// Print cache metadata.
    Info,
    /// Refresh cached chats.
    Refresh {
        /// Limit number of chats to refresh.
        #[arg(short = 'n', long, default_value_t = 100)]
        limit: usize,
    },
    /// Clear cached chats.
    Clear,
}
