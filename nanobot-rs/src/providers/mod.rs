mod base;
mod openai_compatible;
mod registry;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::config::Config;

pub use base::{
    LlmProvider, LlmResponse, ProviderError, ProviderRequestDescriptor, ToolCall, should_retry,
};
pub use openai_compatible::OpenAICompatibleProvider;
pub use registry::{ProviderKind, ProviderRegistry, ProviderSpec, ResolvedProviderConfig};

pub type OpenAIProvider = OpenAICompatibleProvider;

#[derive(Clone)]
pub struct ProviderPool {
    config: Config,
    registry: ProviderRegistry,
    clients: Arc<Mutex<HashMap<String, Arc<OpenAICompatibleProvider>>>>,
    default_model: String,
}

impl ProviderPool {
    pub fn new(config: Config) -> Self {
        let default_model = config.agents.defaults.model.clone();
        Self {
            config,
            registry: ProviderRegistry,
            clients: Arc::new(Mutex::new(HashMap::new())),
            default_model,
        }
    }

    async fn client_for(
        &self,
        request: &ProviderRequestDescriptor,
    ) -> Result<Arc<OpenAICompatibleProvider>> {
        let resolved = self.registry.build_config_for_provider(
            &self.config,
            &request.provider_name,
            &request.model_name,
        )?;
        let key = provider_cache_key(&resolved);

        if let Some(existing) = self.clients.lock().await.get(&key).cloned() {
            return Ok(existing);
        }

        let client = Arc::new(OpenAICompatibleProvider::from_config(resolved)?);
        let mut clients = self.clients.lock().await;
        Ok(clients.entry(key).or_insert_with(|| client.clone()).clone())
    }
}

#[async_trait]
impl LlmProvider for ProviderPool {
    fn default_model(&self) -> &str {
        &self.default_model
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        model: &str,
    ) -> Result<LlmResponse> {
        let request = ProviderRequestDescriptor::new("openai", model, serde_json::Map::new());
        self.chat_with_request(messages, tools, &request).await
    }

    async fn chat_with_request(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        let client = self.client_for(request).await?;
        client.chat_with_request(messages, tools, request).await
    }
}

fn provider_cache_key(config: &ResolvedProviderConfig) -> String {
    let mut headers = config.extra_headers.iter().collect::<Vec<_>>();
    headers.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));
    let header_blob = headers
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "{}\n{}\n{}\n{}",
        config.name, config.api_base, config.api_key, header_blob
    )
}

pub fn build_provider_from_config(config: &Config) -> Result<Arc<dyn LlmProvider>> {
    Ok(Arc::new(ProviderPool::new(config.clone())))
}
