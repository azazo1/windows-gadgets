use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "wslproxy", version, about = "将 Windows TCP 端口转发到 WSL 服务")]
pub struct Cli {
    #[arg(long)]
    pub print_default_config: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 运行 TCP 转发守护进程.
    Run(ConfigArguments),
    /// 校验配置和 WSL 目标连通性.
    Check(ConfigArguments),
}

#[derive(Debug, Args)]
pub struct ConfigArguments {
    /// 指定 TOML 配置文件路径.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}
