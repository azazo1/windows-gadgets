use std::sync::{Arc, Mutex};

use rdev::{Event, EventType, Key, grab, listen};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tracing::error;

use super::ffi;

#[derive(Default)]
struct AltKeyState {
    down: bool,
    passthrough: bool,
    synthetic_presses: u8,
    synthetic_releases: u8,
}

#[derive(Default)]
struct HotkeyState {
    ctrl_down: bool,
    bracket_down: bool,
    escape_down: bool,
    escape_triggered: bool,
    left_alt: AltKeyState,
    right_alt: AltKeyState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    SwitchEnglish,
    SwitchChinese,
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

fn is_left_alt_key(key: Key) -> bool {
    matches!(key, Key::Alt)
}

fn is_right_alt_key(key: Key) -> bool {
    matches!(key, Key::AltGr)
}

fn is_alt_passthrough_combo_key(key: Key) -> bool {
    !matches!(
        key,
        Key::Alt
            | Key::AltGr
            | Key::ControlLeft
            | Key::ControlRight
            | Key::ShiftLeft
            | Key::ShiftRight
            | Key::MetaLeft
            | Key::MetaRight
            | Key::CapsLock
    )
}

fn consume_escape_hotkey(
    state: &mut HotkeyState,
    escape_switching_enabled: bool,
) -> Option<HotkeyAction> {
    if !escape_switching_enabled {
        return None;
    }
    if state.left_alt.down || state.right_alt.down {
        return None;
    }

    let should_trigger = state.escape_down || (state.ctrl_down && state.bracket_down);
    if should_trigger && !state.escape_triggered {
        state.escape_triggered = true;
        return Some(HotkeyAction::SwitchEnglish);
    }

    None
}

fn reset_escape_hotkey(state: &mut HotkeyState, escape_switching_enabled: bool) {
    if !escape_switching_enabled {
        return;
    }

    let still_pressed = state.escape_down || (state.ctrl_down && state.bracket_down);
    if !still_pressed {
        state.escape_triggered = false;
    }
}

fn consume_synthetic_alt_event(state: &mut HotkeyState, key: Key, is_press: bool) -> bool {
    let alt = if is_left_alt_key(key) {
        Some(&mut state.left_alt)
    } else if is_right_alt_key(key) {
        Some(&mut state.right_alt)
    } else {
        None
    };

    let Some(alt) = alt else {
        return false;
    };

    let counter = if is_press {
        &mut alt.synthetic_presses
    } else {
        &mut alt.synthetic_releases
    };
    if *counter == 0 {
        return false;
    }

    *counter -= 1;
    true
}

fn mark_alt_combo(alt: &mut AltKeyState, inject_flag: &mut bool) {
    if alt.down && !alt.passthrough {
        alt.passthrough = true;
        alt.synthetic_presses = alt.synthetic_presses.saturating_add(1);
        *inject_flag = true;
    }
}

pub fn spawn_hotkey_listener(
    escape_switching_enabled: bool,
    alt_switching_enabled: bool,
) -> Option<UnboundedReceiver<HotkeyAction>> {
    if !escape_switching_enabled && !alt_switching_enabled {
        return None;
    }

    let (tx, rx) = unbounded_channel();
    let state = Arc::new(Mutex::new(HotkeyState::default()));

    if !alt_switching_enabled {
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

                            if let Some(action) =
                                consume_escape_hotkey(&mut state, escape_switching_enabled)
                            {
                                let _ = tx.send(action);
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

                            reset_escape_hotkey(&mut state, escape_switching_enabled);
                        }
                        _ => {}
                    }
                };

