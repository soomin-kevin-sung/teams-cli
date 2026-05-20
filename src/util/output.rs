use crate::api::chats::ChatSummary;
use comfy_table::{presets::UTF8_FULL, Cell, Table};

pub fn render_chats_table(chats: &[ChatSummary]) -> String {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["ID", "Type", "Title", "Last Activity"]);
    for chat in chats {
        table.add_row(vec![
            Cell::new(short_id(&chat.id)),
            Cell::new(chat.kind.to_string()),
            Cell::new(
                chat.title
                    .clone()
                    .unwrap_or_else(|| member_labels(chat).join(", ")),
            ),
            Cell::new(
                chat.last_message_at
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ]);
    }
    table.to_string()
}

fn member_labels(chat: &ChatSummary) -> Vec<String> {
    chat.members
        .iter()
        .filter_map(|member| member.label())
        .collect()
}

fn short_id(id: &str) -> String {
    if id.len() > 16 {
        format!("{}…", &id[..16])
    } else {
        id.to_string()
    }
}
