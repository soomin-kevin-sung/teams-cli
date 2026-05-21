use crate::api::client::ApiClient;
use crate::auth::{Session, USER_AGENT};
use crate::commands::target::{self, TargetResolution};
use crate::config::AppPaths;
use crate::error::CliError;
use serde_json::json;

pub async fn run(target: &str, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let resolution = target::resolve_send_target(&api, &paths, target).await?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "resolved": true,
                "target": resolution.target,
                "thread_id": resolution.thread_id,
                "source": resolution.source,
                "chat": resolution.chat
            }))?
        );
    } else {
        println!("Resolved: {}", target_label(&resolution));
        println!("Source  : {:?}", resolution.source);
    }
    Ok(())
}

fn target_label(resolution: &TargetResolution) -> String {
    let title = resolution
        .chat
        .as_ref()
        .and_then(|chat| chat.title.as_deref())
        .unwrap_or("(raw thread id)");
    format!("{title} {}", resolution.thread_id)
}
