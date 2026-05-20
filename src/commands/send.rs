use crate::api::{client::ApiClient, messages};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_ref::{resolve, ChatRef};
use serde_json::json;

pub async fn run(chat: &str, message: &str, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let thread_id = match resolve(chat, &paths.aliases) {
        ChatRef::ThreadId(thread_id) => thread_id,
        ChatRef::UnresolvableUpn(upn) => {
            return Err(CliError::Other(format!(
                "Resolving '{upn}' requires a 1:1 thread lookup; run `teams list-chats` and pass the 19:…@unq.gbl.spaces id directly. UPN resolution is planned for v0.2."
            )));
        }
        ChatRef::Unknown(value) => {
            return Err(CliError::Other(format!(
                "unknown chat reference '{value}'. Pass a raw 19:… thread id or define an alias in {}",
                paths.aliases.display()
            )));
        }
    };

    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let id = messages::send_message(&api, &thread_id, message).await?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&json!({ "id": id }))?);
    } else {
        println!("Sent: {id}");
    }
    Ok(())
}
