use std::fs;
use std::path::{Path, PathBuf};

use rdev::Key;
use serde::{Deserialize, Serialize};

use super::state::{Action, Direction};

/// 单个热键规则，绑定 CapsLock + key 组合到特定操作
///
/// 例如: CapsLock + H 切换窗口焦点到左边的窗口
#[derive(Debug, Clone)]
pub struct HotkeyRule {
    /// 触发此规则的按键 (h, l, k, j, e, v, p, left, right, up, down, space, enter, tab, f1-f12 等)
    pub key: Key,
    /// Shift 键的状态要求 (任意/必须按下/必须未按下)
    pub shift: ShiftRequirement,
    /// 触发此规则时执行的操作 (切换窗口、打开程序、切换输入法等)
    pub action: Action,
    /// 是否拦截键盘事件，阻止系统处理此按键 (true: 吞键, false: 传递给系统)
    /// 通常对 CapsLock 组合键设置为 true 以防止意外输入
    pub suppress: bool,
    /// 是否在按住该键时吞掉后续的重复事件 (true: 吞掉重复, false: 允许重复)
    /// 用来防止长按时触发多次相同操作
    pub pending: bool,
    /// 规则的可读描述 (用于日志输出，如 "caps+h")
    pub description: String,
}

/// CapsLock 快捷键中 Shift 键的状态要求
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftRequirement {
    /// 任意: 无论 Shift 键是否按下都能触发此规则 (CapsLock + key)
    Any,
    /// 按下: 仅当 Shift 键被按下时触发此规则 (CapsLock + Shift + key)
    Down,
    /// 未按下: 仅当 Shift 键未被按下时触发此规则 (CapsLock + key，不含 Shift)
    Up,
}

/// CapsLock 热键配置的完整集合，包括单击动作和所有组合键规则
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// CapsLock 单击（无其他键按下时）执行的动作 (通常是切换输入法)
    pub caps_tap_action: Action,
    /// 所有 CapsLock + key 组合键规则的列表
    pub rules: Vec<HotkeyRule>,
}

