use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use nanobot_rs::agent::AgentLoop;
use nanobot_rs::bus::MessageBus;
use nanobot_rs::channels::weixin::{WeixinAccountState, WeixinAccountStore};
use nanobot_rs::config::{AgentProfileConfig, Config, WebToolsConfig, WeixinConfig};
use nanobot_rs::control::{BootstrapAdmin, ControlStore, Role, RuntimeManager};
use nanobot_rs::mcp::{McpServerInfo, McpServerToolAction, McpToolInfo};
use nanobot_rs::providers::{LlmProvider, LlmResponse, ToolCall};
use nanobot_rs::session::{Session, SessionMessage, SessionStore};
use nanobot_rs::web::{
    self, AgentChatService, AppState, ChatService, WebChatReply, WebSessionDetail,
};
use serde_json::{Map, json};
use std::collections::{HashMap, VecDeque};
use tempfile::{TempDir, tempdir};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower::util::ServiceExt;

#[derive(Clone, Default)]
struct StaticChatService;

#[async_trait]
impl ChatService for StaticChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        Ok(WebChatReply {
            reply: "unused".to_string(),
            active_profile: "openai:mock-model".to_string(),
            persisted: true,
        })
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state() -> AppState {
    AppState::new(Arc::new(StaticChatService), None)
}

fn multiuser_state() -> (AppState, TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");
    let manager = RuntimeManager::new(store.clone(), false);
    let _ = admin;
    (AppState::with_control(store, manager), dir)
}

async fn login_cookie(app: &Router, username: &str, password: &str) -> String {
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"username":"{username}","password":"{password}"}}"#
                )))
                .unwrap(),
        )
        .await
        .expect("login");
    login
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .expect("set-cookie")
        .to_string()
}

fn write_skill(root: &Path, id: &str, content: &str) {
    let skill_dir = root.join(id);
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(skill_dir.join("SKILL.md"), content).expect("write skill");
}

fn unique_skill_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        Utc::now()
            .timestamp_nanos_opt()
            .expect("timestamp nanos available")
    )
}

struct TempBuiltinSkill {
    path: PathBuf,
}

impl TempBuiltinSkill {
    fn new(id: &str, content: &str) -> Self {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join(id);
        if path.exists() {
            std::fs::remove_dir_all(&path).expect("remove stale builtin skill");
        }
        std::fs::create_dir_all(&path).expect("create builtin skill dir");
        std::fs::write(path.join("SKILL.md"), content).expect("write builtin skill");
        Self { path }
    }
}

impl Drop for TempBuiltinSkill {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
struct ReplyChatService {
    reply: String,
    persisted: bool,
}

#[async_trait]
impl ChatService for ReplyChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        Ok(WebChatReply {
            reply: self.reply.clone(),
            active_profile: "openai:mock-model".to_string(),
            persisted: self.persisted,
        })
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state_with_reply(reply: &str) -> AppState {
    AppState::new(
        Arc::new(ReplyChatService {
            reply: reply.to_string(),
            persisted: true,
        }),
        None,
    )
}

fn test_state_with_ephemeral_reply(reply: &str) -> AppState {
    AppState::new(
        Arc::new(ReplyChatService {
            reply: reply.to_string(),
            persisted: false,
        }),
        None,
    )
}

#[derive(Clone)]
struct ErrorChatService;

#[async_trait]
impl ChatService for ErrorChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        anyhow::bail!("provider exploded")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state_with_error() -> AppState {
    AppState::new(Arc::new(ErrorChatService), None)
}

#[derive(Clone)]
struct MockMcpTool {
    name: String,
    original_name: String,
    description: String,
}

#[derive(Clone)]
struct McpChatService {
    server_name: String,
    tools: Arc<Vec<MockMcpTool>>,
    enabled: Arc<Mutex<HashMap<String, bool>>>,
}

impl McpChatService {
    fn new(server_name: &str, tools: Vec<MockMcpTool>, enabled: HashMap<String, bool>) -> Self {
        Self {
            server_name: server_name.to_string(),
            tools: Arc::new(tools),
            enabled: Arc::new(Mutex::new(enabled)),
        }
    }
}

#[async_trait]
impl ChatService for McpChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        anyhow::bail!("unused")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerInfo>> {
        let enabled = self.enabled.lock().await;
        Ok(vec![McpServerInfo {
            name: self.server_name.clone(),
            icon: None,
            tool_count: self.tools.len(),
            tools: self
                .tools
                .iter()
                .map(|tool| McpToolInfo {
                    name: tool.name.clone(),
                    original_name: tool.original_name.clone(),
                    description: tool.description.clone(),
                    enabled: *enabled.get(&tool.name).unwrap_or(&true),
                })
                .collect(),
        }])
    }

    async fn toggle_mcp_tool(&self, name: &str, enabled: bool) -> Result<bool> {
        let mut states = self.enabled.lock().await;
        let Some(state) = states.get_mut(name) else {
            return Ok(false);
        };
        *state = enabled;
        Ok(true)
    }

    async fn apply_mcp_server_action(
        &self,
        name: &str,
        action: McpServerToolAction,
    ) -> Result<bool> {
        if name != self.server_name {
            return Ok(false);
        }
        let mut states = self.enabled.lock().await;
        match action {
            McpServerToolAction::EnableAll | McpServerToolAction::Reset => {
                for tool in self.tools.iter() {
                    states.insert(tool.name.clone(), true);
                }
            }
            McpServerToolAction::DisableAll => {
                for tool in self.tools.iter() {
                    states.insert(tool.name.clone(), false);
                }
            }
        }
        Ok(true)
    }
}

fn test_state_with_mcp_tools() -> AppState {
    let tools = vec![
        MockMcpTool {
            name: "mcp_demo_search".to_string(),
            original_name: "search".to_string(),
            description: "Search docs".to_string(),
        },
        MockMcpTool {
            name: "mcp_demo_fetch".to_string(),
            original_name: "fetch".to_string(),
            description: "Fetch content".to_string(),
        },
    ];
    let enabled = HashMap::from([
        ("mcp_demo_search".to_string(), true),
        ("mcp_demo_fetch".to_string(), false),
    ]);
    AppState::new(Arc::new(McpChatService::new("demo", tools, enabled)), None)
}

