use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use crate::bus::{MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::WecomConfig;

pub struct WecomBotChannel {
    #[allow(dead_code)]
    config: WecomConfig,
    #[allow(dead_code)]
    bus: MessageBus,
    running: AtomicBool,
}

impl WecomBotChannel {
    pub fn new(config: WecomConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            running: AtomicBool::new(false),
        }
    }
}

#[async_trait::async_trait]
impl Channel for WecomBotChannel {
    fn name(&self) -> &'static str {
        "wecom"
    }

    async fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, _msg: OutboundMessage) -> Result<()> {
        Ok(())
    }
}
