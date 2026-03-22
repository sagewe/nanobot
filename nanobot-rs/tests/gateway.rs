use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use nanobot_rs::cli::{App, Commands, GatewayArgs, GatewayRuntime, run_gateway_command};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RuntimeCalls {
    start_channels: usize,
    run_agent: usize,
    stop_agent: usize,
    stop_channels: usize,
    serve_web: Vec<(String, u16)>,
}

#[derive(Clone, Default)]
struct FakeGatewayRuntime {
    calls: Arc<Mutex<RuntimeCalls>>,
}

impl FakeGatewayRuntime {
    fn calls(&self) -> RuntimeCalls {
        self.calls.lock().expect("lock calls").clone()
    }
}

#[async_trait]
impl GatewayRuntime for FakeGatewayRuntime {
    async fn start_channels(&self) -> Result<()> {
        self.calls.lock().expect("lock calls").start_channels += 1;
        Ok(())
    }

    async fn run_agent(&self) {
        self.calls.lock().expect("lock calls").run_agent += 1;
        std::future::pending::<()>().await;
    }

    fn stop_agent(&self) {
        self.calls.lock().expect("lock calls").stop_agent += 1;
    }

    async fn stop_channels(&self) -> Result<()> {
        self.calls.lock().expect("lock calls").stop_channels += 1;
        Ok(())
    }

    async fn serve_web(&self, host: &str, port: u16) -> Result<()> {
        self.calls
            .lock()
            .expect("lock calls")
            .serve_web
            .push((host.to_string(), port));
        std::future::pending::<Result<()>>().await
    }

    async fn wait_for_shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn parse_gateway_args(args: &[&str]) -> GatewayArgs {
    let app = App::try_parse_from(args).expect("parse args");
    match app.command {
        Commands::Gateway(args) => args,
        other => panic!("expected gateway command, got {other:?}"),
    }
}

#[tokio::test]
async fn gateway_startup_wires_web_server_with_default_bind_values() {
    let args = parse_gateway_args(&["nanobot-rs", "gateway"]);
    let runtime = Arc::new(FakeGatewayRuntime::default());

    run_gateway_command(runtime.clone(), args)
        .await
        .expect("run gateway command");

    let calls = runtime.calls();
    assert_eq!(calls.start_channels, 1);
    assert_eq!(calls.run_agent, 1);
    assert_eq!(calls.serve_web, vec![("127.0.0.1".to_string(), 3456)]);
    assert_eq!(calls.stop_agent, 1);
    assert_eq!(calls.stop_channels, 1);
}

#[tokio::test]
async fn gateway_startup_honors_overridden_web_bind_values() {
    let args = parse_gateway_args(&[
        "nanobot-rs",
        "gateway",
        "--web-host",
        "0.0.0.0",
        "--web-port",
        "4123",
    ]);
    let runtime = Arc::new(FakeGatewayRuntime::default());

    run_gateway_command(runtime.clone(), args)
        .await
        .expect("run gateway command");

    let calls = runtime.calls();
    assert_eq!(calls.serve_web, vec![("0.0.0.0".to_string(), 4123)]);
}
