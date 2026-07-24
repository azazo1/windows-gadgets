use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "wslproxy-agent", version, about = "wslproxy 的 WSL 反向转发 helper")]
pub struct Cli {
    #[arg(long)]
    pub listen_port: u16,

    #[arg(long, default_value_t = 128)]
    pub max_connections: usize,

    #[arg(long, default_value = "reverse")]
    pub rule_name: String,

    #[arg(long)]
    pub check: bool,
}
