use crate::error::CliError;
use pulldown_cmark::{html, Event, Options, Parser};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageFormat {
    Text,
    Html,
    Markdown,
}

impl MessageFormat {
    pub fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "text" => Ok(Self::Text),
            "html" => Ok(Self::Html),
            "markdown" | "md" => Ok(Self::Markdown),
            _ => Err(CliError::structured(
                "invalid_arguments",
                "message format must be one of: text, html, markdown",
                json!({ "format": value }),
                2,
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
            Self::Markdown => "markdown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedMessage {
    pub html: String,
    pub format: MessageFormat,
    pub input_chars: usize,
}

impl PreparedMessage {
    pub fn html_chars(&self) -> usize {
        self.html.chars().count()
    }

    pub fn html_escaped(&self) -> bool {
        matches!(self.format, MessageFormat::Text | MessageFormat::Markdown)
    }

    pub fn markdown_converted(&self) -> bool {
        matches!(self.format, MessageFormat::Markdown)
    }
}

pub fn prepare_message(input: &str, format: MessageFormat) -> PreparedMessage {
    let html = match format {
        MessageFormat::Text => format!("<p>{}</p>", html_escape(input)),
        MessageFormat::Html => input.to_string(),
        MessageFormat::Markdown => markdown_to_html(input),
    };
    PreparedMessage {
        html,
        format,
        input_chars: input.chars().count(),
    }
}

pub fn markdown_to_html(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(input, options).map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        other => other,
    });
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

pub fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\n', "<br/>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_plain_text_as_escaped_html_paragraph() {
        let prepared = prepare_message("<hello>\nworld", MessageFormat::Text);

        assert_eq!(prepared.html, "<p>&lt;hello&gt;<br/>world</p>");
        assert!(prepared.html_escaped());
    }

    #[test]
    fn converts_markdown_to_html_and_escapes_raw_html() {
        let prepared = prepare_message(
            "**bold** [link](https://example.com) <script>x</script>",
            MessageFormat::Markdown,
        );

        assert!(prepared.html.contains("<strong>bold</strong>"));
        assert!(prepared
            .html
            .contains("<a href=\"https://example.com\">link</a>"));
        assert!(prepared.html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(!prepared.html.contains("<script>"));
    }

    #[test]
    fn keeps_explicit_html_unchanged() {
        let prepared = prepare_message("<strong>bold</strong>", MessageFormat::Html);

        assert_eq!(prepared.html, "<strong>bold</strong>");
        assert!(!prepared.html_escaped());
    }
}