#[tokio::test]
async fn protected_multiuser_routes_require_authentication() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_sets_cookie_and_me_returns_authenticated_user() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("login");

    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .expect("set-cookie")
        .to_string();

    let me = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("me");

    assert_eq!(me.status(), StatusCode::OK);
    let body = to_bytes(me.into_body(), usize::MAX).await.expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["username"], json!("alice"));
    assert_eq!(payload["role"], json!("admin"));
}

#[tokio::test]
async fn me_config_returns_the_authenticated_users_private_config() {
    let (state, dir) = multiuser_state();
    let app = web::build_router(state);

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("login");
    let cookie = login
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .expect("set-cookie")
        .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/me/config")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("config");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let users =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users: serde_json::Value = serde_json::from_str(&users).expect("users json");
    let user_id = users["users"][0]["user_id"].as_str().expect("user id");
    assert_eq!(
        payload
            .pointer("/agents/defaults/workspace")
            .and_then(serde_json::Value::as_str),
        Some(
            dir.path()
                .join("users")
                .join(user_id)
                .join("workspace")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[tokio::test]
async fn put_my_config_accepts_structured_json_and_persists_the_toml_backing_file() {
    let (state, dir) = multiuser_state();
    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;
    let current = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me/config")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("get config");
    assert_eq!(current.status(), StatusCode::OK);
    let current_body = to_bytes(current.into_body(), usize::MAX)
        .await
        .expect("config body");
    let mut config: serde_json::Value = serde_json::from_slice(&current_body).expect("config json");
    config["channels"]["telegram"]["enabled"] = json!(true);
    config["channels"]["telegram"]["token"] = json!("token-123");

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/me/config")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(config.to_string()))
                .unwrap(),
        )
        .await
        .expect("config");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload["ok"], json!(true));

    let users =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users: serde_json::Value = serde_json::from_str(&users).expect("users json");
    let user_id = users["users"][0]["user_id"].as_str().expect("user id");
    let config_path = dir.path().join("users").join(user_id).join("config.toml");

    assert!(config_path.exists());
    let saved = std::fs::read_to_string(&config_path).expect("config toml");
    assert!(saved.contains("defaultProfile"));
    assert!(saved.contains("token-123"));
}

#[tokio::test]
async fn put_my_config_rejects_an_invalid_default_profile_with_a_json_error_envelope() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;

    let current = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me/config")
                .header("cookie", cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("get config");
    assert_eq!(current.status(), StatusCode::OK);
    let current_body = to_bytes(current.into_body(), usize::MAX)
        .await
        .expect("config body");
    let mut config: serde_json::Value = serde_json::from_slice(&current_body).expect("config json");
    config["agents"]["defaults"]["defaultProfile"] = json!("openai:not-a-real-profile");

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/me/config")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(config.to_string()))
                .unwrap(),
        )
        .await
        .expect("config");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["error"]
            .as_str()
            .expect("error message")
            .contains("defaultProfile"),
        true
    );
}

#[tokio::test]
async fn put_my_config_rejects_malformed_json_with_a_json_error_envelope() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/me/config")
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from("{not-json"))
                .unwrap(),
        )
        .await
        .expect("config");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert!(payload["error"].as_str().is_some());
}

