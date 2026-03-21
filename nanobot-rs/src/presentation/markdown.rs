use std::borrow::Cow;
use std::collections::HashSet;

use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};

const WECOM_MARKDOWN_LIMIT_BYTES: usize = 20_480;

pub fn render_web_html(markdown: &str) -> String {
    let html = render_commonmark_html(markdown);
    Builder::default().clean(&html).to_string()
}

pub fn render_telegram_html(markdown: &str) -> String {
    let html = render_commonmark_html(markdown)
        .replace("<strong>", "<b>")
        .replace("</strong>", "</b>")
        .replace("<em>", "<i>")
        .replace("</em>", "</i>");

    let mut builder = Builder::default();
    builder
        .link_rel(None)
        .tags(
            ["a", "b", "i", "code", "pre"]
                .into_iter()
                .collect::<HashSet<_>>(),
        )
        .generic_attributes(["href"].into_iter().collect::<HashSet<_>>());
    builder.clean(&html).to_string()
}

pub fn render_wecom_markdown(markdown: &str) -> String {
    let sanitized = Builder::default().clean(markdown).to_string();
    truncate_utf8_bytes(&sanitized, WECOM_MARKDOWN_LIMIT_BYTES)
}

fn render_commonmark_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }

    let suffix = "…";
    let budget = max_bytes.saturating_sub(suffix.len());
    let mut end = 0;
    for (index, ch) in input.char_indices() {
        if index + ch.len_utf8() > budget {
            break;
        }
        end = index + ch.len_utf8();
    }

    match input.get(..end) {
        Some(prefix) => {
            let mut truncated = String::with_capacity(end + suffix.len());
            truncated.push_str(prefix);
            truncated.push_str(suffix);
            truncated
        }
        None => Cow::from(suffix).into_owned(),
    }
}
