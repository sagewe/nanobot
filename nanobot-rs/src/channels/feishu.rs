use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use regex::Regex;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::warn;
use url::Url;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::FeishuConfig;
use crate::presentation::should_deliver_to_channel;

const FEISHU_DEDUP_CAPACITY: usize = 1000;
const FEISHU_RECONNECT_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
struct FeishuToken {
    token: String,
    expire_at_unix: i64,
}

pub struct FeishuChannel {
    config: FeishuConfig,
    bus: MessageBus,
    client: reqwest::Client,
    running: AtomicBool,
    token: Arc<Mutex<Option<FeishuToken>>>,
    bot_open_id: Arc<Mutex<Option<String>>>,
    dedup: Arc<Mutex<RecentMessageDedup>>,
}

impl FeishuChannel {
    pub fn new(config: FeishuConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            client: reqwest::Client::new(),
            running: AtomicBool::new(false),
            token: Arc::new(Mutex::new(None)),
            bot_open_id: Arc::new(Mutex::new(None)),
            dedup: Arc::new(Mutex::new(RecentMessageDedup::new(FEISHU_DEDUP_CAPACITY))),
        }
    }

    fn validate_startup_config(&self) -> Result<()> {
        ensure!(
            !self.config.app_id.trim().is_empty() && !self.config.app_secret.trim().is_empty(),
            "feishu app_id/app_secret is required"
        );

        let api_url = Url::parse(self.config.api_base.trim())
            .map_err(|error| anyhow::anyhow!("invalid feishu api_base: {error}"))?;
        ensure!(
            matches!(api_url.scheme(), "http" | "https"),
            "invalid feishu api_base scheme"
        );

        let ws_url = Url::parse(self.config.ws_base.trim())
            .map_err(|error| anyhow::anyhow!("invalid feishu ws_base: {error}"))?;
        ensure!(
            matches!(ws_url.scheme(), "ws" | "wss"),
            "invalid feishu ws_base scheme"
        );

        Ok(())
    }

    async fn cached_token(&self) -> Option<String> {
        let now = chrono::Utc::now().timestamp();
        self.token
            .lock()
            .await
            .as_ref()
            .and_then(|state| (state.expire_at_unix > now).then(|| state.token.clone()))
    }

    async fn tenant_access_token(&self) -> Result<String> {
        if let Some(token) = self.cached_token().await {
            return Ok(token);
        }

        let url = format!(
            "{}/auth/v3/tenant_access_token/internal",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .json(&json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
        ensure!(code == 0, "feishu auth failed");

        let token = payload
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing tenant_access_token"))?
            .to_string();
        let expire = payload
            .get("expire")
            .and_then(Value::as_i64)
            .unwrap_or(7200)
            .max(120);
        *self.token.lock().await = Some(FeishuToken {
            token: token.clone(),
            expire_at_unix: chrono::Utc::now().timestamp() + expire - 60,
        });
        Ok(token)
    }

    async fn bot_open_id(&self) -> Result<String> {
        if let Some(open_id) = self.bot_open_id.lock().await.clone() {
            return Ok(open_id);
        }

        let access_token = self.tenant_access_token().await?;
        let url = format!("{}/bot/v3/info", self.config.api_base.trim_end_matches('/'));
        let payload: Value = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
        ensure!(code == 0, "feishu bot info failed");
        let open_id = payload
            .pointer("/bot/open_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing feishu bot open_id"))?
            .to_string();
        *self.bot_open_id.lock().await = Some(open_id.clone());
        Ok(open_id)
    }

    async fn create_message(
        &self,
        access_token: &str,
        receive_id_type: &str,
        receive_id: &str,
        msg_type: &str,
        content: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/im/v1/messages?receive_id_type={receive_id_type}",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "receive_id": receive_id,
                "msg_type": msg_type,
                "content": content,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu create message failed"
        );
        Ok(())
    }

    async fn reply_message(
        &self,
        access_token: &str,
        message_id: &str,
        msg_type: &str,
        content: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/im/v1/messages/{message_id}/reply",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "msg_type": msg_type,
                "content": content,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu reply failed"
        );
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str) -> Result<()> {
        if self.config.react_emoji.trim().is_empty() {
            return Ok(());
        }

        let access_token = self.tenant_access_token().await?;
        let url = format!(
            "{}/im/v1/messages/{message_id}/reactions",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "reaction_type": {
                    "emoji_type": self.config.react_emoji,
                }
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu add reaction failed"
        );
        Ok(())
    }

    async fn run_session(&self) -> Result<()> {
        let bot_open_id = self.bot_open_id().await?;
        let (stream, _) = connect_async(self.config.ws_base.as_str())
            .await
            .with_context(|| format!("failed to connect to {}", self.config.ws_base))?;
        let (mut writer, mut reader) = stream.split();

        while self.running.load(Ordering::SeqCst) {
            let frame = match reader.next().await {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(error.into()),
                None => bail!("feishu websocket closed"),
            };

            match frame {
                Message::Text(text) => {
                    if let Err(error) = self.handle_text_frame(text.as_ref(), &bot_open_id).await {
                        warn!("feishu dropped invalid frame: {error}");
                    }
                }
                Message::Ping(payload) => {
                    writer
                        .send(Message::Pong(payload))
                        .await
                        .context("failed to respond to feishu websocket ping")?;
                }
                Message::Close(_) => bail!("feishu websocket closed"),
                _ => {}
            }
        }

        Ok(())
    }

    async fn handle_text_frame(&self, text: &str, bot_open_id: &str) -> Result<()> {
        let payload: Value = serde_json::from_str(text).context("invalid feishu json payload")?;
        let Some(event) = payload.get("event") else {
            return Ok(());
        };

        let sender_type = event
            .pointer("/sender/sender_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sender_id = event
            .pointer("/sender/sender_id/open_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if sender_type == "bot" || sender_id.is_empty() || sender_id == bot_open_id {
            return Ok(());
        }
        if !is_allowed_sender(&self.config, sender_id) {
            return Ok(());
        }

        let message_id = event
            .pointer("/message/message_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let message_type = event
            .pointer("/message/message_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chat_id = event
            .pointer("/message/chat_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chat_type = event
            .pointer("/message/chat_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let raw_content = event
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let mentions = event
            .pointer("/message/mentions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if message_id.is_empty() || chat_id.is_empty() {
            return Ok(());
        }
        if chat_type != "p2p"
            && !is_group_message_for_bot(
                raw_content,
                mentions.as_slice(),
                Some(bot_open_id),
                self.config.group_policy.as_str(),
            )
        {
            return Ok(());
        }

        let content = match normalize_inbound_content(message_type, raw_content)? {
            Some(content) => content,
            None => return Ok(()),
        };
        if content.trim().is_empty() {
            return Ok(());
        }

        {
            let mut dedup = self.dedup.lock().await;
            if !dedup.insert(message_id) {
                return Ok(());
            }
        }

        let parent_id = event.pointer("/message/parent_id").and_then(Value::as_str);
        let root_id = event.pointer("/message/root_id").and_then(Value::as_str);
        let mut metadata = HashMap::new();
        metadata.insert("message_id".to_string(), json!(message_id));
        metadata.insert("chat_type".to_string(), json!(chat_type));
        metadata.insert("msg_type".to_string(), json!(message_type));
        metadata.insert("parent_id".to_string(), json!(parent_id));
        metadata.insert("root_id".to_string(), json!(root_id));

        self.bus
            .publish_inbound(InboundMessage {
                channel: "feishu".to_string(),
                sender_id: sender_id.to_string(),
                chat_id: if chat_type == "p2p" {
                    sender_id.to_string()
                } else {
                    chat_id.to_string()
                },
                content,
                timestamp: chrono::Utc::now(),
                metadata,
                session_key_override: None,
            })
            .await?;

        if let Err(error) = self.add_reaction(message_id).await {
            warn!("feishu reaction failed for {message_id}: {error}");
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeishuMessageFormat {
    Text,
    Post,
    Interactive,
}

#[derive(Debug)]
struct RecentMessageDedup {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl RecentMessageDedup {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    fn insert(&mut self, message_id: &str) -> bool {
        if self.seen.contains(message_id) {
            return false;
        }
        let message_id = message_id.to_string();
        self.order.push_back(message_id.clone());
        self.seen.insert(message_id);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }
}

fn markdown_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\((https?://[^\)]+)\)").expect("markdown link"))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^#{1,6}\s+.+$").expect("heading"))
}

fn unordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[ \t]*[-*+]\s+").expect("unordered list"))
}

fn ordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[ \t]*\d+\.\s+").expect("ordered list"))
}

fn bold_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\*\*.+?\*\*|__.+?__").expect("bold"))
}

fn strike_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"~~.+?~~").expect("strike"))
}

fn table_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?m)((?:^[ \t]*\|.+\|[ \t]*\n)(?:^[ \t]*\|[-:\s|]+\|[ \t]*\n)(?:^[ \t]*\|.+\|[ \t]*(?:\n|$))+)",
        )
        .expect("table")
    })
}

