#[cfg(target_os = "windows")]
mod app {
    use clap::Parser;
    use imeswitch::{Config, Runner};
    use std::time::Duration;
    use tracing::info;
    use tracing_subscriber::EnvFilter;

    #[derive(Debug, Parser)]
    #[command(
        name = "imeswitch",
        version,
        about = "Windows 输入法后台切换守护进程",
        after_help = "默认热键:\n  单独按 Left Alt  -> 切到英文输入法\n  单独按 Right Alt -> 切到中文输入法\n  Esc / Ctrl+[ -> 切到英文输入法\n\n使用示例:\n  imeswitch\n  imeswitch --no-ime-resetting --no-alt-switching\n  imeswitch --no-ensure-chinese-mode\n  imeswitch --locale-en 1033 --locale-zh 2052 --poll-ms 80"
    )]
    struct Args {
        #[arg(
            long,
            default_value_t = false,
            help = "禁用: 窗口焦点变化时重置到英文输入法"
        )]
        no_ime_resetting: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "禁用: Esc / Ctrl+[ 快捷切英文（不影响左右 Alt）"
        )]
        no_escape_switching: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "禁用: 单独按左/右 Alt 切换输入法"
        )]
        no_alt_switching: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "禁用: 中文输入法布局下自动保持中文模式"
        )]
        no_ensure_chinese_mode: bool,

        #[arg(long, default_value_t = 1033, help = "英文输入法 locale（默认 1033）")]
        locale_en: u32,

        #[arg(long, default_value_t = 2052, help = "中文输入法 locale（默认 2052）")]
        locale_zh: u16,

        #[arg(long, default_value_t = 300, help = "主循环轮询间隔（毫秒）")]
        poll_ms: u64,
    }

    impl From<Args> for Config {
        fn from(args: Args) -> Self {
            Self {
                ime_resetting: !args.no_ime_resetting,
                escape_switching: !args.no_escape_switching,
                alt_switching: !args.no_alt_switching,
                locale_en: args.locale_en,
                locale_zh: args.locale_zh,
                ensure_chinese_mode: !args.no_ensure_chinese_mode,
                poll_interval: Duration::from_millis(args.poll_ms.max(10)),
            }
        }
    }

    pub async fn run() {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("imeswitch=info"));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .init();

        let args = Args::parse();
        info!(?args, "starting imeswitch daemon");
        let runner = Runner::new(args.into());
        runner.run().await;
    }
}

#[cfg(target_os = "windows")]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    app::run().await;
}

#[cfg(not(target_os = "windows"))]
fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("error"))
        .with_target(false)
        .try_init();
    tracing::error!("imeswitch only supports Windows.");
}
