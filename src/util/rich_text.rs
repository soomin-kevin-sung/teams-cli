use pulldown_cmark::{html, Event, Options, Parser};

#[derive(Debug, Clone)]
pub struct PreparedMessage {
    pub html: String,
    pub input_chars: usize,
}

impl PreparedMessage {
    pub fn html_chars(&self) -> usize {
        self.html.chars().count()
    }

    pub fn html_escaped(&self) -> bool {
        true
    }

    pub fn markdown_converted(&self) -> bool {
        true
    }
}

pub fn prepare_message(input: &str) -> PreparedMessage {
    PreparedMessage {
        html: markdown_to_html(input),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_markdown_to_html_and_escapes_raw_html() {
        let prepared = prepare_message("**bold** [link](https://example.com) <script>x</script>");

        assert!(prepared.html.contains("<strong>bold</strong>"));
        assert!(prepared
            .html
            .contains("<a href=\"https://example.com\">link</a>"));
        assert!(prepared.html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(!prepared.html.contains("<script>"));
    }
}