fn code_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```.*?```").expect("code block"))
}

fn has_italic_marker(text: &str) -> bool {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if bytes[start] != b'*' {
            continue;
        }
        if start > 0 && bytes[start - 1] == b'*' {
            continue;
        }
        if start + 1 >= bytes.len() || bytes[start + 1] == b'*' {
            continue;
        }
        for end in start + 1..bytes.len() {
            if bytes[end] != b'*' {
                continue;
            }
            if bytes[end - 1] == b'*' {
                continue;
            }
            if end + 1 < bytes.len() && bytes[end + 1] == b'*' {
                continue;
            }
            return true;
        }
    }
    false
}

fn detect_message_format(content: &str) -> FeishuMessageFormat {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return FeishuMessageFormat::Text;
    }

    let has_complex_markdown = trimmed.contains("```")
        || heading_re().is_match(trimmed)
        || table_block_re().is_match(trimmed)
        || unordered_list_re().is_match(trimmed)
        || ordered_list_re().is_match(trimmed)
        || bold_re().is_match(trimmed)
        || has_italic_marker(trimmed)
        || strike_re().is_match(trimmed);

    if has_complex_markdown || trimmed.len() > 2000 {
        return FeishuMessageFormat::Interactive;
    }
    if markdown_link_re().is_match(trimmed) || trimmed.len() > 200 {
        return FeishuMessageFormat::Post;
    }
    FeishuMessageFormat::Text
}

