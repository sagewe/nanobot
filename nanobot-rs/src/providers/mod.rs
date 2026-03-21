mod base;
mod openai_compatible;
mod registry;

use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;

pub use base::{LlmProvider, LlmResponse, ProviderError, ProviderRequestDescriptor, ToolCall, should_retry};
pub use openai_compatible::OpenAICompatibleProvider;
pub use registry::{ProviderKind, ProviderRegistry, ProviderSpec, ResolvedProviderConfig};

pub type OpenAIProvider = OpenAICompatibleProvider;

pub fn build_provider_from_config(config: &Config) -> Result<Arc<dyn LlmProvider>> {
    ProviderRegistry.build_provider(config)
}
