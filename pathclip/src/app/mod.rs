mod clipboard;
mod hotkey;
mod instance;
mod logging;
mod runner;
mod settings;
mod transform;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

use clipboard::ClipboardMonitor;
use hotkey::HotkeyMonitor;
use instance::InstanceGuard;
use runner::Runner;
use settings::Settings;

#[derive(Debug)]
pub(super) enum RuntimeEvent {
    ClipboardChanged,
    HotkeyPressed(u32),
}

#[derive(Debug, Parser)]
#[command(
    name = "pathclip",
    version,
    about = "自动转换 Windows 剪贴板路径",
    after_help = "配置文件: ~/.config/pathclip/config.toml\n空 hotkey 表示不注册该 profile 的热键."
)]
struct Args {
    #[arg(long, value_name = "PATH", help = "指定配置文件路径")]
    config: Option<PathBuf>,

    #[arg(long, help = "输出默认配置并退出")]
    print_default_config: bool,
}

pub async fn run() -> Result<()> {
    logging::init();
    let args = Args::parse();

    if args.print_default_config {
        use std::io::Write;
        std::io::stdout()
            .write_all(Settings::default_source().as_bytes())
            .context("failed to write default settings")?;
        return Ok(());
    }

    let settings = Settings::load(args.config)?;
    let _instance = InstanceGuard::acquire()?;
    info!("pathclip daemon started");

    let (sender, receiver) = unbounded_channel();
    let clipboard_monitor = if settings.auto_profile().is_some() {
        Some(ClipboardMonitor::spawn(sender.clone())?)
    } else {
        None
    };
    let hotkey_monitor = HotkeyMonitor::spawn(settings.registered_hotkeys(), sender)?;

    if settings.auto_profile().is_none() && hotkey_monitor.is_none() {
        warn!("automatic conversion and all hotkeys are disabled");
    }

    let mut runner = Runner::new(settings, receiver)?;
    tokio::select! {
        result = runner.run() => result?,
        signal = tokio::signal::ctrl_c() => {
            signal.context("failed to wait for Ctrl+C")?;
            info!("shutdown requested");
        }
    }

    if let Some(monitor) = clipboard_monitor {
        monitor.stop().await;
    }
    if let Some(monitor) = hotkey_monitor {
        monitor.stop().await;
    }

    Ok(())
}