impl HotkeyConfig {
    pub fn resolve(&self, key: Key, lshift_pressing: bool) -> Option<&HotkeyRule> {
        self.rules.iter().find(|rule| {
            if rule.key != key {
                return false;
            }

            match rule.shift {
                ShiftRequirement::Any => true,
                ShiftRequirement::Down => lshift_pressing,
                ShiftRequirement::Up => !lshift_pressing,
            }
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct FileConfig {
    caps: Option<CapsConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CapsConfig {
    tap_action: Option<String>,
    bindings: Option<Vec<BindingConfig>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BindingConfig {
    key: String,
    action: String,
    shift: Option<String>,
    suppress: Option<bool>,
    pending: Option<bool>,
}

pub fn load_hotkey_config() -> HotkeyConfig {
    let path = config_path();
    let Some(path) = path else {
        tracing::warn!(target: "fncaps::config", "cannot determine config path, use built-in defaults");
        return default_hotkey_config();
    };

    if !path.exists() {
        tracing::warn!(target: "fncaps::config", path = %path.display(), "config file not found");
        let default_cfg = default_hotkey_config();
        // 尝试生成并保存默认配置
        if let Err(e) = save_default_config(&path, &default_cfg) {
            tracing::warn!(target: "fncaps::config", path = %path.display(), error = %e, "failed to save default config");
        } else {
            tracing::info!(target: "fncaps::config", path = %path.display(), "default config generated and saved");
        }
        return default_cfg;
    }

    match load_hotkey_config_from_file(&path) {
        Ok(cfg) => {
            tracing::info!(
                target: "fncaps::config",
                path = %path.display(),
                rules = cfg.rules.len(),
                ?cfg.caps_tap_action,
                "hotkey config loaded"
            );
            cfg
        }
        Err(err) => {
            tracing::error!(target: "fncaps::config", path = %path.display(), error = %err, "invalid config, fallback to defaults");
            default_hotkey_config()
        }
    }
}

fn load_hotkey_config_from_file(path: &Path) -> Result<HotkeyConfig, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("failed to read config '{}': {e}", path.display()))?;

    let parsed: FileConfig = toml::from_str(&raw)
        .map_err(|e| format!("failed to parse TOML '{}': {e}", path.display()))?;

    build_config_from_file(parsed)
}

/// 将默认配置转换为 FileConfig 并保存到文件
fn save_default_config(path: &Path, cfg: &HotkeyConfig) -> Result<(), String> {
    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create config directory '{}': {e}", parent.display()))?;
    }

    // 转换为可序列化的 FileConfig
    let file_cfg = config_to_file(cfg);

    // 序列化为 TOML
    let toml_str = toml::to_string_pretty(&file_cfg)
        .map_err(|e| format!("failed to serialize config: {e}"))?;

    // 保存到文件
    fs::write(path, toml_str)
        .map_err(|e| format!("failed to write config '{}': {e}", path.display()))?;

    Ok(())
}

/// 将 HotkeyConfig 转换为 FileConfig（用于序列化）
fn config_to_file(cfg: &HotkeyConfig) -> FileConfig {
    let tap_action = action_to_string(&cfg.caps_tap_action);

    let bindings = cfg
        .rules
        .iter()
        .map(|rule| {
            let key_str = key_to_string(rule.key);
            let action_str = action_to_string(&rule.action);
            let shift_str = match rule.shift {
                super::config::ShiftRequirement::Any => None,
                super::config::ShiftRequirement::Down => Some("down".to_string()),
                super::config::ShiftRequirement::Up => Some("up".to_string()),
            };

            BindingConfig {
                key: key_str,
                action: action_str,
                shift: shift_str,
                suppress: Some(rule.suppress),
                pending: Some(rule.pending),
            }
        })
        .collect();

    FileConfig {
        caps: Some(CapsConfig {
            tap_action: Some(tap_action),
            bindings: Some(bindings),
        }),
    }
}

/// 将 Action 转换为字符串表示
fn action_to_string(action: &Action) -> String {
    match action {
        Action::None => "none".to_string(),
        Action::SwitchTo(Direction::Left) => "switch_left".to_string(),
        Action::SwitchTo(Direction::Right) => "switch_right".to_string(),
        Action::SwitchTo(Direction::Up) => "switch_up".to_string(),
        Action::SwitchTo(Direction::Down) => "switch_down".to_string(),
        Action::Scroll(n) if *n > 0 => "scroll_up".to_string(),
        Action::Scroll(_) => "scroll_down".to_string(),
        Action::SwitchIme => "switch_ime".to_string(),
        Action::OpenProgram { program } => format!("open_program:{}", program),
        Action::SwitchWindow { title } => format!("switch_window:{}", title),
        Action::SwitchOrOpen { window_title, program } => {
            format!("switch_or_open:{}:{}", window_title, program)
        }
    }
}

/// 将 Key 转换为字符串表示
fn key_to_string(key: Key) -> String {
    match key {
        Key::KeyA => "a",
        Key::KeyB => "b",
        Key::KeyC => "c",
        Key::KeyD => "d",
        Key::KeyE => "e",
        Key::KeyF => "f",
        Key::KeyG => "g",
        Key::KeyH => "h",
        Key::KeyI => "i",
        Key::KeyJ => "j",
        Key::KeyK => "k",
        Key::KeyL => "l",
        Key::KeyM => "m",
        Key::KeyN => "n",
        Key::KeyO => "o",
        Key::KeyP => "p",
        Key::KeyQ => "q",
        Key::KeyR => "r",
        Key::KeyS => "s",
        Key::KeyT => "t",
        Key::KeyU => "u",
        Key::KeyV => "v",
        Key::KeyW => "w",
        Key::KeyX => "x",
        Key::KeyY => "y",
        Key::KeyZ => "z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::LeftArrow => "left",
        Key::RightArrow => "right",
        Key::UpArrow => "up",
        Key::DownArrow => "down",
        Key::Space => "space",
        Key::Return => "enter",
        Key::Tab => "tab",
        Key::Escape => "esc",
        Key::Backspace => "backspace",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Insert => "insert",
        Key::Delete => "delete",
        Key::F1 => "f1",
        Key::F2 => "f2",
        Key::F3 => "f3",
        Key::F4 => "f4",
        Key::F5 => "f5",
        Key::F6 => "f6",
        Key::F7 => "f7",
        Key::F8 => "f8",
        Key::F9 => "f9",
        Key::F10 => "f10",
        Key::F11 => "f11",
        Key::F12 => "f12",
        _ => "unknown",
    }
    .to_string()
}

fn build_config_from_file(parsed: FileConfig) -> Result<HotkeyConfig, String> {
    let caps = parsed.caps.unwrap_or(CapsConfig {
        tap_action: None,
        bindings: None,
    });

    let tap_action = caps
        .tap_action
        .as_deref()
        .map(parse_action)
        .transpose()?
        .unwrap_or(Action::SwitchIme);

    let Some(bindings) = caps.bindings else {
        return Ok(default_hotkey_config_with_tap(tap_action));
    };

    let mut rules = Vec::with_capacity(bindings.len());

    for (index, binding) in bindings.into_iter().enumerate() {
        let key = parse_key(&binding.key)
            .ok_or_else(|| format!("bindings[{index}] unknown key: '{}'", binding.key))?;

        let action = parse_action(binding.action.as_str())
            .map_err(|e| format!("bindings[{index}] action error: {e}"))?;

        let shift = parse_shift(binding.shift.as_deref())
            .map_err(|e| format!("bindings[{index}] shift error: {e}"))?;

        let suppress = binding.suppress.unwrap_or(true);
        let pending = binding.pending.unwrap_or(true);
        let description = format!("caps+{}", binding.key);

        rules.push(HotkeyRule {
            key,
            shift,
            action,
            suppress,
            pending,
            description,
        });
    }

    Ok(HotkeyConfig {
        caps_tap_action: tap_action,
        rules,
    })
}

fn config_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("FNCAPS_CONFIG") {
        let custom_path = PathBuf::from(custom);
        tracing::info!(target: "fncaps::config", path = %custom_path.display(), "using config path from FNCAPS_CONFIG");
        return Some(custom_path);
    }

    let dir = dirs_next::config_dir()?.join("fncaps");
    Some(dir.join("fncaps.toml"))
}

fn default_hotkey_config() -> HotkeyConfig {
    default_hotkey_config_with_tap(Action::SwitchIme)
}

fn default_hotkey_config_with_tap(tap_action: Action) -> HotkeyConfig {
    HotkeyConfig {
        caps_tap_action: tap_action,
        rules: vec![
            mk_rule(
                Key::LeftArrow,
                ShiftRequirement::Any,
                Action::SwitchTo(Direction::Left),
                true,
                true,
                "caps+left",
            ),
            mk_rule(
                Key::KeyH,
                ShiftRequirement::Any,
                Action::SwitchTo(Direction::Left),
                true,
                true,
                "caps+h",
            ),
            mk_rule(
                Key::RightArrow,
                ShiftRequirement::Any,
                Action::SwitchTo(Direction::Right),
                true,
                true,
                "caps+right",
            ),
            mk_rule(
                Key::KeyL,
                ShiftRequirement::Any,
                Action::SwitchTo(Direction::Right),
                true,
                true,
                "caps+l",
            ),
            mk_rule(
                Key::UpArrow,
                ShiftRequirement::Down,
                Action::Scroll(1),
                true,
                false,
                "caps+shift+up",
            ),
            mk_rule(
                Key::KeyK,
                ShiftRequirement::Down,
                Action::Scroll(1),
                true,
                false,
                "caps+shift+k",
            ),
            mk_rule(
                Key::UpArrow,
                ShiftRequirement::Up,
                Action::SwitchTo(Direction::Up),
                true,
                true,
                "caps+up",
            ),
            mk_rule(
                Key::KeyK,
                ShiftRequirement::Up,
                Action::SwitchTo(Direction::Up),
                true,
                true,
                "caps+k",
            ),
            mk_rule(
                Key::DownArrow,
                ShiftRequirement::Down,
                Action::Scroll(-1),
                true,
                false,
                "caps+shift+down",
            ),
            mk_rule(
                Key::KeyJ,
                ShiftRequirement::Down,
                Action::Scroll(-1),
                true,
                false,
                "caps+shift+j",
            ),
            mk_rule(
                Key::DownArrow,
                ShiftRequirement::Up,
                Action::SwitchTo(Direction::Down),
                true,
                true,
                "caps+down",
            ),
            mk_rule(
                Key::KeyJ,
                ShiftRequirement::Up,
                Action::SwitchTo(Direction::Down),
                true,
                true,
                "caps+j",
            ),
            mk_rule(
                Key::KeyE,
                ShiftRequirement::Any,
                Action::OpenProgram {
                    program: "notepad.exe".to_string(),
                },
                true,
                true,
                "caps+e",
            ),
            mk_rule(
                Key::KeyV,
                ShiftRequirement::Any,
                Action::SwitchOrOpen {
                    window_title: "Code".to_string(),
                    program: "Code.exe".to_string(),
                },
                true,
                true,
                "caps+v",
            ),
            mk_rule(
                Key::KeyP,
                ShiftRequirement::Any,
                Action::SwitchOrOpen {
                    window_title: "PowerShell".to_string(),
                    program: "pwsh.exe".to_string(),
                },
                true,
                true,
                "caps+p",
            ),
        ],
    }
}

fn mk_rule(
    key: Key,
    shift: ShiftRequirement,
    action: Action,
    suppress: bool,
    pending: bool,
    description: &str,
) -> HotkeyRule {
    HotkeyRule {
        key,
        shift,
        action,
        suppress,
        pending,
        description: description.to_string(),
    }
}

fn parse_shift(value: Option<&str>) -> Result<ShiftRequirement, String> {
    let Some(value) = value else {
        return Ok(ShiftRequirement::Any);
    };

    match value.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(ShiftRequirement::Any),
        "down" | "pressed" => Ok(ShiftRequirement::Down),
        "up" | "released" => Ok(ShiftRequirement::Up),
        other => Err(format!(
            "unsupported shift mode '{other}', expected any/down/up"
        )),
    }
}

fn parse_action(value: &str) -> Result<Action, String> {
    let trimmed = value.trim().to_ascii_lowercase();

    // 简单的不带参数的 action
    if let Ok(action) = parse_simple_action(&trimmed) {
        return Ok(action);
    }

    // 带参数的 action: 按冒号分割
    if let Some(colon_pos) = trimmed.find(':') {
        let action_name = &trimmed[..colon_pos];
        let params = &trimmed[colon_pos + 1..];

        return match action_name {
            "open_program" | "open_app" => {
                if params.is_empty() {
                    Err("open_program requires a program name".to_string())
                } else {
                    Ok(Action::OpenProgram {
                        program: params.to_string(),
                    })
                }
            }
            "switch_window" | "switch_win" => {
                if params.is_empty() {
                    Err("switch_window requires a window title".to_string())
                } else {
                    Ok(Action::SwitchWindow {
                        title: params.to_string(),
                    })
                }
            }
            "switch_or_open" | "switch_or_launch" => {
                // 格式: switch_or_open:window_title:program_path
                // 或: switch_or_open:window_title|program_path (也支持管道符)
                let separator = if params.contains('|') { '|' } else { ':' };
                let parts: Vec<&str> = params.split(separator).collect();

                if parts.len() < 2 {
                    return Err(
                        "switch_or_open requires: switch_or_open:window_title:program_path"
                            .to_string(),
                    );
                }

                let window_title = parts[0].trim().to_string();
                let program = parts[1..].join(":").trim().to_string(); // 允许 program 路径中含冒号

                if window_title.is_empty() || program.is_empty() {
                    Err("switch_or_open requires non-empty window_title and program".to_string())
                } else {
                    Ok(Action::SwitchOrOpen {
                        window_title,
                        program,
                    })
                }
            }
            _other => Err(format!("unsupported parameterized action '{action_name}'")),
        };
    }

    Err(format!("unsupported action '{trimmed}'"))
}

fn parse_simple_action(value: &str) -> Result<Action, String> {
    match value {
        "none" => Ok(Action::None),
        "switch_ime" => Ok(Action::SwitchIme),
        "switch_left" => Ok(Action::SwitchTo(Direction::Left)),
        "switch_right" => Ok(Action::SwitchTo(Direction::Right)),
        "switch_up" => Ok(Action::SwitchTo(Direction::Up)),
        "switch_down" => Ok(Action::SwitchTo(Direction::Down)),
        "scroll_up" => Ok(Action::Scroll(1)),
        "scroll_down" => Ok(Action::Scroll(-1)),
        _ => Err(format!("unknown simple action '{value}'")),
    }
}

fn parse_key(value: &str) -> Option<Key> {
    let normalized = value.trim().to_ascii_lowercase();

    let key = match normalized.as_str() {
        "left" | "leftarrow" => Key::LeftArrow,
        "right" | "rightarrow" => Key::RightArrow,
        "up" | "uparrow" => Key::UpArrow,
        "down" | "downarrow" => Key::DownArrow,
        "space" => Key::Space,
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => {
            if normalized.len() == 1 {
                let ch = normalized.chars().next()?;
                return parse_single_char_key(ch);
            }
            return None;
        }
    };

    Some(key)
}

fn parse_single_char_key(ch: char) -> Option<Key> {
    let key = match ch {
        'a' => Key::KeyA,
        'b' => Key::KeyB,
        'c' => Key::KeyC,
        'd' => Key::KeyD,
        'e' => Key::KeyE,
        'f' => Key::KeyF,
        'g' => Key::KeyG,
        'h' => Key::KeyH,
        'i' => Key::KeyI,
        'j' => Key::KeyJ,
        'k' => Key::KeyK,
        'l' => Key::KeyL,
        'm' => Key::KeyM,
        'n' => Key::KeyN,
        'o' => Key::KeyO,
        'p' => Key::KeyP,
        'q' => Key::KeyQ,
        'r' => Key::KeyR,
        's' => Key::KeyS,
        't' => Key::KeyT,
        'u' => Key::KeyU,
        'v' => Key::KeyV,
        'w' => Key::KeyW,
        'x' => Key::KeyX,
        'y' => Key::KeyY,
        'z' => Key::KeyZ,
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        '9' => Key::Num9,
        _ => return None,
    };

    Some(key)
}
