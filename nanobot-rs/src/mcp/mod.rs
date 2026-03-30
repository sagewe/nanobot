//! MCP client: connects to MCP servers and wraps their tools as native nanobot tools.
//!
//! Mirrors `nanobot/agent/tools/mcp.py`.  Supports stdio and streamable-HTTP
//! transports.  SSE (legacy) servers are also accessed via streamable-HTTP
//! when the URL ends with `/sse`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, RawContent};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use serde::Serialize;

use crate::config::McpServerConfig;
use crate::tools::{Tool, ToolContext, ToolRegistry};

/// Timeout for the initial MCP server handshake (transport connect + initialize).
const CONNECT_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// McpToolWrapper — cheap to clone (all Arc/String inside)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct McpToolWrapper {
    peer: Arc<rmcp::Peer<RoleClient>>,
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
        let arguments = match args {
            Value::Object(map) => Some(map),
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
            self.peer.call_tool(params),
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
}

/// Info about a single MCP tool, exposed via the web API.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolInfo {
    pub name: String,
    pub original_name: String,
    pub description: String,
}

/// Info about a connected MCP server, exposed via the web API.
#[derive(Debug, Clone, Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub tool_count: usize,
    pub tools: Vec<McpToolInfo>,
}

impl McpClients {
    /// Register clones of all MCP tool wrappers into `registry`.
    pub async fn register_tools(&self, registry: &ToolRegistry) {
        for wrapper in &self.wrappers {
            registry.register(wrapper.clone()).await;
        }
    }

    pub fn tool_count(&self) -> usize {
        self.wrappers.len()
    }

    /// Group wrappers by server name and return summary info.
    pub fn list_servers(&self) -> Vec<McpServerInfo> {
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
            });
        }
        order.into_iter()
            .map(|name| {
                let tools = map.remove(&name).unwrap_or_default();
                McpServerInfo { tool_count: tools.len(), name, tools }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// connect_mcp_servers
// ---------------------------------------------------------------------------

/// Connect to every configured MCP server, register their tools, and return
/// an `McpClients` handle that must be kept alive for the agent's lifetime.
pub async fn connect_mcp_servers(
    servers: &HashMap<String, McpServerConfig>,
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

    let config = StreamableHttpClientTransportConfig::with_uri(url)
        .custom_headers(custom_headers);

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
            peer: Arc::clone(peer),
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
