use std::sync::Arc;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinSet;
use tokio::time::interval;
use tracing::error;
use tracing::info;

use super::config::Config;
use super::ffi;
use super::hotkey;

pub struct Runner {
    config: Arc<Config>,
    hotkey_rx: Option<UnboundedReceiver<()>>,
}

impl Runner {
    pub fn new(config: Config) -> Self {
        let hotkey_rx = hotkey::spawn_escape_listener(config.escape_switching);

        Self {
            config: Arc::new(config),
            hotkey_rx,
        }
    }

    pub async fn run(mut self) -> ! {
        let mut tasks = JoinSet::new();

        if self.config.ensure_chinese_mode {
            let config = Arc::clone(&self.config);
            tasks.spawn(async move {
                Self::run_chinese_mode_guard(config).await;
            });
        }

        if self.config.ime_resetting {
            let config = Arc::clone(&self.config);
            tasks.spawn(async move {
                Self::run_focus_reset_loop(config).await;
            });
        }

        if let Some(hotkey_rx) = self.hotkey_rx.take() {
            let config = Arc::clone(&self.config);
            tasks.spawn(async move {
                Self::run_hotkey_consumer(hotkey_rx, config).await;
            });
        }

        loop {
            match tasks.join_next().await {
                Some(Ok(())) => {}
                Some(Err(err)) => {
                    error!(?err, "imeswitch child task exited unexpectedly");
                }
                None => {
                    error!("imeswitch has no active child tasks");
                    std::future::pending::<()>().await;
                }
            }
        }
    }

    async fn run_chinese_mode_guard(config: Arc<Config>) {
        let mut ticker = interval(config.poll_interval);
        loop {
            ticker.tick().await;
            let Some(layout) = ffi::current_layout_id() else {
                continue;
            };
            if layout != config.locale_zh {
                continue;
            }

            let Some(mode) = ffi::get_input_mode() else {
                continue;
            };

            if mode & 0x01 == 0 {
                let _ = ffi::switch_input_mode(1);
            }
        }
    }

    async fn run_focus_reset_loop(config: Arc<Config>) {
        let mut prev_foreground_window = ffi::foreground_window();
        let mut ticker = interval(config.poll_interval);

        loop {
            ticker.tick().await;
            let now = ffi::foreground_window();
            if now != prev_foreground_window {
                prev_foreground_window = now;
                info!("foreground changed, reset to english ime.");
                let _ = ffi::switch_input_method(config.locale_en);
            }
        }
    }

    async fn run_hotkey_consumer(mut rx: UnboundedReceiver<()>, config: Arc<Config>) {
        while rx.recv().await.is_some() {
            info!("key: [escape], reset to english ime.");
            let _ = ffi::switch_input_method(config.locale_en);
        }
    }
}
