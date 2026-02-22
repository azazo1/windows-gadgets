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

#[derive(Debug, Clone, Copy)]
pub enum Action {
    None,
    SwitchTo(Direction),
    Scroll(i32),
    OpenTextEditor,
    OpenVsCode,
    OpenPwsh,
    SwitchIme,
}

pub static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));
