use crate::api::{chats, client::ApiClient};
use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_cache;
use crate::util::json::print_pretty;
use crate::util::output::render_chats_table;
use serde::Serialize;
use serde_json::json;
use std::cmp::Reverse;

#[derive(Debug, Serialize)]
struct SearchResult {
    score: usize,
    matched: Vec<String>,
    chat: SearchChat,
}

#[derive(Debug, Serialize)]
struct SearchChat {
    id: String,
    kind: chats::ChatKind,
    title: Option<String>,
    member_count: usize,
}

pub async fn run(query: &str, limit: usize, json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let session = Session::load(&http).await?;
    let api = ApiClient::new(session)?;
    let owner = chat_cache::owner_from_api(&api).await;
    let mut chats = chat_cache::load_for_owner(&paths, &owner)?;
    if chats.is_empty() {
        chats = chats::list_chats(&api, 100).await?;
        chat_cache::write(&paths, &owner, &chats)?;
    }

    let mut results = search(query, &chats);
    results.truncate(limit);
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "query": query,
            "count": results.len(),
            "results": results
        }))?;
    } else {
        let table_chats = results
            .iter()
            .map(|result| chats::ChatSummary {
                id: result.chat.id.clone(),
                kind: result.chat.kind.clone(),
                title: result.chat.title.clone(),
                last_message_at: None,
                last_message_preview: None,
                members: Vec::new(),
            })
            .collect::<Vec<_>>();
        println!("{}", render_chats_table(&table_chats));
    }
    Ok(())
}

fn search(query: &str, chats: &[chats::ChatSummary]) -> Vec<SearchResult> {
    let query = normalize(query);
    if query.is_empty() {
        return Vec::new();
    }

    let mut results = chats
        .iter()
        .filter_map(|chat| {
            let mut score = 0;
            let mut matched = Vec::new();
            add_match(&mut score, &mut matched, "id", &chat.id, &query, 2);
            if let Some(title) = chat.title.as_deref() {
                add_match(&mut score, &mut matched, "title", title, &query, 5);
            }
            for member in &chat.members {
                if let Some(name) = member.display_name.as_deref() {
                    add_match(
                        &mut score,
                        &mut matched,
                        "member.display_name",
                        name,
                        &query,
                        4,
                    );
                }
                if let Some(upn) = member.user_principal_name.as_deref() {
                    add_match(
                        &mut score,
                        &mut matched,
                        "member.user_principal_name",
                        upn,
                        &query,
                        4,
                    );
                }
            }
            (score > 0).then(|| SearchResult {
                score,
                matched,
                chat: search_chat(chat),
            })
        })
        .collect::<Vec<_>>();
    results.sort_by_key(|result| Reverse(result.score));
    results
}

fn search_chat(chat: &chats::ChatSummary) -> SearchChat {
    SearchChat {
        id: chat.id.clone(),
        kind: chat.kind.clone(),
        title: chat.title.clone(),
        member_count: chat.members.len(),
    }
}

fn add_match(
    score: &mut usize,
    matched: &mut Vec<String>,
    label: &str,
    value: &str,
    query: &str,
    weight: usize,
) {
    let value_norm = normalize(value);
    if value_norm == query {
        *score += weight * 3;
        matched.push(format!("{label}:exact"));
    } else if value_norm.contains(query) {
        *score += weight;
        matched.push(format!("{label}:contains"));
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_scores_title_matches() {
        let chat = chats::ChatSummary {
            id: "19:abc@thread.v2".to_string(),
            kind: chats::ChatKind::Group,
            title: Some("Project Alpha".to_string()),
            last_message_at: None,
            last_message_preview: None,
            members: Vec::new(),
        };

        let results = search("alpha", &[chat]);

        assert_eq!(results.len(), 1);
        assert!(results[0].matched.contains(&"title:contains".to_string()));
    }
}
