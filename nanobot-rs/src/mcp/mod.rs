//! MCP client: connects to MCP servers and wraps their tools as native nanobot tools.
//!
//! Mirrors `nanobot/agent/tools/mcp.py`.  Supports stdio and streamable-HTTP
//! transports.  SSE (legacy) servers are also accessed via streamable-HTTP
//! when the URL ends with `/sse`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, RawContent};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use crate::config::McpServerConfig;
use crate::tools::{Tool, ToolContext, ToolRegistry};

/// Timeout for the initial MCP server handshake (transport connect + initialize).
const CONNECT_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// McpToolWrapper — cheap to clone (all Arc/String inside)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct McpToolWrapper {
    peer: Option<Arc<rmcp::Peer<RoleClient>>>,
    /// Name exposed to the agent: `mcp_{server}_{original}`.
    tool_name: String,
    original_name: String,
    server_name: String,
    description: String,
    schema: Value,
    timeout_secs: u64,
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> String {
        let Some(peer) = &self.peer else {
            return "(MCP test tool cannot execute)".to_string();
        };
        debug!(tool = %self.tool_name, args = %args, "MCP tool execute called");
        let arguments = match args {
            Value::Object(map) => {
                // Strip out empty-string values — some MCP servers (e.g. Home Assistant)
                // reject slots whose value is "" and return "invalid slot info".
                let filtered: Map<String, Value> = map
                    .into_iter()
                    .filter(|(_, v)| v != &Value::String(String::new()))
                    .collect();
                Some(filtered)
            }
            Value::Null => None,
            other => {
                let mut m = Map::new();
                m.insert("input".to_string(), other);
                Some(m)
            }
        };

        let mut params = CallToolRequestParams::new(self.original_name.clone());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }

        let result = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            peer.call_tool(params),
        )
        .await;

        match result {
            Err(_) => {
                warn!(
                    tool = %self.tool_name,
                    timeout = self.timeout_secs,
                    "MCP tool call timed out"
                );
                format!("(MCP tool call timed out after {}s)", self.timeout_secs)
            }
            Ok(Err(e)) => {
                error!(tool = %self.tool_name, error = %e, "MCP tool call failed");
                format!("(MCP tool call failed: {e})")
            }
            Ok(Ok(result)) => {
                let parts: Vec<String> = result
                    .content
                    .into_iter()
                    .map(|block| match block.raw {
                        RawContent::Text(t) => t.text,
                        other => format!("{other:?}"),
                    })
                    .collect();
                let text = if parts.is_empty() {
                    "(no output)".to_string()
                } else {
                    parts.join("\n")
                };
                if result.is_error == Some(true) {
                    format!("(MCP tool error: {text})")
                } else {
                    text
                }
            }
        }
    }

    async fn set_context(&self, _context: ToolContext) {}
}

// ---------------------------------------------------------------------------
// McpClients — keeps running services alive AND pre-built wrappers for reuse
// ---------------------------------------------------------------------------

/// Holds all live MCP client sessions.  Drop to disconnect from all servers.
/// Call `register_tools` before each agent turn to populate a fresh registry.
pub struct McpClients {
    /// Keeps the background I/O tasks alive.
    /// Wrapped in Arc<Mutex<...>> so McpClients is Sync (required for tokio::spawn futures).
    _services: Arc<std::sync::Mutex<Vec<Box<dyn std::any::Any + Send>>>>,
    /// Pre-built wrappers — cheap to clone into each fresh ToolRegistry.
    wrappers: Vec<McpToolWrapper>,
    /// Tool names (full `mcp_{server}_{tool}` form) that are disabled by the user.
    disabled_tools: Arc<std::sync::Mutex<HashSet<String>>>,
    state_path: Option<PathBuf>,
}

/// Info about a single MCP tool, exposed via the web API.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolInfo {
    pub name: String,
    pub original_name: String,
    pub description: String,
    pub enabled: bool,
}