#[tokio::test]
async fn admin_users_endpoint_lists_accounts_for_admins_only() {
    let (state, dir) = multiuser_state();
    let store = ControlStore::new(dir.path()).expect("control store");
    store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create user");
    let app = web::build_router(state);

    let admin_cookie = login_cookie(&app, "alice", "password123").await;
    let admin_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/users")
                .header("cookie", admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("admin response");
    assert_eq!(admin_response.status(), StatusCode::OK);

    let user_cookie = login_cookie(&app, "bob", "password456").await;
    let user_response = app
        .oneshot(
            Request::builder()
                .uri("/api/admin/users")
                .header("cookie", user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("user response");
    assert_eq!(user_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_endpoints_can_create_and_manage_users() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);
    let admin_cookie = login_cookie(&app, "alice", "password123").await;

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("content-type", "application/json")
                .header("cookie", admin_cookie.clone())
                .body(Body::from(
                    r#"{"username":"bob","displayName":"Bob","password":"password456","role":"user"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("create user");
    assert_eq!(create.status(), StatusCode::OK);
    let create_body = to_bytes(create.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: serde_json::Value =
        serde_json::from_slice(&create_body).expect("create payload");
    let bob_user_id = create_payload["user"]["userId"]
        .as_str()
        .expect("bob user id")
        .to_string();

    let bob_cookie = login_cookie(&app, "bob", "password456").await;

    let disable = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/admin/users/{bob_user_id}/disable"))
                .header("cookie", admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("disable");
    assert_eq!(disable.status(), StatusCode::OK);

    let disabled_me = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header("cookie", bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("disabled me");
    assert_eq!(disabled_me.status(), StatusCode::UNAUTHORIZED);

    for (uri, body) in [
        (
            format!("/api/admin/users/{bob_user_id}/password"),
            Some(r#"{"password":"newsecret789"}"#),
        ),
        (
            format!("/api/admin/users/{bob_user_id}/role"),
            Some(r#"{"role":"admin"}"#),
        ),
        (format!("/api/admin/users/{bob_user_id}/enable"), None),
    ] {
        let mut request = Request::builder().method("POST").uri(uri);
        request = request.header("cookie", admin_cookie.clone());
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from(body.unwrap_or_default().to_string()))
                    .unwrap(),
            )
            .await
            .expect("manage user");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let old_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"bob","password":"password456"}"#))
                .unwrap(),
        )
        .await
        .expect("old login");
    assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

    let new_cookie = login_cookie(&app, "bob", "newsecret789").await;
    let me = app
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header("cookie", new_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("me");
    assert_eq!(me.status(), StatusCode::OK);
    let me_body = to_bytes(me.into_body(), usize::MAX).await.expect("me body");
    let me_payload: serde_json::Value = serde_json::from_slice(&me_body).expect("me payload");
    assert_eq!(me_payload["role"], json!("admin"));
}

#[tokio::test]
async fn change_password_endpoint_rotates_credentials_for_the_authenticated_user() {
    let (state, _dir) = multiuser_state();
    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;

    let change = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(
                    r#"{"currentPassword":"password123","newPassword":"newpassword456"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("change password");
    assert_eq!(change.status(), StatusCode::OK);

    let old_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("old login");
    assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

    let new_login = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"newpassword456"}"#,
                ))
                .unwrap(),
        )
        .await
        .expect("new login");
    assert_eq!(new_login.status(), StatusCode::OK);
}

#[tokio::test]
async fn skills_api_lists_builtin_and_workspace_entries_for_authenticated_user() {
    let builtin_id = unique_skill_id("builtin-weather");
    let workspace_id = unique_skill_id("alice-weather");
    let other_workspace_id = unique_skill_id("bob-weather");
    let builtin_name = format!("Builtin Weather {builtin_id}");
    let workspace_name = format!("Alice Workspace Weather {workspace_id}");
    let _builtin = TempBuiltinSkill::new(
        &builtin_id,
        &format!("---\nname: {builtin_name}\ndescription: builtin weather\n---\n\nBuiltin body\n"),
    );

    let (state, dir) = multiuser_state();
    let store = ControlStore::new(dir.path()).expect("control store");
    let bob = store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create bob");
    let alice = store
        .get_user_by_username("alice")
        .expect("lookup alice")
        .expect("alice");
    write_skill(
        &store.user_workspace_path(&alice.user_id).join("skills"),
        &workspace_id,
        &format!(
            "---\nname: {workspace_name}\ndescription: alice workspace weather\n---\n\nAlice body\n"
        ),
    );
    write_skill(
        &store.user_workspace_path(&bob.user_id).join("skills"),
        &other_workspace_id,
        &format!(
            "---\nname: Bob Workspace Weather {other_workspace_id}\ndescription: bob workspace weather\n---\n\nBob body\n"
        ),
    );

    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/skills")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("skills list");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("skills list body");
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("skills list json");
    let workspace = payload["workspace"]
        .as_array()
        .expect("workspace skills array");
    let builtin = payload["builtin"].as_array().expect("builtin skills array");

    assert!(workspace.iter().any(|skill| {
        skill["id"] == json!(workspace_id) && skill["name"] == json!(workspace_name)
    }));
    assert!(
        !workspace
            .iter()
            .any(|skill| skill["id"] == json!(other_workspace_id))
    );
    assert!(
        builtin
            .iter()
            .any(|skill| skill["id"] == json!(builtin_id) && skill["readOnly"] == json!(true))
    );
}

#[tokio::test]
async fn skills_api_toggles_workspace_state_in_single_user_mode_without_rewriting_skill_body() {
    let dir = tempdir().expect("tempdir");
    let raw_content = "---\nname: local weather\ndescription: workspace weather\n---\n\nBody line 1\nBody line 2\n";
    let skill_path = dir
        .path()
        .join("skills")
        .join("local-weather")
        .join("SKILL.md");
    write_skill(
        dir.path().join("skills").as_path(),
        "local-weather",
        raw_content,
    );
    let app = agent_app(&dir, Vec::new()).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/skills/workspace/local-weather/state")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":false}"#))
                .unwrap(),
        )
        .await
        .expect("toggle workspace state");

    assert_eq!(response.status(), StatusCode::OK);
    let state_path = dir.path().join(".nanobot").join("skills-state.json");
    let state_raw = std::fs::read_to_string(&state_path).expect("state file");
    let state_json: serde_json::Value = serde_json::from_str(&state_raw).expect("state json");
    assert_eq!(state_json["local-weather"]["enabled"], json!(false));
    assert_eq!(
        std::fs::read_to_string(&skill_path).expect("workspace skill after toggle"),
        raw_content
    );
}

#[tokio::test]
async fn cron_jobs_are_scoped_to_the_authenticated_user_runtime() {
    let (state, dir) = multiuser_state();
    let store = ControlStore::new(dir.path()).expect("control store");
    store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create bob");
    let app = web::build_router(state);

    let alice_cookie = login_cookie(&app, "alice", "password123").await;
    let bob_cookie = login_cookie(&app, "bob", "password456").await;

    for (cookie, message) in [
        (alice_cookie.clone(), "alice task"),
        (bob_cookie.clone(), "bob task"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cron/jobs")
                    .header("content-type", "application/json")
                    .header("cookie", cookie)
                    .body(Body::from(format!(
                        r#"{{"message":"{message}","everySeconds":60}}"#
                    )))
                    .unwrap(),
            )
            .await
            .expect("add cron job");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let alice_jobs = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/cron/jobs")
                .header("cookie", alice_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("alice jobs");
    assert_eq!(alice_jobs.status(), StatusCode::OK);
    let alice_jobs_body = to_bytes(alice_jobs.into_body(), usize::MAX)
        .await
        .expect("alice jobs body");
    let alice_jobs_payload: serde_json::Value =
        serde_json::from_slice(&alice_jobs_body).expect("alice jobs json");
    assert_eq!(alice_jobs_payload["jobs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        alice_jobs_payload["jobs"][0]["payload"]["message"],
        json!("alice task")
    );

    let bob_jobs = app
        .oneshot(
            Request::builder()
                .uri("/api/cron/jobs")
                .header("cookie", bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("bob jobs");
    assert_eq!(bob_jobs.status(), StatusCode::OK);
    let bob_jobs_body = to_bytes(bob_jobs.into_body(), usize::MAX)
        .await
        .expect("bob jobs body");
    let bob_jobs_payload: serde_json::Value =
        serde_json::from_slice(&bob_jobs_body).expect("bob jobs json");
    assert_eq!(bob_jobs_payload["jobs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        bob_jobs_payload["jobs"][0]["payload"]["message"],
        json!("bob task")
    );
}

#[derive(Clone)]
struct WeixinLoginSnapshot {
    qrcode: String,
    qrcode_img_content: String,
    status: String,
}

struct WeixinAccountChatService {
    enabled: bool,
    store: WeixinAccountStore,
    _workspace: TempDir,
    login: Arc<Mutex<Option<WeixinLoginSnapshot>>>,
}

impl WeixinAccountChatService {
    fn new(enabled: bool, workspace: TempDir, store: WeixinAccountStore) -> Self {
        Self {
            enabled,
            store,
            _workspace: workspace,
            login: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl ChatService for WeixinAccountChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        anyhow::bail!("unused")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }

    async fn get_weixin_account(&self) -> Result<web::WebWeixinAccount> {
        let account = self.store.load_account()?;
        Ok(web::WebWeixinAccount::from_account(
            self.enabled,
            account.as_ref(),
        ))
    }

    async fn start_weixin_login(&self) -> Result<web::WeixinLoginStartResponse> {
        let login = WeixinLoginSnapshot {
            qrcode: "qr-token".to_string(),
            qrcode_img_content: "data:image/png;base64,abc".to_string(),
            status: "wait".to_string(),
        };
        *self.login.lock().await = Some(login.clone());
        Ok(web::WeixinLoginStartResponse {
            qrcode: login.qrcode,
            qrcode_img_content: login.qrcode_img_content,
        })
    }

    async fn poll_weixin_login(&self) -> Result<web::WebWeixinLoginStatus> {
        let login = self.login.lock().await.clone();
        let account = self.store.load_account()?;
        Ok(web::WebWeixinLoginStatus::from_state(
            login.as_ref().map(|snapshot| snapshot.status.as_str()),
            account.as_ref(),
        ))
    }

    async fn logout_weixin(&self) -> Result<web::WebWeixinAccount> {
        self.store.clear_all()?;
        *self.login.lock().await = None;
        Ok(web::WebWeixinAccount::from_account(self.enabled, None))
    }
}

async fn build_test_router_with_weixin_account_state(account: WeixinAccountState) -> Router {
    let dir = tempdir().expect("tempdir");
    let store = WeixinAccountStore::new(dir.path()).expect("weixin store");
    store.save_account(&account).expect("save account");
    web::build_router(AppState::new(
        Arc::new(WeixinAccountChatService::new(true, dir, store)),
        None,
    ))
}

fn sample_weixin_account() -> WeixinAccountState {
    WeixinAccountState {
        bot_token: "bot-token".to_string(),
        ilink_bot_id: "bot@im.bot".to_string(),
        baseurl: "https://ilinkai.weixin.qq.com".to_string(),
        ilink_user_id: Some("user@im.wechat".to_string()),
        get_updates_buf: "cursor-1".to_string(),
        longpolling_timeout_ms: 35_000,
        status: "active".to_string(),
        updated_at: Utc::now(),
    }
}

#[derive(Clone)]
struct MockProvider {
    model: String,
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn default_model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.responses
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no more responses"))
    }
}

fn mock_provider(responses: Vec<LlmResponse>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider {
        model: "mock-model".to_string(),
        responses: Arc::new(Mutex::new(responses.into())),
    })
}

async fn spawn_test_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

async fn json_response(app: Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app.oneshot(request).await.expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response bytes");
    let payload = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&bytes).expect("json payload")
    };
    (status, payload)
}

async fn agent_app(dir: &TempDir, responses: Vec<LlmResponse>) -> Router {
    let provider = mock_provider(responses);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    web::build_router(AppState::new(Arc::new(AgentChatService::new(agent)), None))
}

async fn agent_app_with_config(
    dir: &TempDir,
    mut config: Config,
    responses: Vec<LlmResponse>,
) -> Router {
    let provider = mock_provider(responses);
    let bus = MessageBus::new(32);
    config.agents.defaults.workspace = dir.path().display().to_string();
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");
    web::build_router(AppState::new(Arc::new(AgentChatService::new(agent)), None))
}

async fn agent_app_with_profiles(
    dir: &TempDir,
    responses: Vec<LlmResponse>,
    extra_profiles: &[(&str, &str, &str)],
) -> Router {
    let provider = mock_provider(responses);
    let bus = MessageBus::new(32);
    let mut config = Config::default();
    config.agents.defaults.workspace = dir.path().display().to_string();
    config.agents.defaults.default_profile = "openai:mock-model".to_string();
    config.agents.defaults.provider = "openai".to_string();
    config.agents.defaults.model = "mock-model".to_string();
    config.agents.profiles.insert(
        "openai:mock-model".to_string(),
        AgentProfileConfig {
            provider: "openai".to_string(),
            model: "mock-model".to_string(),
            request: Map::new(),
        },
    );
    for (key, provider_name, model_name) in extra_profiles {
        config.agents.profiles.insert(
            (*key).to_string(),
            AgentProfileConfig {
                provider: (*provider_name).to_string(),
                model: (*model_name).to_string(),
                request: Map::new(),
            },
        );
    }
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");
    web::build_router(AppState::new(Arc::new(AgentChatService::new(agent)), None))
}

#[derive(Clone, Default)]
struct WeixinApiState {
    requests: Arc<Mutex<Vec<String>>>,
}

async fn weixin_get_bot_qrcode(
    axum::extract::State(state): axum::extract::State<WeixinApiState>,
) -> axum::Json<serde_json::Value> {
    state
        .requests
        .lock()
        .await
        .push("/ilink/bot/get_bot_qrcode".to_string());
    axum::Json(json!({
        "data": {
            "qrcode": "qr-token",
            "qrcode_img_content": "data:image/png;base64,abc"
        }
    }))
}

async fn weixin_get_bot_qrcode_page_url(
    axum::extract::State(state): axum::extract::State<WeixinApiState>,
) -> axum::Json<serde_json::Value> {
    state
        .requests
        .lock()
        .await
        .push("/ilink/bot/get_bot_qrcode".to_string());
    axum::Json(json!({
        "data": {
            "qrcode": "qr-token",
            "qrcode_img_content": "https://liteapp.weixin.qq.com/q/7GiQu1?qrcode=9318e0bbe626487f169d4cd996b2640a&bot_type=3"
        }
    }))
}

async fn spawn_weixin_api_server() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let state = WeixinApiState::default();
    let requests = state.requests.clone();
    let app = Router::new()
        .route(
            "/ilink/bot/get_bot_qrcode",
            axum::routing::get(weixin_get_bot_qrcode),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, requests)
}

async fn spawn_weixin_page_qr_api_server() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let state = WeixinApiState::default();
    let requests = state.requests.clone();
    let app = Router::new()
        .route(
            "/ilink/bot/get_bot_qrcode",
            axum::routing::get(weixin_get_bot_qrcode_page_url),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, requests)
}

fn save_session(workspace: &Path, session: &Session) {
    SessionStore::new(workspace)
        .expect("session store")
        .save(session)
        .expect("save session");
}

fn text_message(role: &str, content: &str) -> SessionMessage {
    SessionMessage {
        role: role.to_string(),
        content: json!(content),
        timestamp: Some(Utc::now()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Map::new(),
    }
}

#[tokio::test]
async fn root_and_health_routes_respond() {
    let app = web::build_router(test_state());
    let addr = spawn_test_server(app).await;

    let html = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("fetch root")
        .text()
        .await
        .expect("root body");
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("fetch health")
        .text()
        .await
        .expect("health body");

    assert!(html.to_ascii_lowercase().contains("<!doctype html>"));
    assert_eq!(health, "ok");
}

#[tokio::test]
async fn mcp_servers_endpoint_returns_tool_enabled_state() {
    let app = web::build_router(test_state_with_mcp_tools());
    let (status, response) = json_response(
        app,
        Request::builder()
            .method("GET")
            .uri("/api/mcp/servers")
            .body(Body::empty())
            .expect("mcp servers request"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["servers"][0]["name"], "demo");
    assert_eq!(response["servers"][0]["tool_count"], 2);
    assert_eq!(
        response["servers"][0]["tools"][0]["name"],
        "mcp_demo_search"
    );
    assert_eq!(response["servers"][0]["tools"][0]["enabled"], true);
    assert_eq!(response["servers"][0]["tools"][1]["name"], "mcp_demo_fetch");
    assert_eq!(response["servers"][0]["tools"][1]["enabled"], false);
}

#[tokio::test]
async fn mcp_toggle_endpoint_updates_tool_state_and_rejects_unknown_tools() {
    let app = web::build_router(test_state_with_mcp_tools());
    let (status, _) = json_response(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/mcp/tools/mcp_demo_fetch/toggle")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":true}"#))
            .expect("toggle mcp tool request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, servers) = json_response(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/mcp/servers")
            .body(Body::empty())
            .expect("mcp servers request"),
    )
    .await;
    assert_eq!(servers["servers"][0]["tools"][1]["enabled"], true);

    let (status, payload) = json_response(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/mcp/tools/mcp_demo_missing/toggle")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":false}"#))
            .expect("missing toggle request"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not found")
    );
}

#[tokio::test]
async fn mcp_server_bulk_action_endpoint_updates_tool_state() {
    let app = web::build_router(test_state_with_mcp_tools());

    let (status, _) = json_response(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/mcp/servers/demo/tools/bulk")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"disableAll"}"#))
            .expect("disable all request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, after_disable) = json_response(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/mcp/servers")
            .body(Body::empty())
            .expect("servers request"),
    )
    .await;
    assert_eq!(after_disable["servers"][0]["tools"][0]["enabled"], false);
    assert_eq!(after_disable["servers"][0]["tools"][1]["enabled"], false);

    let (status, _) = json_response(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/mcp/servers/demo/tools/bulk")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"reset"}"#))
            .expect("reset request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, after_reset) = json_response(
        app.clone(),
        Request::builder()
            .method("GET")
            .uri("/api/mcp/servers")
            .body(Body::empty())
            .expect("servers request"),
    )
    .await;
    assert_eq!(after_reset["servers"][0]["tools"][0]["enabled"], true);
    assert_eq!(after_reset["servers"][0]["tools"][1]["enabled"], true);

    let (status, payload) = json_response(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/mcp/servers/missing/tools/bulk")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"enableAll"}"#))
            .expect("missing server request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not found")
    );
}

#[tokio::test]
async fn chat_endpoint_returns_agent_reply() {
    let app = web::build_router(test_state_with_reply("**hello** from agent"));
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-1"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "**hello** from agent");
    assert!(
        response["replyHtml"]
            .as_str()
            .unwrap_or_default()
            .contains("<strong>hello</strong>")
    );
    assert_eq!(response["channel"], "web");
    assert_eq!(response["sessionId"], "browser-session-1");
    assert_eq!(response["persisted"], true);
}

#[tokio::test]
async fn chat_endpoint_exposes_ephemeral_reply_flag() {
    let app = web::build_router(test_state_with_ephemeral_reply("temporary"));
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "/btw hi",
            "sessionId": "browser-session-btw"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "temporary");
    assert_eq!(response["persisted"], false);
}

#[tokio::test]
async fn chat_endpoint_rejects_blank_messages() {
    let app = web::build_router(test_state_with_reply("should not be used"));
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "   ",
            "sessionId": "browser-session-2"
        }))
        .send()
        .await
        .expect("send blank chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("blank chat response");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("message must not be empty")
    );
}

#[tokio::test]
async fn chat_endpoint_returns_internal_error_for_web_session_service_failures() {
    let app = web::build_router(test_state_with_error());
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-error"
        }))
        .send()
        .await
        .expect("send chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("chat error payload");

    assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("provider exploded")
    );
}

