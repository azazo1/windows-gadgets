use std::sync::{Arc, Mutex};

use rdev::{Event, EventType, Key, listen};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tracing::error;

#[derive(Default)]
struct HotkeyState {
    ctrl_down: bool,
    bracket_down: bool,
    escape_down: bool,
    triggered: bool,
}

fn is_ctrl(key: Key) -> bool {
    matches!(key, Key::ControlLeft | Key::ControlRight)
}

fn is_bracket_key(key: Key) -> bool {
    matches!(key, Key::LeftBracket)
}

fn is_escape_key(key: Key) -> bool {
    matches!(key, Key::Escape)
}

pub fn spawn_escape_listener(enabled: bool) -> Option<UnboundedReceiver<()>> {
    if !enabled {
        return None;
    }

    let (tx, rx) = unbounded_channel();
    let state = Arc::new(Mutex::new(HotkeyState::default()));

    std::thread::spawn({
        let state = Arc::clone(&state);
        move || {
            let callback = move |event: Event| {
                let mut state = match state.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };

                match event.event_type {
                    EventType::KeyPress(key) => {
                        if is_ctrl(key) {
                            state.ctrl_down = true;
                        }
                        if is_bracket_key(key) {
                            state.bracket_down = true;
                        }
                        if is_escape_key(key) {
                            state.escape_down = true;
                        }

                        let should_trigger =
                            state.escape_down || (state.ctrl_down && state.bracket_down);
                        if should_trigger && !state.triggered {
                            let _ = tx.send(());
                            state.triggered = true;
                        }
                    }
                    EventType::KeyRelease(key) => {
                        if is_ctrl(key) {
                            state.ctrl_down = false;
                        }
                        if is_bracket_key(key) {
                            state.bracket_down = false;
                        }
                        if is_escape_key(key) {
                            state.escape_down = false;
                        }

                        let still_pressed =
                            state.escape_down || (state.ctrl_down && state.bracket_down);
                        if !still_pressed {
                            state.triggered = false;
                        }
                    }
                    _ => {}
                }
            };

            if let Err(err) = listen(callback) {
                error!(?err, "hotkey listener stopped");
            }
        }
    });

    Some(rx)
}
