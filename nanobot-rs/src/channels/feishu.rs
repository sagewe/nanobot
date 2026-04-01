use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail, ensure};
use url::Url;

use crate::bus::{MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::FeishuConfig;

pub struct FeishuChannel {
    config: FeishuConfig,
    #[allow(dead_code)]
    bus: MessageBus,
    running: AtomicBool,
}

impl FeishuChannel {
    pub fn new(config: FeishuConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            running: AtomicBool::new(false),
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
}

#[async_trait::async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn start(&self) -> Result<()> {
        self.validate_startup_config()?;
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        if msg.chat_id.trim().is_empty() {
            bail!("feishu chat_id is required");
        }
        Ok(())
    }
}