fn render_post_body(content: &str) -> String {
    let paragraphs: Vec<Vec<Value>> = content
        .trim()
        .split('\n')
        .map(|line| {
            let mut elements = Vec::new();
            let mut last_end = 0;
            for captures in markdown_link_re().captures_iter(line) {
                let full = captures.get(0).expect("full match");
                if full.start() > last_end {
                    elements.push(json!({
                        "tag": "text",
                        "text": &line[last_end..full.start()],
                    }));
                }
                elements.push(json!({
                    "tag": "a",
                    "text": captures.get(1).expect("link text").as_str(),
                    "href": captures.get(2).expect("href").as_str(),
                }));
                last_end = full.end();
            }
            if last_end < line.len() {
                elements.push(json!({
                    "tag": "text",
                    "text": &line[last_end..],
                }));
            }
            if elements.is_empty() {
                elements.push(json!({
                    "tag": "text",
                    "text": "",
                }));
            }
            elements
        })
        .collect();

    json!({
        "zh_cn": {
            "content": paragraphs
        }
    })
    .to_string()
}

fn strip_md_formatting(text: &str) -> String {
    let mut result = text.to_string();
    result = result.replace("**", "").replace("__", "");
    result = result.replace('*', "");
    result = result.replace("~~", "");
    result
}

fn parse_md_table(table_text: &str) -> Value {
    let lines: Vec<&str> = table_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let split_row = |line: &str| -> Vec<String> {
        line.trim()
            .trim_matches('|')
            .split('|')
            .map(|cell| strip_md_formatting(cell.trim()))
            .collect()
    };

    let headers = split_row(lines[0]);
    let rows = lines[2..]
        .iter()
        .map(|line| split_row(line))
        .collect::<Vec<_>>();

    json!({
        "tag": "table",
        "page_size": rows.len() + 1,
        "columns": headers.iter().enumerate().map(|(index, header)| json!({
            "tag": "column",
            "name": format!("c{index}"),
            "display_name": header,
            "width": "auto"
        })).collect::<Vec<_>>(),
        "rows": rows.iter().map(|row| {
            let mut map = serde_json::Map::new();
            for (index, header) in headers.iter().enumerate() {
                let _ = header;
                map.insert(
                    format!("c{index}"),
                    Value::String(row.get(index).cloned().unwrap_or_default()),
                );
            }
            Value::Object(map)
        }).collect::<Vec<_>>()
    })
}

