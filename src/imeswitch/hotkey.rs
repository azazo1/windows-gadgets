use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use rdev::{Event, EventType, Key, listen};
use tracing::error;

#[derive(Default)]
struct HotkeyState {
    ctrl_down: bool,
    trigger_down: bool,
    triggered: bool,
}

fn is_ctrl(key: Key) -> bool {
    matches!(key, Key::ControlLeft | Key::ControlRight)
}

fn is_trigger_key(key: Key) -> bool {
    matches!(key, Key::LeftBracket | Key::Escape)
}

pub fn spawn_escape_listener(enabled: bool) -> Option<Receiver<()>> {
    if !enabled {
        return None;
    }

    let (tx, rx) = mpsc::channel();
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
                        if is_trigger_key(key) {
                            state.trigger_down = true;
                        }

                        if state.ctrl_down && state.trigger_down && !state.triggered {
                            let _ = tx.send(());
                            state.triggered = true;
                        }
                    }
                    EventType::KeyRelease(key) => {
                        if is_ctrl(key) {
                            state.ctrl_down = false;
                        }
                        if is_trigger_key(key) {
                            state.trigger_down = false;
                        }

                        if !state.ctrl_down || !state.trigger_down {
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
