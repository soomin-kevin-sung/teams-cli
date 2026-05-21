use crate::api::{chats, client::ApiClient};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_cache;
use crate::util::json::print_pretty;
use crate::util::output::render_chats_table;
use serde_json::json;

pub async fn run(limit: usize, include_preview: bool, json_output: bool) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let chats = chats::list_chats(&api, limit).await?;
    let paths = AppPaths::resolve()?;
    let owner = chat_cache::owner_from_api(&api).await;
    chat_cache::write(&paths, &owner, &chats)?;

    if json_output {
        let output_chats = redact_previews(&chats, include_preview);
        print_pretty(&json!({
            "ok": true,
            "count": output_chats.len(),
            "include_preview": include_preview,
            "chats": output_chats
        }))?;
    } else {
        println!("{}", render_chats_table(&chats));
    }
    Ok(())
}

fn redact_previews(chats: &[chats::ChatSummary], include_preview: bool) -> Vec<chats::ChatSummary> {
    chats
        .iter()
        .cloned()
        .map(|mut chat| {
            if !include_preview {
                chat.last_message_preview = None;
            }
            chat
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_previews_by_default() {
        let chats = vec![chats::ChatSummary {
            id: "19:abc@thread.v2".into(),
            kind: chats::ChatKind::Group,
            title: Some("Project".into()),
            last_message_at: None,
            last_message_preview: Some("sensitive".into()),
            members: Vec::new(),
        }];

        assert_eq!(redact_previews(&chats, false)[0].last_message_preview, None);
        assert_eq!(
            redact_previews(&chats, true)[0]
                .last_message_preview
                .as_deref(),
            Some("sensitive")
        );
    }
}