#[tokio::test]
async fn chat_endpoint_returns_message_tool_reply() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(
        &dir,
        vec![
            LlmResponse {
                content: Some("sending".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "message".to_string(),
                    arguments: json!({
                        "content": "Hi from the message tool"
                    }),
                }],
                finish_reason: "tool_calls".to_string(),
                extra: Map::new(),
            },
            LlmResponse {
                content: Some("done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            },
        ],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-message"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "Hi from the message tool");
    assert_eq!(response["replyHtml"], "<p>Hi from the message tool</p>\n");
    assert_eq!(response["persisted"], false);
}

#[tokio::test]
async fn sessions_endpoint_returns_channel_grouped_results_with_stable_order_and_capabilities() {
    let dir = tempdir().expect("tempdir");
    let mut recent = Session::new("web:recent");
    recent.active_profile = Some("openrouter:deepseek-r1".to_string());
    recent.messages = vec![
        text_message("user", "hi"),
        text_message("assistant", "Most recent assistant reply"),
    ];
    recent.created_at = Utc::now() - Duration::minutes(10);
    recent.updated_at = Utc::now() - Duration::minutes(1);
    save_session(dir.path(), &recent);

    let mut telegram = Session::new("telegram:chat-1");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "Telegram transcript")];
    telegram.created_at = Utc::now() - Duration::minutes(9);
    telegram.updated_at = Utc::now() - Duration::minutes(2);
    save_session(dir.path(), &telegram);

    let mut wecom = Session::new("wecom:room-2");
    wecom.active_profile = Some("openai:gpt-4.1-mini".to_string());
    wecom.messages = vec![text_message("assistant", "WeCom transcript")];
    wecom.created_at = Utc::now() - Duration::minutes(8);
    wecom.updated_at = Utc::now() - Duration::minutes(3);
    save_session(dir.path(), &wecom);

    let mut weixin = Session::new("weixin:user@im.wechat");
    weixin.active_profile = Some("openai:gpt-4.1-mini".to_string());
    weixin.messages = vec![text_message("assistant", "Weixin transcript")];
    weixin.created_at = Utc::now() - Duration::minutes(7);
    weixin.updated_at = Utc::now() - Duration::minutes(3);
    save_session(dir.path(), &weixin);

    let mut cli = Session::new("cli:terminal-3");
    cli.active_profile = Some("openai:gpt-4.1-mini".to_string());
    cli.messages = vec![text_message("assistant", "CLI transcript")];
    cli.created_at = Utc::now() - Duration::minutes(6);
    cli.updated_at = Utc::now() - Duration::minutes(4);
    save_session(dir.path(), &cli);

    let mut system = Session::new("system:job-4");
    system.active_profile = Some("openai:gpt-4.1-mini".to_string());
    system.messages = vec![text_message("assistant", "System transcript")];
    system.created_at = Utc::now() - Duration::minutes(5);
    system.updated_at = Utc::now() - Duration::minutes(5);
    save_session(dir.path(), &system);

    let mut alpha = Session::new("alpha:item-5");
    alpha.active_profile = Some("openai:gpt-4.1-mini".to_string());
    alpha.messages = vec![text_message("assistant", "Alpha transcript")];
    alpha.created_at = Utc::now() - Duration::minutes(4);
    alpha.updated_at = Utc::now() - Duration::minutes(6);
    save_session(dir.path(), &alpha);

    let mut zeta = Session::new("zeta:item-6");
    zeta.active_profile = Some("openai:gpt-4.1-mini".to_string());
    zeta.messages = vec![text_message("assistant", "Zeta transcript")];
    zeta.created_at = Utc::now() - Duration::minutes(3);
    zeta.updated_at = Utc::now() - Duration::minutes(7);
    save_session(dir.path(), &zeta);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::get(format!("http://{addr}/api/sessions"))
        .await
        .expect("fetch sessions")
        .json()
        .await
        .expect("sessions payload");

    let groups = response["groups"].as_array().expect("groups array");
    let channels = groups
        .iter()
        .map(|group| {
            group["channel"]
                .as_str()
                .expect("group channel")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        channels,
        vec![
            "web", "telegram", "wecom", "weixin", "cli", "system", "alpha", "zeta"
        ]
    );

    let web_sessions = groups[0]["sessions"].as_array().expect("web sessions");
    assert_eq!(web_sessions.len(), 1);
    assert_eq!(web_sessions[0]["sessionId"], "recent");
    assert_eq!(web_sessions[0]["channel"], "web");
    assert_eq!(web_sessions[0]["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(web_sessions[0]["preview"], "Most recent assistant reply");
    assert_eq!(web_sessions[0]["readOnly"], false);
    assert_eq!(web_sessions[0]["canSend"], true);
    assert_eq!(web_sessions[0]["canDuplicate"], false);

    let telegram_sessions = groups[1]["sessions"].as_array().expect("telegram sessions");
    assert_eq!(telegram_sessions.len(), 1);
    assert_eq!(telegram_sessions[0]["sessionId"], "chat-1");
    assert_eq!(telegram_sessions[0]["channel"], "telegram");
    assert_eq!(telegram_sessions[0]["readOnly"], true);
    assert_eq!(telegram_sessions[0]["canSend"], false);
    assert_eq!(telegram_sessions[0]["canDuplicate"], true);

    let weixin_sessions = groups[3]["sessions"].as_array().expect("weixin sessions");
    assert_eq!(weixin_sessions.len(), 1);
    assert_eq!(weixin_sessions[0]["sessionId"], "user@im.wechat");
    assert_eq!(weixin_sessions[0]["channel"], "weixin");
    assert_eq!(weixin_sessions[0]["readOnly"], true);
    assert_eq!(weixin_sessions[0]["canSend"], false);
    assert_eq!(weixin_sessions[0]["canDuplicate"], true);
}

#[tokio::test]
async fn session_detail_endpoint_returns_channel_capabilities_and_source_session_key() {
    let dir = tempdir().expect("tempdir");
    let mut session = Session::new("telegram:focus");
    session.active_profile = Some("openrouter:deepseek-r1".to_string());
    session.source_session_key = Some("wecom:origin-room".to_string());
    session.messages = vec![
        text_message("user", "hello"),
        text_message("assistant", "**hi** back"),
    ];
    save_session(dir.path(), &session);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/sessions/telegram/focus"))
            .await
            .expect("fetch session detail")
            .json()
            .await
            .expect("detail payload");

    assert_eq!(response["sessionId"], "focus");
    assert_eq!(response["channel"], "telegram");
    assert_eq!(response["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(response["readOnly"], true);
    assert_eq!(response["canSend"], false);
    assert_eq!(response["canDuplicate"], true);
    assert_eq!(response["sourceSessionKey"], "wecom:origin-room");
    assert!(response["updatedAt"].as_str().is_some());
    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
    assert!(messages[0]["timestamp"].as_str().is_some());
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "**hi** back");
    assert!(
        messages[1]["contentHtml"]
            .as_str()
            .unwrap_or_default()
            .contains("<strong>hi</strong>")
    );
}

#[tokio::test]
async fn session_detail_endpoint_groups_btw_messages_into_a_thread_block() {
    let dir = tempdir().expect("tempdir");
    let mut session = Session::new("web:btw-focus");
    let mut query_extra = Map::new();
    query_extra.insert("_exclude_from_context".to_string(), json!(true));
    query_extra.insert("_timeline_kind".to_string(), json!("btw_query"));
    query_extra.insert("_btw_id".to_string(), json!("btw-1"));

    let mut answer_extra = Map::new();
    answer_extra.insert("_exclude_from_context".to_string(), json!(true));
    answer_extra.insert("_timeline_kind".to_string(), json!("btw_answer"));
    answer_extra.insert("_btw_id".to_string(), json!("btw-1"));

    session.messages = vec![
        text_message("user", "main task"),
        SessionMessage {
            role: "user".to_string(),
            content: json!("side question"),
            timestamp: Some(Utc::now() - Duration::seconds(1)),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            extra: query_extra,
        },
        SessionMessage {
            role: "assistant".to_string(),
            content: json!("side answer"),
            timestamp: Some(Utc::now()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            extra: answer_extra,
        },
    ];
    save_session(dir.path(), &session);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/sessions/web/btw-focus"))
            .await
            .expect("fetch session detail")
            .json()
            .await
            .expect("detail payload");

    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["kind"], "message");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["kind"], "btw_thread");
    assert_eq!(messages[1]["role"], "btw");
    assert_eq!(messages[1]["query"], "side question");
    assert_eq!(messages[1]["content"], "side answer");
    assert_eq!(messages[1]["pending"], false);
    assert_eq!(messages[1]["stale"], false);
}

#[tokio::test]
async fn create_session_endpoint_initializes_default_profile() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions"))
        .send()
        .await
        .expect("create session")
        .json()
        .await
        .expect("create payload");

    assert!(response["sessionId"].as_str().is_some());
    assert_eq!(response["channel"], "web");
    assert_eq!(response["activeProfile"], "openai:mock-model");
    assert_eq!(response["readOnly"], false);
    assert_eq!(response["canSend"], true);
    assert_eq!(response["canDuplicate"], false);
    assert!(response.get("messages").is_none());
}

#[tokio::test]
async fn chat_endpoint_includes_active_profile() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(
        &dir,
        vec![LlmResponse {
            content: Some("hello from model".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        }],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hello",
            "sessionId": "browser-session-profile"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "hello from model");
    assert_eq!(response["channel"], "web");
    assert_eq!(response["activeProfile"], "openai:mock-model");
}

#[tokio::test]
async fn weixin_account_endpoints_report_login_status() {
    let app = build_test_router_with_weixin_account_state(sample_weixin_account()).await;
    let addr = spawn_test_server(app).await;

    let account: serde_json::Value = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account")
        .json()
        .await
        .expect("weixin account payload");
    assert_eq!(account["enabled"], true);
    assert_eq!(account["loggedIn"], true);
    assert_eq!(account["expired"], false);
    assert_eq!(account["botId"], "bot@im.bot");
    assert_eq!(account["userId"], "user@im.wechat");

    let login_start: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/login/start"))
        .send()
        .await
        .expect("start weixin login")
        .json()
        .await
        .expect("login start payload");
    assert_eq!(login_start["qrcode"], "qr-token");
    assert_eq!(login_start["qrcodeImgContent"], "data:image/png;base64,abc");

    let login_status: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/weixin/login/status"))
            .await
            .expect("poll weixin login")
            .json()
            .await
            .expect("login status payload");
    assert_eq!(login_status["status"], "wait");
    assert_eq!(login_status["loggedIn"], true);
    assert_eq!(login_status["expired"], false);

    let logout: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/logout"))
        .send()
        .await
        .expect("logout weixin")
        .json()
        .await
        .expect("logout payload");
    assert_eq!(logout["loggedIn"], false);
    assert_eq!(logout["expired"], false);

    let after_logout: serde_json::Value = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account after logout")
        .json()
        .await
        .expect("weixin account after logout payload");
    assert_eq!(after_logout["loggedIn"], false);
    assert_eq!(after_logout["expired"], false);
}

#[tokio::test]
async fn real_agentchatservice_respects_disabled_weixin_config() {
    let dir = tempdir().expect("tempdir");
    let mut config = Config::default();
    config.channels.weixin = WeixinConfig {
        enabled: false,
        api_base: "https://custom-weixin.example.com".to_string(),
        cdn_base: "https://novac2c.cdn.weixin.qq.com/c2c".to_string(),
    };
    let app = agent_app_with_config(&dir, config, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let account: serde_json::Value = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account")
        .json()
        .await
        .expect("weixin account payload");

    assert_eq!(account["enabled"], false);
    assert_eq!(account["loggedIn"], false);
    assert_eq!(account["expired"], false);
}

#[tokio::test]
async fn real_agentchatservice_uses_configured_weixin_api_base() {
    let dir = tempdir().expect("tempdir");
    let (api_addr, requests) = spawn_weixin_api_server().await;
    let mut config = Config::default();
    config.channels.weixin = WeixinConfig {
        enabled: true,
        api_base: format!("http://{api_addr}"),
        cdn_base: "https://novac2c.cdn.weixin.qq.com/c2c".to_string(),
    };
    let app = agent_app_with_config(&dir, config, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/login/start"))
        .send()
        .await
        .expect("start weixin login")
        .json()
        .await
        .expect("login start payload");

    assert_eq!(response["qrcode"], "qr-token");
    assert_eq!(response["qrcodeImgContent"], "data:image/png;base64,abc");
    assert_eq!(requests.lock().await.len(), 1);
    assert_eq!(requests.lock().await[0], "/ilink/bot/get_bot_qrcode");
}

#[tokio::test]
async fn real_agentchatservice_converts_weixin_qr_page_urls_to_renderable_images() {
    let dir = tempdir().expect("tempdir");
    let (api_addr, requests) = spawn_weixin_page_qr_api_server().await;
    let mut config = Config::default();
    config.channels.weixin = WeixinConfig {
        enabled: true,
        api_base: format!("http://{api_addr}"),
        cdn_base: "https://novac2c.cdn.weixin.qq.com/c2c".to_string(),
    };
    let app = agent_app_with_config(&dir, config, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/login/start"))
        .send()
        .await
        .expect("start weixin login")
        .json()
        .await
        .expect("login start payload");

    assert_eq!(response["qrcode"], "qr-token");
    assert!(
        response["qrcodeImgContent"]
            .as_str()
            .unwrap_or_default()
            .starts_with("data:image/svg+xml;base64,")
    );
    assert_eq!(requests.lock().await.len(), 1);
    assert_eq!(requests.lock().await[0], "/ilink/bot/get_bot_qrcode");
}

#[tokio::test]
async fn real_agentchatservice_rejects_weixin_status_before_login_start() {
    let dir = tempdir().expect("tempdir");
    let mut config = Config::default();
    config.channels.weixin = WeixinConfig {
        enabled: true,
        api_base: "https://custom-weixin.example.com".to_string(),
        cdn_base: "https://novac2c.cdn.weixin.qq.com/c2c".to_string(),
    };
    let app = agent_app_with_config(&dir, config, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::get(format!("http://{addr}/api/weixin/login/status"))
        .await
        .expect("poll weixin login status");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("login status payload");

    assert_eq!(status, reqwest::StatusCode::CONFLICT);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("weixin login has not been started")
    );
}

#[tokio::test]
async fn real_agentchatservice_surfaces_weixin_runtime_init_failures_as_internal_errors() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("channels"), "not a directory")
        .expect("block weixin channel directory");

    let mut config = Config::default();
    config.channels.weixin = WeixinConfig {
        enabled: true,
        api_base: "https://custom-weixin.example.com".to_string(),
        cdn_base: "https://novac2c.cdn.weixin.qq.com/c2c".to_string(),
    };
    let app = agent_app_with_config(&dir, config, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let account = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account after init failure");
    let account_status = account.status();
    let account_payload: serde_json::Value = account.json().await.expect("account payload");

    assert_eq!(account_status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        account_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to initialize weixin web runtime")
    );

    let login_start = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/login/start"))
        .send()
        .await
        .expect("start weixin login after init failure");
    let login_start_status = login_start.status();
    let login_start_payload: serde_json::Value =
        login_start.json().await.expect("login start payload");

    assert_eq!(
        login_start_status,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(
        login_start_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to initialize weixin web runtime")
    );

    let login_status = reqwest::get(format!("http://{addr}/api/weixin/login/status"))
        .await
        .expect("poll weixin login status after init failure");
    let login_status_code = login_status.status();
    let login_status_payload: serde_json::Value =
        login_status.json().await.expect("login status payload");

    assert_eq!(
        login_status_code,
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(
        login_status_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to initialize weixin web runtime")
    );

    let logout = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/logout"))
        .send()
        .await
        .expect("logout weixin after init failure");
    let logout_status = logout.status();
    let logout_payload: serde_json::Value = logout.json().await.expect("logout payload");

    assert_eq!(logout_status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        logout_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("failed to initialize weixin web runtime")
    );
}

#[tokio::test]
async fn chat_endpoint_rejects_non_web_sessions_until_duplicated() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "not web")];
    save_session(dir.path(), &telegram);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hello",
            "channel": "telegram",
            "sessionId": "outside"
        }))
        .send()
        .await
        .expect("send non-web chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("non-web payload");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("duplicate")
    );
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("read-only")
    );
}

