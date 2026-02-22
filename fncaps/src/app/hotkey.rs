use std::net::TcpListener;
use std::sync::OnceLock;

use rdev::{Event, EventType, Key, grab};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, KEYEVENTF_KEYUP, MOUSEEVENTF_WHEEL, VK_LSHIFT, keybd_event, mouse_event,
};

use super::config::{HotkeyConfig, load_hotkey_config};
use super::ime;
use super::launch;
use super::state::{Action, STATE};
use super::windows_ops;

static HOTKEY_CONFIG: OnceLock<HotkeyConfig> = OnceLock::new();

pub fn run() -> Result<(), String> {
    let _instance = TcpListener::bind("127.0.0.1:23982")
        .map_err(|e| format!("failed to bind singleton port: {e}"))?;

    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let _ = std::env::set_current_dir(parent);
    }

    let cfg = load_hotkey_config();
    let _ = HOTKEY_CONFIG.set(cfg);

    let loaded = HOTKEY_CONFIG
        .get()
        .map(|c| c.rules.len())
        .unwrap_or_default();
    tracing::info!(target: "fncaps::hotkey", rules = loaded, "hotkey rules ready");

    tracing::info!(target: "fncaps::hotkey", "global keyboard capture started");
    grab(event_callback).map_err(|e| format!("keyboard grab failed: {e:?}"))
}

fn event_callback(event: Event) -> Option<Event> {
    let (key, is_pressing) = match event.event_type {
        EventType::KeyPress(key) => (key, true),
        EventType::KeyRelease(key) => (key, false),
        _ => return Some(event),
    };

    let mut action = Action::None;
    let mut suppress = false;

    {
        let mut state = match STATE.lock() {
            Ok(s) => s,
            Err(_) => return Some(event),
        };

        if state.pending_key == Some(key) {
            if is_pressing {
                if state.caps_lock_pressing {
                    tracing::debug!(target: "fncaps::hotkey", ?key, "suppress repeat while key pending");
                    return None;
                }
                return Some(event);
            }

            state.pending_key = None;
            tracing::trace!(target: "fncaps::hotkey", ?key, "suppress pending key release");
            return None;
        }

        if key == Key::CapsLock {
            if state.caps_lock_pressing != is_pressing {
                state.caps_lock_pressing = is_pressing;
                tracing::debug!(target: "fncaps::hotkey", pressing = is_pressing, "capslock state changed");
                if is_pressing {
                    state.operations = false;
                } else if !state.operations {
                    action = Action::SwitchIme;
                }
            }
            suppress = true;
        } else if key == Key::ShiftLeft {
            state.lshift_pressing = is_pressing;
            state.operations = true;
            tracing::trace!(target: "fncaps::hotkey", pressing = is_pressing, "left shift state changed");
            return Some(event);
        }

        if state.caps_lock_pressing && is_pressing {
            if let Some(rule) = HOTKEY_CONFIG
                .get()
                .and_then(|cfg| cfg.resolve(key, state.lshift_pressing))
            {
                if rule.pending {
                    state.pending_key = Some(key);
                }
                state.operations = true;
                action = rule.action.clone();
                suppress = rule.suppress;
                tracing::info!(
                    target: "fncaps::hotkey",
                    ?key,
                    ?rule.action,
                    pending = rule.pending,
                    suppress = rule.suppress,
                    rule = %rule.description,
                    "matched configured caps binding"
                );
            }
        }

        if matches!(action, Action::SwitchIme)
            && !is_pressing
            && key == Key::CapsLock
            && let Some(cfg) = HOTKEY_CONFIG.get()
        {
            action = cfg.caps_tap_action.clone();
            tracing::info!(target: "fncaps::hotkey", ?action, "caps tap action from config");
        }
    }

    execute_action(action, key, is_pressing);

    if suppress { None } else { Some(event) }
}

fn execute_action(action: Action, key: Key, is_pressing: bool) {
    match action {
        Action::None => {
            if is_pressing {
                tracing::trace!(target: "fncaps::hotkey", ?key, "pass-through key event");
            }
        }
        Action::SwitchTo(direction) => {
            tracing::info!(target: "fncaps::hotkey", ?direction, ?key, "trigger window switch");
            windows_ops::switch_to(direction)
        }
        Action::Scroll(delta) => {
            tracing::info!(target: "fncaps::hotkey", delta, ?key, "trigger wheel scrolling");
            scroll_with_lshift_workaround(delta)
        }
        Action::SwitchIme => {
            tracing::info!(target: "fncaps::hotkey", "caps tap detected, switching IME");
            ime::switch_im()
        }
        Action::OpenProgram { program } => {
            tracing::info!(target: "fncaps::hotkey", ?key, program = %program, "trigger custom program launch");
            launch::open_program(&program)
        }
        Action::SwitchWindow { title } => {
            tracing::info!(target: "fncaps::hotkey", ?key, window_title = %title, "trigger window switch to specific");
            windows_ops::switch_to_window(&title)
        }
        Action::SwitchOrOpen {
            window_title,
            program,
        } => {
            tracing::info!(
                target: "fncaps::hotkey",
                ?key,
                window_title = %window_title,
                program = %program,
                "trigger switch-or-open"
            );
            windows_ops::switch_to_window_or_open(&window_title, &program)
        }
    }
}

fn scroll_with_lshift_workaround(delta: i32) {
    let shift_down = unsafe { GetKeyState(VK_LSHIFT.0 as i32) } < 0;
    unsafe {
        if shift_down {
            keybd_event(VK_LSHIFT.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, delta * 120, 0);
        if shift_down {
            keybd_event(VK_LSHIFT.0 as u8, 0, Default::default(), 0);
        }
    }
    tracing::debug!(target: "fncaps::hotkey", delta, shift_down, "scroll wheel sent");
}