/// Info about a connected MCP server, exposed via the web API.
#[derive(Debug, Clone, Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub tool_count: usize,
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpServerToolAction {
    EnableAll,
    DisableAll,
    Reset,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpToolStateStore {
    #[serde(default = "default_mcp_state_version")]
    version: u32,
    #[serde(default)]
    disabled_tools: Vec<String>,
}

fn default_mcp_state_version() -> u32 {
    1
}

impl McpClients {
    /// Register clones of all enabled MCP tool wrappers into `registry`.
    pub async fn register_tools(&self, registry: &ToolRegistry) {
        let wrappers: Vec<McpToolWrapper> = {
            let disabled = self.disabled_tools.lock().unwrap();
            self.wrappers
                .iter()
                .filter(|wrapper| !disabled.contains(&wrapper.tool_name))
                .cloned()
                .collect()
        };
        for wrapper in wrappers {
            registry.register(wrapper).await;
        }
    }

    pub fn tool_count(&self) -> usize {
        self.wrappers.len()
    }

    /// Enable or disable a tool by its full name (`mcp_{server}_{tool}`).
    /// Returns `false` if the tool name is not found.
    pub fn toggle_tool(&self, name: &str, enabled: bool) -> Result<bool> {
        let exists = self.wrappers.iter().any(|w| w.tool_name == name);
        if !exists {
            return Ok(false);
        }
        self.update_disabled_tools(|disabled| {
            if enabled {
                disabled.remove(name);
            } else {
                disabled.insert(name.to_string());
            }
        })?;
        Ok(true)
    }

    /// Apply a bulk action to all active tools for a server.
    /// Returns `false` if the server name is not found.
    pub fn apply_server_action(
        &self,
        server_name: &str,
        action: McpServerToolAction,
    ) -> Result<bool> {
        let tool_names = self.server_tool_names(server_name);
        if tool_names.is_empty() {
            return Ok(false);
        }
        let prefix = server_tool_prefix(server_name);
        self.update_disabled_tools(|disabled| match action {
            McpServerToolAction::EnableAll => {
                for tool_name in &tool_names {
                    disabled.remove(tool_name);
                }
            }
            McpServerToolAction::DisableAll => {
                for tool_name in &tool_names {
                    disabled.insert(tool_name.clone());
                }
            }
            McpServerToolAction::Reset => {
                disabled.retain(|tool_name| !tool_name.starts_with(&prefix));
            }
        })?;
        Ok(true)
    }

    /// Group wrappers by server name and return summary info.
    pub fn list_servers(&self) -> Vec<McpServerInfo> {
        let disabled = self.disabled_tools.lock().unwrap();
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, Vec<McpToolInfo>> = HashMap::new();
        for w in &self.wrappers {
            let server_name = w.server_name.clone();
            if !map.contains_key(&server_name) {
                order.push(server_name.clone());
            }
            map.entry(server_name).or_default().push(McpToolInfo {
                name: w.tool_name.clone(),
                original_name: w.original_name.clone(),
                description: w.description.clone(),
                enabled: !disabled.contains(&w.tool_name),
            });
        }
        order
            .into_iter()
            .map(|name| {
                let tools = map.remove(&name).unwrap_or_default();
                McpServerInfo {
                    tool_count: tools.len(),
                    name,
                    tools,
                }
            })
            .collect()
    }

    fn server_tool_names(&self, server_name: &str) -> Vec<String> {
        self.wrappers
            .iter()
            .filter(|wrapper| wrapper.server_name == server_name)
            .map(|wrapper| wrapper.tool_name.clone())
            .collect()
    }

    fn update_disabled_tools<F>(&self, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut HashSet<String>),
    {
        let mut current = self.disabled_tools.lock().unwrap();
        let mut next = current.clone();
        mutate(&mut next);
        self.save_disabled_tools(&next)?;
        *current = next;
        Ok(())
    }

    fn save_disabled_tools(&self, disabled: &HashSet<String>) -> Result<()> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut disabled_tools: Vec<String> = disabled.iter().cloned().collect();
        disabled_tools.sort();
        let payload = McpToolStateStore {
            version: default_mcp_state_version(),
            disabled_tools,
        };
        std::fs::write(path, serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// connect_mcp_servers
// ---------------------------------------------------------------------------

/// Connect to every configured MCP server, register their tools, and return
/// an `McpClients` handle that must be kept alive for the agent's lifetime.
pub async fn connect_mcp_servers(
    servers: &HashMap<String, McpServerConfig>,
    state_path: Option<PathBuf>,
) -> McpClients {
    let mut services: Vec<Box<dyn std::any::Any + Send>> = Vec::new();
    let mut all_wrappers: Vec<McpToolWrapper> = Vec::new();

    for (name, cfg) in servers {
        match connect_one(name, cfg).await {
            Ok((service, wrappers)) => {
                services.push(service);
                all_wrappers.extend(wrappers);
            }
            Err(e) => {
                error!(server = %name, error = %e, "Failed to connect to MCP server");
            }
        }
    }

    McpClients {
        _services: Arc::new(std::sync::Mutex::new(services)),
        wrappers: all_wrappers,
        disabled_tools: Arc::new(std::sync::Mutex::new(load_disabled_tools(
            state_path.as_deref(),
        ))),
        state_path,
    }
}

async fn connect_one(
    name: &str,
    cfg: &McpServerConfig,
) -> Result<(Box<dyn std::any::Any + Send>, Vec<McpToolWrapper>)> {
    let transport_type = resolve_transport_type(cfg)?;
    match transport_type.as_str() {
        "stdio" => connect_stdio(name, cfg).await,
        "streamableHttp" | "sse" => connect_http(name, cfg).await,
        other => Err(anyhow!("unknown MCP transport type '{other}'")),
    }
}

fn resolve_transport_type(cfg: &McpServerConfig) -> Result<String> {
    if let Some(t) = &cfg.transport_type {
        return Ok(t.clone());
    }
    if cfg.command.is_some() {
        return Ok("stdio".to_string());
    }
    if let Some(url) = &cfg.url {
        if url.trim_end_matches('/').ends_with("/sse") {
            return Ok("sse".to_string());
        }
        return Ok("streamableHttp".to_string());
    }
    Err(anyhow!("MCP server has neither 'command' nor 'url'"))
}

async fn connect_stdio(
    name: &str,
    cfg: &McpServerConfig,
) -> Result<(Box<dyn std::any::Any + Send>, Vec<McpToolWrapper>)> {
    let command = cfg
        .command
        .as_deref()
        .ok_or_else(|| anyhow!("stdio transport requires 'command'"))?;

    let mut cmd = Command::new(command);
    cmd.args(&cfg.args);
    for (k, v) in &cfg.env {
        cmd.env(k, v);
    }

    let transport = TokioChildProcess::new(cmd)?;
    let running = tokio::time::timeout(
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
        ().serve(transport),
    )
    .await
    .map_err(|_| anyhow!("timed out connecting to MCP server after {CONNECT_TIMEOUT_SECS}s"))??;
    let peer = Arc::new(running.peer().clone());
    let wrappers = build_wrappers(name, cfg, &peer).await;
    Ok((Box::new(running), wrappers))
}

async fn connect_http(
    name: &str,
    cfg: &McpServerConfig,
) -> Result<(Box<dyn std::any::Any + Send>, Vec<McpToolWrapper>)> {
    use http::{HeaderName, HeaderValue};
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    let url = cfg
        .url
        .as_deref()
        .ok_or_else(|| anyhow!("HTTP transport requires 'url'"))?;

    let custom_headers: HashMap<HeaderName, HeaderValue> = cfg
        .headers
        .iter()
        .filter_map(|(k, v)| {
            let name = HeaderName::from_bytes(k.as_bytes()).ok()?;
            let val = HeaderValue::from_str(v).ok()?;
            Some((name, val))
        })
        .collect();

    let config = StreamableHttpClientTransportConfig::with_uri(url).custom_headers(custom_headers);

    let transport = StreamableHttpClientTransport::from_config(config);

    let running = tokio::time::timeout(
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
        ().serve(transport),
    )
    .await
    .map_err(|_| anyhow!("timed out connecting to MCP server after {CONNECT_TIMEOUT_SECS}s"))??;
    let peer = Arc::new(running.peer().clone());
    let wrappers = build_wrappers(name, cfg, &peer).await;
    Ok((Box::new(running), wrappers))
}

async fn build_wrappers(
    server_name: &str,
    cfg: &McpServerConfig,
    peer: &Arc<rmcp::Peer<RoleClient>>,
) -> Vec<McpToolWrapper> {
    let allow_all = cfg.enabled_tools.contains(&"*".to_string());
    let enabled: std::collections::HashSet<&str> =
        cfg.enabled_tools.iter().map(String::as_str).collect();

    let tools = match peer.list_all_tools().await {
        Ok(t) => t,
        Err(e) => {
            error!(server = %server_name, error = %e, "Failed to list MCP tools");
            return Vec::new();
        }
    };

    let mut wrappers = Vec::new();
    let mut unmatched: std::collections::HashSet<&str> = if allow_all {
        Default::default()
    } else {
        enabled.clone()
    };

    for tool_def in tools {
        let wrapped_name = format!("mcp_{server_name}_{}", tool_def.name);
        if !allow_all
            && !enabled.contains(tool_def.name.as_ref())
            && !enabled.contains(wrapped_name.as_str())
        {
            debug!(tool = %wrapped_name, "skipping MCP tool (not in enabledTools)");
            continue;
        }
        unmatched.remove(tool_def.name.as_ref());
        unmatched.remove(wrapped_name.as_str());

        let schema = Value::Object(tool_def.input_schema.as_ref().clone());
        wrappers.push(McpToolWrapper {
            peer: Some(Arc::clone(peer)),
            tool_name: wrapped_name,
            original_name: tool_def.name.to_string(),
            server_name: server_name.to_string(),
            description: tool_def
                .description
                .as_deref()
                .unwrap_or(&tool_def.name)
                .to_string(),
            schema,
            timeout_secs: cfg.tool_timeout,
        });
    }

    if !unmatched.is_empty() {
        warn!(
            server = %server_name,
            unmatched = ?unmatched,
            "Some enabledTools entries were not found on server"
        );
    }

    info!(
        server = %server_name,
        count = wrappers.len(),
        "MCP server connected"
    );
    wrappers
}

fn server_tool_prefix(server_name: &str) -> String {
    format!("mcp_{server_name}_")
}

fn load_disabled_tools(path: Option<&Path>) -> HashSet<String> {
    let Some(path) = path else {
        return HashSet::new();
    };
    if !path.exists() {
        return HashSet::new();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            warn!(path = %path.display(), error = %error, "Failed to read MCP tool state");
            return HashSet::new();
        }
    };
    match serde_json::from_str::<McpToolStateStore>(&text) {
        Ok(store) => store.disabled_tools.into_iter().collect(),
        Err(error) => {
            warn!(path = %path.display(), error = %error, "Failed to parse MCP tool state");
            HashSet::new()
        }
    }
}

#[cfg(test)]
pub(crate) fn test_clients(
    tools: &[(&str, &str, &str)],
    disabled_tools: &[&str],
    state_path: Option<PathBuf>,
) -> McpClients {
    let wrappers = tools
        .iter()
        .map(|(server_name, original_name, description)| McpToolWrapper {
            peer: None,
            tool_name: format!("mcp_{server_name}_{original_name}"),
            original_name: (*original_name).to_string(),
            description: (*description).to_string(),
            schema: Value::Object(Map::new()),
            timeout_secs: 30,
        })
        .collect();
    McpClients {
        _services: Arc::new(std::sync::Mutex::new(Vec::new())),
        wrappers,
        disabled_tools: Arc::new(std::sync::Mutex::new(
            disabled_tools
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        )),
        state_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reset_server_clears_hidden_overrides_while_enable_all_only_clears_visible_tools() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("mcp").join("tools.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&McpToolStateStore {
                version: default_mcp_state_version(),
                disabled_tools: vec![
                    "mcp_demo_fetch".to_string(),
                    "mcp_demo_hidden".to_string(),
                    "mcp_other_search".to_string(),
                ],
            })
            .expect("serialize store"),
        )
        .expect("write store");

        let clients = McpClients {
            _services: Arc::new(std::sync::Mutex::new(Vec::new())),
            wrappers: vec![
                McpToolWrapper {
                    peer: None,
                    tool_name: "mcp_demo_search".to_string(),
                    original_name: "search".to_string(),
                    description: "Search".to_string(),
                    schema: Value::Object(Map::new()),
                    timeout_secs: 30,
                },
                McpToolWrapper {
                    peer: None,
                    tool_name: "mcp_demo_fetch".to_string(),
                    original_name: "fetch".to_string(),
                    description: "Fetch".to_string(),
                    schema: Value::Object(Map::new()),
                    timeout_secs: 30,
                },
            ],
            disabled_tools: Arc::new(std::sync::Mutex::new(load_disabled_tools(Some(&path)))),
            state_path: Some(path.clone()),
        };

        clients
            .apply_server_action("demo", McpServerToolAction::EnableAll)
            .expect("enable all");
        let after_enable_all = load_disabled_tools(Some(&path));
        assert!(after_enable_all.contains("mcp_demo_hidden"));
        assert!(!after_enable_all.contains("mcp_demo_fetch"));
        assert!(after_enable_all.contains("mcp_other_search"));

        clients
            .apply_server_action("demo", McpServerToolAction::Reset)
            .expect("reset");
        let after_reset = load_disabled_tools(Some(&path));
        assert!(!after_reset.contains("mcp_demo_hidden"));
        assert!(!after_reset.contains("mcp_demo_fetch"));
        assert!(after_reset.contains("mcp_other_search"));
    }
}