#[tokio::test]
async fn duplicate_session_endpoint_returns_new_web_detail_with_copied_history() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openrouter:deepseek-r1".to_string());
    telegram.messages = vec![
        text_message("user", "hello from telegram"),
        text_message("assistant", "reply from telegram"),
    ];
    save_session(dir.path(), &telegram);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "telegram",
            "sessionId": "outside"
        }))
        .send()
        .await
        .expect("duplicate session")
        .json()
        .await
        .expect("duplicate payload");

    assert_eq!(response["channel"], "web");
    assert!(response["sessionId"].as_str().is_some());
    assert_eq!(response["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(response["readOnly"], false);
    assert_eq!(response["canSend"], true);
    assert_eq!(response["canDuplicate"], false);
    assert_eq!(response["sourceSessionKey"], "telegram:outside");
    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"], "hello from telegram");
    assert_eq!(messages[1]["content"], "reply from telegram");
}

#[tokio::test]
async fn duplicate_session_endpoint_supports_weixin_sources() {
    let dir = tempdir().expect("tempdir");
    let mut weixin = Session::new("weixin:user@im.wechat");
    weixin.active_profile = Some("openai:gpt-4.1-mini".to_string());
    weixin.messages = vec![
        text_message("user", "hello from weixin"),
        text_message("assistant", "reply from weixin"),
    ];
    save_session(dir.path(), &weixin);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "weixin",
            "sessionId": "user@im.wechat"
        }))
        .send()
        .await
        .expect("duplicate weixin session")
        .json()
        .await
        .expect("duplicate payload");

    assert_eq!(response["channel"], "web");
    assert_eq!(response["sourceSessionKey"], "weixin:user@im.wechat");
    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"], "hello from weixin");
    assert_eq!(messages[1]["content"], "reply from weixin");
}

