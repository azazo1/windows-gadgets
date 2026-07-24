mod access;
mod cli;
mod config;
mod forward;
mod logging;
mod network_adapter;
mod target;

use anyhow::{Context, Result, bail};
use clap::Parser;

use self::cli::{Cli, Command};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.print_default_config {
        if cli.command.is_some() {
            bail!("--print-default-config 不能与子命令同时使用");
        }
        print!("{}", config::Settings::default_source());
        return Ok(());
    }

    logging::init();

    match cli.command.context("需要指定 run 或 check 子命令")? {
        Command::Run(arguments) => {
            let settings = config::Settings::load(arguments.config)?;
            forward::run(settings).await
        }
        Command::Check(arguments) => {
            let settings = config::Settings::load(arguments.config)?;
            forward::check(settings).await
        }
    }
}
