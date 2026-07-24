mod cli;
mod logging;
mod relay;

use std::net::Ipv4Addr;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tokio::net::TcpListener;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::info;
use wslproxy_protocol::{SplitIo, agent_handshake};

use self::cli::Cli;

pub fn init_logging() {
    logging::init();
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.listen_port == 0 {
        bail!("--listen-port 必须大于 0");
    }
    if cli.max_connections == 0 {
        bail!("--max-connections 必须大于 0");
    }

    if cli.check {
        return check(cli.listen_port).await;
    }

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, cli.listen_port))
        .await
        .with_context(|| format!("无法绑定 WSL loopback 监听端口 127.0.0.1:{}", cli.listen_port))?;
    info!(listen = %listener.local_addr()?, "已绑定 WSL 反向转发监听端点");

    let mut pipe = SplitIo::new(tokio::io::stdin(), tokio::io::stdout());
    agent_handshake(&mut pipe, cli.listen_port).await?;
    info!(listen_port = cli.listen_port, "已完成 Windows 端协议握手");

    relay::serve(
        pipe.compat(),
        listener,
        cli.rule_name,
        cli.max_connections,
    )
    .await
}

async fn check(listen_port: u16) -> Result<()> {
    let mut pipe = SplitIo::new(tokio::io::stdin(), tokio::io::stdout());
    agent_handshake(&mut pipe, listen_port).await?;
    info!(listen_port, "反向 agent 检查握手完成");
    Ok(())
}
