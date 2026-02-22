use rdev::Key;
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn angle(self) -> f64 {
        match self {
            Self::Up => -90.0,
            Self::Down => 90.0,
            Self::Left => 180.0,
            Self::Right => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct State {
    pub caps_lock_pressing: bool,
    pub lshift_pressing: bool,
    pub pending_key: Option<Key>,
    pub operations: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            caps_lock_pressing: false,
            lshift_pressing: false,
            pending_key: None,
            operations: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    SwitchTo(Direction),
    Scroll(i32),
    SwitchIme,
    /// 打开指定程序
    OpenProgram {
        program: String,
    },
    /// 切换到指定窗口标题
    SwitchWindow {
        title: String,
    },
    /// 切换到指定窗口或打开程序
    SwitchOrOpen {
        window_title: String,
        program: String,
    },
}

pub static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));