fn split_headings(content: &str) -> Vec<Value> {
    let mut protected = content.to_string();
    let mut code_blocks = Vec::new();
    for captures in code_block_re().find_iter(content) {
        code_blocks.push(captures.as_str().to_string());
        protected = protected.replacen(
            captures.as_str(),
            &format!("\u{0}CODE{}\u{0}", code_blocks.len() - 1),
            1,
        );
    }

    let mut elements = Vec::new();
    let mut last = 0;
    for heading in heading_re().find_iter(&protected) {
        let before = protected[last..heading.start()].trim();
        if !before.is_empty() {
            elements.push(json!({
                "tag": "markdown",
                "content": before,
            }));
        }

        let heading_text = heading.as_str().trim_start_matches('#').trim();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**{}**", strip_md_formatting(heading_text)),
            }
        }));
        last = heading.end();
    }
    let remaining = protected[last..].trim();
    if !remaining.is_empty() {
        elements.push(json!({
            "tag": "markdown",
            "content": remaining,
        }));
    }

    for (index, code_block) in code_blocks.iter().enumerate() {
        let placeholder = format!("\u{0}CODE{index}\u{0}");
        for element in &mut elements {
            if let Some(content) = element.get_mut("content") {
                if let Some(as_str) = content.as_str() {
                    *content = Value::String(as_str.replace(&placeholder, code_block));
                }
            }
        }
    }

    if elements.is_empty() {
        vec![json!({
            "tag": "markdown",
            "content": content,
        })]
    } else {
        elements
    }
}

fn build_card_elements(content: &str) -> Vec<Value> {
    let mut elements = Vec::new();
    let mut last = 0;
    for table in table_block_re().find_iter(content) {
        let before = content[last..table.start()].trim();
        if !before.is_empty() {
            elements.extend(split_headings(before));
        }
        elements.push(parse_md_table(table.as_str()));
        last = table.end();
    }
    let remaining = content[last..].trim();
    if !remaining.is_empty() {
        elements.extend(split_headings(remaining));
    }
    if elements.is_empty() {
        vec![json!({
            "tag": "markdown",
            "content": content,
        })]
    } else {
        elements
    }
}

fn render_interactive_cards(content: &str) -> Vec<String> {
    let elements = build_card_elements(content);
    let mut groups: Vec<Vec<Value>> = Vec::new();
    let mut current = Vec::new();
    let mut table_count = 0;

    for element in elements {
        let is_table = element.get("tag").and_then(Value::as_str) == Some("table");
        if is_table && table_count >= 1 {
            groups.push(current);
            current = Vec::new();
            table_count = 0;
        }
        if is_table {
            table_count += 1;
        }
        current.push(element);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    groups
        .into_iter()
        .map(|group| {
            json!({
                "config": { "wide_screen_mode": true },
                "elements": group,
            })
            .to_string()
        })
        .collect()
}

fn extract_post_text(payload: &Value) -> String {
    let root = payload.get("post").unwrap_or(payload);
    let Some(root_object) = root.as_object() else {
        return String::new();
    };

    let block = ["zh_cn", "en_us", "ja_jp"]
        .iter()
        .find_map(|key| root_object.get(*key).and_then(Value::as_object))
        .or_else(|| root_object.values().find_map(|value| value.as_object()));

    let Some(block) = block else {
        return String::new();
    };

    let mut lines = Vec::new();
    if let Some(title) = block.get("title").and_then(Value::as_str) {
        if !title.trim().is_empty() {
            lines.push(title.trim().to_string());
        }
    }

    if let Some(rows) = block.get("content").and_then(Value::as_array) {
        for row in rows {
            let Some(elements) = row.as_array() else {
                continue;
            };
            let mut row_parts = Vec::new();
            for element in elements {
                let Some(tag) = element.get("tag").and_then(Value::as_str) else {
                    continue;
                };
                match tag {
                    "text" => {
                        if let Some(text) = element.get("text").and_then(Value::as_str) {
                            row_parts.push(text.to_string());
                        }
                    }
                    "a" => {
                        if let Some(text) = element.get("text").and_then(Value::as_str) {
                            row_parts.push(text.to_string());
                        }
                    }
                    "at" => {
                        let user_name = element
                            .get("user_name")
                            .and_then(Value::as_str)
                            .unwrap_or("user");
                        row_parts.push(format!("@{user_name}"));
                    }
                    "code_block" => {
                        let language = element
                            .get("language")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let text = element.get("text").and_then(Value::as_str).unwrap_or("");
                        row_parts.push(format!("```{language}\n{text}\n```"));
                    }
                    "img" => {}
                    _ => {}
                }
            }
            let row_text = row_parts.join(" ").trim().to_string();
            if !row_text.is_empty() {
                lines.push(row_text);
            }
        }
    }

    lines.join("\n").trim().to_string()
}

fn is_allowed_sender(config: &FeishuConfig, sender_id: &str) -> bool {
    if config.allow_from.is_empty() {
        return false;
    }
    config
        .allow_from
        .iter()
        .any(|allowed| allowed == "*" || allowed == sender_id)
}

fn is_group_message_for_bot(
    raw_content: &str,
    mentions: &[Value],
    bot_open_id: Option<&str>,
    policy: &str,
) -> bool {
    if policy == "open" {
        return true;
    }
    if raw_content.contains("@_all") {
        return true;
    }
    let Some(bot_open_id) = bot_open_id else {
        return false;
    };
    mentions.iter().any(|mention| {
        mention
            .pointer("/id/open_id")
            .and_then(Value::as_str)
            .map(|open_id| open_id == bot_open_id)
            .unwrap_or(false)
    })
}

fn resolve_receive_id_type(chat_id: &str) -> &'static str {
    if chat_id.starts_with("ou_") {
        "open_id"
    } else {
        "chat_id"
    }
}

