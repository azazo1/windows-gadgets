use std::sync::mpsc::Receiver;
use std::time::Instant;
use tracing::info;

use super::config::Config;
use super::ffi;
use super::hotkey;

pub struct Runner {
    config: Config,
    prev_foreground_window: Option<isize>,
    hotkey_rx: Option<Receiver<()>>,
}

impl Runner {
    pub fn new(config: Config) -> Self {
        let prev_foreground_window = ffi::foreground_window();
        let hotkey_rx = hotkey::spawn_escape_listener(config.escape_switching);

        Self {
            config,
            prev_foreground_window,
            hotkey_rx,
        }
    }

    pub fn run(&mut self) -> ! {
        let mut last_tick = Instant::now();

        loop {
            self.ensure_chinese_mode_if_needed();
            self.reset_method_on_focus_change_if_needed();
            self.handle_escape_hotkey();

            if self.config.verbose && last_tick.elapsed() >= self.config.tick_interval {
                info!("imeswitch alive");
                last_tick = Instant::now();
            }

            std::thread::sleep(self.config.poll_interval);
        }
    }

    fn ensure_chinese_mode_if_needed(&self) {
        if !self.config.ensure_chinese_mode {
            return;
        }

        let Some(layout) = ffi::current_layout_id() else {
            return;
        };
        if layout != self.config.locale_zh {
            return;
        }

        let Some(mode) = ffi::get_input_mode() else {
            return;
        };

        if mode & 0x01 == 0 {
            let _ = ffi::switch_input_mode(1);
        }
    }

    fn reset_method_on_focus_change_if_needed(&mut self) {
        if !self.config.ime_resetting {
            return;
        }

        let now = ffi::foreground_window();
        if now != self.prev_foreground_window {
            self.prev_foreground_window = now;
            let _ = ffi::switch_input_method(self.config.locale_en);
        }
    }

    fn handle_escape_hotkey(&self) {
        let Some(rx) = &self.hotkey_rx else {
            return;
        };

        while rx.try_recv().is_ok() {
            let _ = ffi::switch_input_method(self.config.locale_en);
        }
    }
}
