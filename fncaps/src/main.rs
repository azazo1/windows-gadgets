#[cfg(target_os = "windows")]
mod app;
mod version;

use clap::Parser;

/// 常驻的 CapsLock 组合键守护进程, 运行期不接受任何参数.
#[derive(Debug, Parser)]
#[command(
    name = "fncaps",
    version = version::display(),
    about = "把 CapsLock 扩展成一组高频快捷键"
)]
struct Cli {}

#[cfg(target_os = "windows")]
fn main() {
    Cli::parse();
    app::run();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    Cli::parse();
    eprintln!("fncaps only supports Windows.");
}
