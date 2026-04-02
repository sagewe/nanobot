use std::borrow::Cow;
use std::collections::HashSet;

use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};

const WECOM_MARKDOWN_LIMIT_BYTES: usize = 20_480;
const TELEGRAM_MESSAGE_LIMIT: usize = 4_000;

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
    builder.clean(&html).to_string().trim().to_string()
}

pub fn split_telegram_html_chunks(html: &str, limit: usize) -> Vec<String> {
    if html.chars().count() <= limit {
        return vec![html.to_string()];
    }

    let tokens = tokenize_html(html);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut open_tags: Vec<OpenTag> = Vec::new();

    for token in tokens {
        if is_html_tag(&token) {
            let token_len = token.chars().count();
            if !current.is_empty()
                && current.chars().count() + token_len + closing_tags_len(&open_tags) > limit
            {
                push_chunk(&mut chunks, &mut current, &open_tags);
            }
            current.push_str(&token);
            update_open_tags(&token, &mut open_tags);
            continue;
        }

        let mut remainder = token.as_str();
        while !remainder.is_empty() {
            let available =
                limit.saturating_sub(current.chars().count() + closing_tags_len(&open_tags));
            if available == 0 {
                push_chunk(&mut chunks, &mut current, &open_tags);
                continue;
            }

            let (head, tail) = split_at_char_boundary(remainder, available);
            current.push_str(head);
            remainder = tail;

            if !remainder.is_empty() {
                push_chunk(&mut chunks, &mut current, &open_tags);
            }
        }
    }

    if !current.is_empty() {
        push_chunk(&mut chunks, &mut current, &open_tags);
    }

    chunks
}

pub fn telegram_message_limit() -> usize {
    TELEGRAM_MESSAGE_LIMIT
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

#[derive(Clone)]
struct OpenTag {
    name: String,
    opening: String,
}

fn push_chunk(chunks: &mut Vec<String>, current: &mut String, open_tags: &[OpenTag]) {
    let mut chunk = std::mem::take(current);
    for tag in open_tags.iter().rev() {
        chunk.push_str("</");
        chunk.push_str(&tag.name);
        chunk.push('>');
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    for tag in open_tags {
        current.push_str(&tag.opening);
    }
}

fn closing_tags_len(open_tags: &[OpenTag]) -> usize {
    open_tags
        .iter()
        .map(|tag| tag.name.chars().count() + 3)
        .sum()
}

fn tokenize_html(html: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cursor = 0;

    while cursor < html.len() {
        let tail = &html[cursor..];
        if let Some(stripped) = tail.strip_prefix('<') {
            if let Some(end) = stripped.find('>') {
                let end_index = cursor + end + 2;
                tokens.push(html[cursor..end_index].to_string());
                cursor = end_index;
                continue;
            }
        }

        if let Some(next_tag) = tail.find('<') {
            let end_index = cursor + next_tag;
            tokens.push(html[cursor..end_index].to_string());
            cursor = end_index;
        } else {
            tokens.push(tail.to_string());
            break;
        }
    }

    tokens
}

fn is_html_tag(token: &str) -> bool {
    token.starts_with('<') && token.ends_with('>')
}

fn update_open_tags(token: &str, open_tags: &mut Vec<OpenTag>) {
    if token.starts_with("</") {
        if let Some(name) = tag_name(token) {
            if let Some(index) = open_tags.iter().rposition(|tag| tag.name == name) {
                open_tags.remove(index);
            }
        }
        return;
    }

    if token.starts_with('<') && !token.ends_with("/>") {
        if let Some(name) = tag_name(token) {
            open_tags.push(OpenTag {
                name,
                opening: token.to_string(),
            });
        }
    }
}

fn tag_name(token: &str) -> Option<String> {
    let trimmed = token
        .trim_start_matches("</")
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let name = trimmed.split_whitespace().next()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn split_at_char_boundary(input: &str, max_chars: usize) -> (&str, &str) {
    if input.chars().count() <= max_chars {
        return (input, "");
    }

    let mut end = 0;
    for (count, (index, ch)) in input.char_indices().enumerate() {
        if count >= max_chars {
            break;
        }
        end = index + ch.len_utf8();
    }
    (&input[..end], &input[end..])
}