#[tokio::test]
async fn nested_session_ids_are_browsable_and_duplicable() {
    let dir = tempdir().expect("tempdir");
    let mut system = Session::new("system:wecom:chat-42");
    system.active_profile = Some("openai:gpt-4.1-mini".to_string());
    system.messages = vec![
        text_message("user", "nested hello"),
        text_message("assistant", "nested reply"),
    ];
    save_session(dir.path(), &system);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let detail: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/sessions/system/wecom:chat-42"))
            .await
            .expect("fetch nested detail")
            .json()
            .await
            .expect("nested detail payload");

    assert_eq!(detail["channel"], "system");
    assert_eq!(detail["sessionId"], "wecom:chat-42");
    assert_eq!(detail["messages"][1]["content"], "nested reply");

    let duplicated: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "system",
            "sessionId": "wecom:chat-42"
        }))
        .send()
        .await
        .expect("duplicate nested session")
        .json()
        .await
        .expect("duplicate nested payload");

    assert_eq!(duplicated["channel"], "web");
    assert_eq!(duplicated["sourceSessionKey"], "system:wecom:chat-42");
}

#[tokio::test]
async fn duplicate_session_endpoint_returns_not_found_for_missing_source() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "telegram",
            "sessionId": "missing"
        }))
        .send()
        .await
        .expect("duplicate missing session");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("missing duplicate payload");

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("not found")
    );
}