                if let Err(err) = listen(callback) {
                    error!(?err, "hotkey listener stopped");
                }
            }
        });

        return Some(rx);
    }

    std::thread::spawn({
        let state = Arc::clone(&state);
        move || {
            let callback = move |event: Event| -> Option<Event> {
                let mut suppress = false;
                let mut inject_left_down = false;
                let mut inject_right_down = false;
                let mut inject_left_up = false;
                let mut inject_right_up = false;
                let mut emitted_action = None;

                {
                    let mut state = match state.lock() {
                        Ok(guard) => guard,
                        Err(_) => return Some(event),
                    };

                    match event.event_type {
                        EventType::KeyPress(key) => {
                            if consume_synthetic_alt_event(&mut state, key, true) {
                                return Some(event);
                            }

                            if is_ctrl(key) {
                                state.ctrl_down = true;
                            }
                            if is_bracket_key(key) {
                                state.bracket_down = true;
                            }
                            if is_escape_key(key) {
                                state.escape_down = true;
                            }

                            if is_left_alt_key(key) {
                                if !state.left_alt.down {
                                    state.left_alt.down = true;
                                    state.left_alt.passthrough = false;
                                }
                                suppress = true;
                            } else if is_right_alt_key(key) {
                                if !state.right_alt.down {
                                    state.right_alt.down = true;
                                    state.right_alt.passthrough = false;
                                }
                                suppress = true;
                            } else {
                                if is_alt_passthrough_combo_key(key) {
                                    mark_alt_combo(&mut state.left_alt, &mut inject_left_down);
                                    mark_alt_combo(&mut state.right_alt, &mut inject_right_down);
                                }
                                emitted_action =
                                    consume_escape_hotkey(&mut state, escape_switching_enabled);
                            }
                        }
                        EventType::KeyRelease(key) => {
                            if consume_synthetic_alt_event(&mut state, key, false) {
                                return Some(event);
                            }

                            if is_ctrl(key) {
                                state.ctrl_down = false;
                            }
                            if is_bracket_key(key) {
                                state.bracket_down = false;
                            }
                            if is_escape_key(key) {
                                state.escape_down = false;
                            }

                            if is_left_alt_key(key) {
                                if state.left_alt.down {
                                    if state.left_alt.passthrough {
                                        state.left_alt.synthetic_releases =
                                            state.left_alt.synthetic_releases.saturating_add(1);
                                        inject_left_up = true;
                                    } else {
                                        emitted_action = Some(HotkeyAction::SwitchEnglish);
                                    }
                                    state.left_alt.down = false;
                                    state.left_alt.passthrough = false;
                                }
                                suppress = true;
                            } else if is_right_alt_key(key) {
                                if state.right_alt.down {
                                    if state.right_alt.passthrough {
                                        state.right_alt.synthetic_releases =
                                            state.right_alt.synthetic_releases.saturating_add(1);
                                        inject_right_up = true;
                                    } else {
                                        emitted_action = Some(HotkeyAction::SwitchChinese);
                                    }
                                    state.right_alt.down = false;
                                    state.right_alt.passthrough = false;
                                }
                                suppress = true;
                            }

                            reset_escape_hotkey(&mut state, escape_switching_enabled);
                        }
                        _ => {}
                    }
                }

                if inject_left_down {
                    ffi::emit_alt_key(true, false);
                }
                if inject_right_down {
                    ffi::emit_alt_key(false, false);
                }
                if inject_left_up {
                    ffi::emit_alt_key(true, true);
                }
                if inject_right_up {
                    ffi::emit_alt_key(false, true);
                }
                if let Some(action) = emitted_action {
                    let _ = tx.send(action);
                }

                if suppress { None } else { Some(event) }
            };

            if let Err(err) = grab(callback) {
                error!(?err, "hotkey listener stopped");
            }
        }
    });

    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::is_alt_passthrough_combo_key;
    use rdev::Key;

    #[test]
    fn alt_passthrough_requires_non_modifier_key() {
        assert!(is_alt_passthrough_combo_key(Key::Escape));
        assert!(is_alt_passthrough_combo_key(Key::LeftBracket));
        assert!(!is_alt_passthrough_combo_key(Key::Alt));
        assert!(!is_alt_passthrough_combo_key(Key::AltGr));
        assert!(!is_alt_passthrough_combo_key(Key::ControlLeft));
        assert!(!is_alt_passthrough_combo_key(Key::ShiftRight));
        assert!(!is_alt_passthrough_combo_key(Key::MetaLeft));
        assert!(!is_alt_passthrough_combo_key(Key::CapsLock));
    }
}
