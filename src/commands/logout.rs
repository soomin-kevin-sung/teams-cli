use crate::auth::Session;
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::json::print_pretty;
use serde_json::json;
use std::fs;

pub async fn run(json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    Session::logout()?;
    let cache_path = paths.cache_dir.join("chats.json");
    let cache_cleared = cache_path.exists();
    if cache_cleared {
        fs::remove_file(&cache_path)?;
    }
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "logged_out": true,
            "server_side_revocation": false,
            "cache_cleared": cache_cleared
        }))?;
    } else {
        println!("Logged out. (No server-side revocation; tokens expire naturally.)");
    }
    Ok(())
}
