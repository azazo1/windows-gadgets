mod access;
mod cli;
mod config;
mod forward;
mod logging;
mod network_adapter;
mod reverse;
mod target;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::info;

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
            run_daemon(settings).await
        }
        Command::Check(arguments) => {
            let settings = config::Settings::load(arguments.config)?;
            check_rules(settings).await
        }
    }
}

async fn run_daemon(settings: config::Settings) -> Result<()> {
    let (shutdown_sender, _) = watch::channel(false);
    let mut tasks = JoinSet::new();
    let prepared_forwards = forward::prepare(&settings).await?;
    let prepared_reverse = reverse::prepare(&settings).await?;
    let forward_rules = forward::launch(
        prepared_forwards,
        &settings,
        shutdown_sender.subscribe(),
        &mut tasks,
    );
    let reverse_rules = reverse::launch(
        prepared_reverse,
        &settings,
        shutdown_sender.subscribe(),
        &mut tasks,
    );

    info!(forward_rules, reverse_rules, "wslproxy 已开始运行");
    let early_error = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("已收到退出信号");
            None
        }
        task = tasks.join_next() => task_result_to_error(task, false),
    };

    let _ = shutdown_sender.send(true);
    let mut shutdown_error = early_error;
    while let Some(task) = tasks.join_next().await {
        if shutdown_error.is_none() {
            shutdown_error = task_result_to_error(Some(task), true);
        }
    }

    if let Some(error) = shutdown_error {
        return Err(error);
    }

    info!("wslproxy 已停止");
    Ok(())
}

async fn check_rules(settings: config::Settings) -> Result<()> {
    let forward_rules = forward::check(&settings).await?;
    let reverse_rules = reverse::check(&settings).await?;
    info!(forward_rules, reverse_rules, "全部规则检查通过");
    Ok(())
}

fn task_result_to_error(
    task: Option<Result<Result<()>, tokio::task::JoinError>>,
    shutting_down: bool,
) -> Option<anyhow::Error> {
    match task {
        Some(Ok(Ok(()))) if shutting_down => None,
        Some(Ok(Ok(()))) => Some(anyhow!("转发运行任务意外结束")),
        Some(Ok(Err(error))) => Some(error),
        Some(Err(error)) => Some(anyhow!("转发运行任务异常退出: {error}")),
        None => Some(anyhow!("没有可运行的转发任务")),
    }
}