#[tokio::test]
async fn duplicate_session_endpoint_rejects_already_writable_web_sessions() {
    let dir = tempdir().expect("tempdir");
    let mut web_session = Session::new("web:alpha");
    web_session.active_profile = Some("openai:gpt-4.1-mini".to_string());
    web_session.messages = vec![text_message("assistant", "already web")];
    save_session(dir.path(), &web_session);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "web",
            "sessionId": "alpha"
        }))
        .send()
        .await
        .expect("duplicate web session");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("duplicate web payload");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("already writable")
    );
}

#[tokio::test]
async fn session_endpoints_reject_invalid_or_missing_ids() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "not web")];
    save_session(dir.path(), &telegram);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let invalid = reqwest::get(format!("http://{addr}/api/sessions/telegram/bad$id"))
        .await
        .expect("fetch invalid id");
    let invalid_status = invalid.status();
    let invalid_payload: serde_json::Value = invalid.json().await.expect("invalid payload");
    assert_eq!(invalid_status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        invalid_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid session id")
    );

    let missing = reqwest::get(format!("http://{addr}/api/sessions/telegram/missing"))
        .await
        .expect("fetch missing id");
    let missing_status = missing.status();
    let missing_payload: serde_json::Value = missing.json().await.expect("missing payload");
    assert_eq!(missing_status, reqwest::StatusCode::NOT_FOUND);
    assert!(
        missing_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("session not found")
    );
}