fn placeholder_message(message_type: &str) -> Option<String> {
    match message_type {
        "image"
        | "audio"
        | "file"
        | "sticker"
        | "interactive"
        | "share_chat"
        | "share_user"
        | "share_calendar_event"
        | "system"
        | "merge_forward" => Some(format!("[{message_type}]")),
        _ => None,
    }
}

fn normalize_inbound_content(message_type: &str, raw_content: &str) -> Result<Option<String>> {
    match message_type {
        "text" => {
            let payload: Value =
                serde_json::from_str(raw_content).context("invalid feishu text content")?;
            Ok(payload
                .get("text")
                .and_then(Value::as_str)
                .map(|text| text.trim().to_string()))
        }
        "post" => {
            let payload: Value =
                serde_json::from_str(raw_content).context("invalid feishu post content")?;
            Ok(Some(extract_post_text(&payload)))
        }
        other => Ok(placeholder_message(other)),
    }
}

#[async_trait::async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn start(&self) -> Result<()> {
        self.validate_startup_config()?;
        self.running.store(true, Ordering::SeqCst);
        let _ = self.bot_open_id().await?;
        while self.running.load(Ordering::SeqCst) {
            if let Err(error) = self.run_session().await {
                warn!("feishu channel session ended: {error}");
            }
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(FEISHU_RECONNECT_DELAY).await;
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        if !should_deliver_to_channel("feishu", &msg.metadata) {
            return Ok(());
        }

        if msg.chat_id.trim().is_empty() {
            bail!("feishu chat_id is required");
        }

        let access_token = self.tenant_access_token().await?;
        let _ = self.bot_open_id().await?;
        let receive_id_type = resolve_receive_id_type(&msg.chat_id);

        let payloads: Vec<(&'static str, String)> = match detect_message_format(&msg.content) {
            FeishuMessageFormat::Text => vec![(
                "text",
                json!({
                    "text": msg.content.trim(),
                })
                .to_string(),
            )],
            FeishuMessageFormat::Post => vec![("post", render_post_body(&msg.content))],
            FeishuMessageFormat::Interactive => render_interactive_cards(&msg.content)
                .into_iter()
                .map(|card| ("interactive", card))
                .collect(),
        };

        let reply_message_id = if self.config.reply_to_message
            && !msg
                .metadata
                .get("_progress")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && !msg
                .metadata
                .get("_tool_hint")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            msg.metadata.get("message_id").and_then(Value::as_str)
        } else {
            None
        };

        for (index, (msg_type, content)) in payloads.iter().enumerate() {
            let use_reply = index == 0 && reply_message_id.is_some();
            if use_reply {
                let message_id = reply_message_id.expect("reply id");
                if self
                    .reply_message(&access_token, message_id, msg_type, content)
                    .await
                    .is_ok()
                {
                    continue;
                }
            }
            self.create_message(
                &access_token,
                receive_id_type,
                &msg.chat_id,
                msg_type,
                content,
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn feishu_detects_plain_long_content_as_post() {
        let content = "a".repeat(300);
        assert_eq!(detect_message_format(&content), FeishuMessageFormat::Post);
    }

    #[test]
    fn feishu_detects_code_block_as_interactive() {
        assert_eq!(
            detect_message_format("```rust\nfn main() {}\n```"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_heading_as_interactive() {
        assert_eq!(
            detect_message_format("# Title\nbody"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_markdown_link_as_post() {
        assert_eq!(
            detect_message_format("[docs](https://example.com)"),
            FeishuMessageFormat::Post
        );
    }

    #[test]
    fn feishu_detects_threshold_boundaries() {
        assert_eq!(
            detect_message_format(&"a".repeat(200)),
            FeishuMessageFormat::Text
        );
        assert_eq!(
            detect_message_format(&"a".repeat(201)),
            FeishuMessageFormat::Post
        );
        assert_eq!(
            detect_message_format(&"a".repeat(2001)),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_lists_and_style_markers_as_interactive() {
        assert_eq!(
            detect_message_format("- item"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("1. item"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("**bold**"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("*italic*"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("~~strike~~"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_tables_and_splits_multiple_tables() {
        let table = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(
            detect_message_format(table),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            render_interactive_cards(&format!("{table}\n\n{table}")).len(),
            2
        );
    }

    #[test]
    fn feishu_renders_interactive_cards_with_required_schema() {
        let cards = render_interactive_cards("# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        let first: Value = serde_json::from_str(&cards[0]).expect("card json");
        assert_eq!(first["config"]["wide_screen_mode"], true);
        assert!(first["elements"].as_array().is_some());
    }

    #[test]
    fn feishu_flattens_post_content_deterministically() {
        let payload = json!({
            "zh_cn": {
                "title": "Title",
                "content": [[
                    {"tag": "text", "text": "hello"},
                    {"tag": "a", "text": "docs", "href": "https://example.com"},
                    {"tag": "at", "user_name": "bot"}
                ]]
            }
        });

        assert_eq!(extract_post_text(&payload), "Title\nhello docs @bot");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_locale_fallback() {
        let payload = json!({
            "post": {
                "en_us": {
                    "content": [[{"tag": "text", "text": "english"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "english");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_ja_locale_fallback() {
        let payload = json!({
            "post": {
                "ja_jp": {
                    "content": [[{"tag": "text", "text": "japanese"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "japanese");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_first_object_locale_fallback() {
        let payload = json!({
            "post": {
                "custom_locale": {
                    "content": [[{"tag": "text", "text": "custom"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "custom");
    }

    #[test]
    fn feishu_flattens_post_code_blocks_and_ignores_images() {
        let payload = json!({
            "zh_cn": {
                "content": [[
                    {"tag": "code_block", "language": "rust", "text": "fn main() {}"},
                    {"tag": "img", "image_key": "img_1"}
                ]]
            }
        });

        assert_eq!(extract_post_text(&payload), "```rust\nfn main() {}\n```");
    }

    #[test]
    fn feishu_allowlist_denies_empty_and_accepts_wildcard() {
        let mut config = FeishuConfig::default();
        assert!(!is_allowed_sender(&config, "ou_user_1"));
        config.allow_from = vec!["*".to_string()];
        assert!(is_allowed_sender(&config, "ou_user_1"));
    }

    #[test]
    fn feishu_open_group_policy_accepts_unmentioned_group_messages() {
        assert!(is_group_message_for_bot(
            "hello group",
            &[],
            Some("ou_bot_1"),
            "open"
        ));
    }

    #[test]
    fn feishu_mention_group_policy_requires_all_or_bot_open_id() {
        assert!(!is_group_message_for_bot(
            "hello group",
            &[],
            Some("ou_bot_1"),
            "mention",
        ));
        assert!(is_group_message_for_bot(
            "@_all hello",
            &[],
            Some("ou_bot_1"),
            "mention",
        ));
        assert!(is_group_message_for_bot(
            "hello",
            &[json!({"id": {"open_id": "ou_bot_1"}})],
            Some("ou_bot_1"),
            "mention",
        ));
    }

    #[test]
    fn feishu_resolves_receive_id_type_by_chat_id_prefix() {
        assert_eq!(resolve_receive_id_type("ou_user_1"), "open_id");
        assert_eq!(resolve_receive_id_type("oc_group_1"), "chat_id");
    }

    #[test]
    fn feishu_dedup_cache_evicts_oldest_entries() {
        let mut dedup = RecentMessageDedup::new(2);
        assert!(dedup.insert("m1"));
        assert!(dedup.insert("m2"));
        assert!(!dedup.insert("m1"));
        assert!(dedup.insert("m3"));
        assert!(dedup.insert("m1"));
    }
}
