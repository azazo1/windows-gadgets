use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub ime_resetting: bool,
    pub escape_switching: bool,
    pub locale_en: u32,
    pub locale_zh: u16,
    pub ensure_chinese_mode: bool,
    pub poll_interval: Duration,
}
