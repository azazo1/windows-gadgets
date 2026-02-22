#[cfg(target_os = "windows")]
mod app {
    use clap::Parser;
    use std::time::Duration;
    use tracing::info;
    use tracing_subscriber::EnvFilter;
    use windows_gadgets::imeswitch::{Config, Runner};

    #[derive(Debug, Parser)]
    #[command(
        name = "imeswitch",
        version,
        about = "Windows 输入法后台切换守护进程"
    )]
    struct Args {
        #[arg(long, default_value_t = true)]
        ime_resetting: bool,

        #[arg(long, default_value_t = true)]
        escape_switching: bool,

        #[arg(long, default_value_t = true)]
        ensure_chinese_mode: bool,

        #[arg(long, default_value_t = 1033)]
        locale_en: u32,

        #[arg(long, default_value_t = 2052)]
        locale_zh: u16,

        #[arg(long, default_value_t = 100)]
        poll_ms: u64,

        #[arg(long, default_value_t = 5)]
        tick_secs: u64,

        #[arg(long, default_value_t = false)]
        verbose: bool,
    }

    impl From<Args> for Config {
        fn from(args: Args) -> Self {
            Self {
                ime_resetting: args.ime_resetting,
                escape_switching: args.escape_switching,
                locale_en: args.locale_en,
                locale_zh: args.locale_zh,
                ensure_chinese_mode: args.ensure_chinese_mode,
                poll_interval: Duration::from_millis(args.poll_ms.max(10)),
                tick_interval: Duration::from_secs(args.tick_secs.max(1)),
                verbose: args.verbose,
            }
        }
    }

    pub fn run() {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("imeswitch=info,windows_gadgets=info")
        });
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .init();

        let args = Args::parse();
        info!(?args, "starting imeswitch daemon");
        let mut runner = Runner::new(args.into());
        runner.run();
    }
}

#[cfg(target_os = "windows")]
fn main() {
    app::run();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("error"))
        .with_target(false)
        .try_init();
    tracing::error!("imeswitch only supports Windows.");
}
