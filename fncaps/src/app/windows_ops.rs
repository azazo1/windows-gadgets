use std::f64::consts::PI;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, IsZoomed, SW_SHOW, SetCursorPos,
    SetForegroundWindow, ShowWindow, WindowFromPoint,
};

use super::state::Direction;

const MIN_VALID_WINDOW_WIDTH: i32 = 30;
const MIN_VALID_WINDOW_HEIGHT: i32 = 30;

#[derive(Debug, Clone, Copy)]
struct ScreenBounds {
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    total_width: i32,
    total_height: i32,
    diagonal_square: f64,
}

#[derive(Clone, Copy)]
struct WindowInfo {
    hwnd: HWND,
    rect: RECT,
}

pub fn switch_to(direction: Direction) {
    let Some(bounds) = update_screen_size() else {
        tracing::warn!(target: "fncaps::windows", "cannot get monitor bounds");
        return;
    };

    let windows = enumerate_valid_windows(bounds);
    tracing::debug!(target: "fncaps::windows", count = windows.len(), ?direction, "valid windows collected");

    if windows.is_empty() {
        return;
    }

    let focused = match select_focused(&windows) {
        Some(w) => w,
        None => {
            tracing::debug!(target: "fncaps::windows", "no focused window in candidates, focusing first");
            focus_on_window(windows[0]);
            return;
        }
    };

    let focused_center = rect_center(focused.rect);
    let mut selected: Option<WindowInfo> = None;
    let mut min_weight = f64::INFINITY;

    for window in windows {
        if window.hwnd == focused.hwnd {
            continue;
        }

        let center = rect_center(window.rect);
        let dy = center.1 - focused_center.1;
        let dx = center.0 - focused_center.0;
        let mut angle = dy.atan2(dx) * 180.0 / PI;

        if angle > 180.0 {
            angle -= 360.0;
        }

        let angle_diff_ratio = if direction == Direction::Left {
            (direction.angle() - angle.abs()).abs() / 180.0
        } else {
            (direction.angle() - angle).abs() / 180.0
        };

        let distance_square =
            (focused_center.0 - center.0).powi(2) + (focused_center.1 - center.1).powi(2);
        let distance_square_ratio = if bounds.diagonal_square > 0.0 {
            distance_square / bounds.diagonal_square
        } else {
            1.0
        };

        let weight = distance_square_ratio * 0.6 + angle_diff_ratio * 0.4;
        if weight < min_weight {
            min_weight = weight;
            selected = Some(window);
        }
    }

    if let Some(window) = selected {
        tracing::info!(target: "fncaps::windows", ?direction, weight = min_weight, "switching focus to selected window");
        focus_on_window(window);
    }
}

fn update_screen_size() -> Option<ScreenBounds> {
    #[derive(Clone, Copy)]
    struct MonitorCollect {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        initialized: bool,
    }

    unsafe extern "system" fn monitor_enum_proc(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let collect = lparam.0 as *mut MonitorCollect;
        if collect.is_null() {
            return BOOL(0);
        }
        let collect = unsafe { &mut *collect };

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if unsafe { GetMonitorInfoW(monitor, &mut info as *mut MONITORINFO) }.as_bool() {
            let rect = info.rcMonitor;
            if !collect.initialized {
                collect.start_x = rect.left;
                collect.start_y = rect.top;
                collect.end_x = rect.right;
                collect.end_y = rect.bottom;
                collect.initialized = true;
            } else {
                collect.start_x = collect.start_x.min(rect.left);
                collect.start_y = collect.start_y.min(rect.top);
                collect.end_x = collect.end_x.max(rect.right);
                collect.end_y = collect.end_y.max(rect.bottom);
            }
        }

        BOOL(1)
    }

    let mut collect = MonitorCollect {
        start_x: 0,
        start_y: 0,
        end_x: 0,
        end_y: 0,
        initialized: false,
    };

    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(monitor_enum_proc),
            LPARAM((&mut collect as *mut MonitorCollect) as isize),
        );
    }

    if !collect.initialized {
        return None;
    }

    let total_width = collect.end_x - collect.start_x;
    let total_height = collect.end_y - collect.start_y;
    let diagonal_square = (total_width as f64).powi(2) + (total_height as f64).powi(2);

    Some(ScreenBounds {
        start_x: collect.start_x,
        start_y: collect.start_y,
        end_x: collect.end_x,
        end_y: collect.end_y,
        total_width,
        total_height,
        diagonal_square,
    })
}

fn get_window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    let ok = unsafe { GetWindowRect(hwnd, &mut rect) };
    ok.ok()?;
    Some(rect)
}

fn rect_width(rect: RECT) -> i32 {
    rect.right - rect.left
}

fn rect_height(rect: RECT) -> i32 {
    rect.bottom - rect.top
}

fn rect_center(rect: RECT) -> (f64, f64) {
    (
        rect.left as f64 + rect_width(rect) as f64 / 2.0,
        rect.top as f64 + rect_height(rect) as f64 / 2.0,
    )
}

fn get_title(hwnd: HWND) -> Option<String> {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return None;
    }

    let mut buffer = vec![0u16; (len as usize) + 1];
    let read = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if read <= 0 {
        return None;
    }

    let text = OsString::from_wide(&buffer[..read as usize])
        .to_string_lossy()
        .trim()
        .to_string();

    if text.is_empty() { None } else { Some(text) }
}

fn is_window_maximized(hwnd: HWND) -> bool {
    unsafe { IsZoomed(hwnd).as_bool() }
}

fn top_root_from_point(point: POINT) -> Option<HWND> {
    let top = unsafe { WindowFromPoint(point) };
    if top.0.is_null() {
        return None;
    }
    let root = unsafe { GetAncestor(top, GA_ROOT) };
    if root.0.is_null() { None } else { Some(root) }
}

fn is_valid_window(hwnd: HWND, bounds: ScreenBounds, current_pid: u32) -> Option<WindowInfo> {
    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
        return None;
    }

    let title = get_title(hwnd)?;

    let mut pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == current_pid {
        return None;
    }

    let rect = get_window_rect(hwnd)?;
    let center = rect_center(rect);
    if center.0 < bounds.start_x as f64 || center.0 > bounds.end_x as f64 {
        return None;
    }
    if center.1 < bounds.start_y as f64 || center.1 > bounds.end_y as f64 {
        return None;
    }

    let center_point = POINT {
        x: center.0 as i32,
        y: center.1 as i32,
    };
    let top_root = top_root_from_point(center_point)?;
    let window_root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if top_root != window_root {
        return None;
    }

    let width = rect_width(rect);
    let height = rect_height(rect);

    let partly_onscreen = rect.right >= bounds.start_x
        && rect.bottom >= bounds.start_y
        && rect.left <= bounds.end_x
        && rect.top <= bounds.end_y
        && width >= MIN_VALID_WINDOW_WIDTH
        && width <= bounds.total_width
        && height >= MIN_VALID_WINDOW_HEIGHT
        && height <= bounds.total_height;

    if !(is_window_maximized(hwnd) || partly_onscreen) {
        return None;
    }

    tracing::trace!(target: "fncaps::windows", title = %title, pid, "valid window accepted");
    Some(WindowInfo { hwnd, rect })
}

fn enumerate_valid_windows(bounds: ScreenBounds) -> Vec<WindowInfo> {
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = lparam.0 as *mut Vec<HWND>;
        if windows.is_null() {
            return BOOL(0);
        }
        let windows = unsafe { &mut *windows };
        windows.push(hwnd);
        BOOL(1)
    }

    let mut all = Vec::<HWND>::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM((&mut all as *mut Vec<HWND>) as isize),
        );
    }

    let current_pid = std::process::id();
    all.into_iter()
        .filter_map(|hwnd| is_valid_window(hwnd, bounds, current_pid))
        .collect()
}

fn select_focused(windows: &[WindowInfo]) -> Option<WindowInfo> {
    let focused = unsafe { GetForegroundWindow() };
    windows.iter().copied().find(|w| w.hwnd == focused)
}

fn focus_on_window(window: WindowInfo) {
    unsafe {
        let _ = ShowWindow(window.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(window.hwnd);
    }

    let (x, y) = rect_center(window.rect);
    unsafe {
        let _ = SetCursorPos(x as i32, y as i32);
    }
}

/// 切换到指定窗口标题的窗口
pub fn switch_to_window(title: &str) {
    tracing::info!(target: "fncaps::windows", target_title = title, "attempting to switch to window");

    let Some(bounds) = update_screen_size() else {
        tracing::warn!(target: "fncaps::windows", "cannot get monitor bounds");
        return;
    };

    let windows = enumerate_valid_windows(bounds);
    tracing::debug!(target: "fncaps::windows", count = windows.len(), "valid windows collected");

    // 精确匹配
    if let Some(window) = windows.iter().find(|w| {
        if let Some(wnd_title) = get_title(w.hwnd) {
            wnd_title == title
        } else {
            false
        }
    }) {
        tracing::info!(target: "fncaps::windows", matched_title = title, "exact match found, switching");
        focus_on_window(*window);
        return;
    }

    // 模糊匹配（包含）
    if let Some(window) = windows.iter().find(|w| {
        if let Some(wnd_title) = get_title(w.hwnd) {
            wnd_title.contains(title)
        } else {
            false
        }
    }) {
        tracing::info!(target: "fncaps::windows", matched_title = title, "fuzzy match found, switching");
        focus_on_window(*window);
        return;
    }

    tracing::warn!(target: "fncaps::windows", target_title = title, "no window found with matching title");
}

/// 切换到指定窗口，或如果窗口不存在则打开程序
pub fn switch_to_window_or_open(window_title: &str, program: &str) {
    tracing::info!(
        target: "fncaps::windows",
        window_title,
        program,
        "attempting switch-or-open"
    );

    let Some(bounds) = update_screen_size() else {
        tracing::warn!(target: "fncaps::windows", "cannot get monitor bounds, fallback to opening program");
        super::launch::open_program(program);
        return;
    };

    let windows = enumerate_valid_windows(bounds);
    tracing::debug!(target: "fncaps::windows", count = windows.len(), "valid windows collected");

    // 精确匹配
    if let Some(window) = windows.iter().find(|w| {
        if let Some(wnd_title) = get_title(w.hwnd) {
            wnd_title.eq_ignore_ascii_case(window_title)
        } else {
            false
        }
    }) {
        tracing::info!(target: "fncaps::windows", matched_title = window_title, "exact match found, switching");
        focus_on_window(*window);
        return;
    }

    // 模糊匹配（包含）
    if let Some(window) = windows.iter().find(|w| {
        if let Some(wnd_title) = get_title(w.hwnd) {
            wnd_title
                .to_lowercase()
                .contains(&window_title.to_lowercase())
        } else {
            false
        }
    }) {
        tracing::info!(target: "fncaps::windows", matched_title = window_title, "fuzzy match found, switching");
        focus_on_window(*window);
        return;
    }

    tracing::warn!(target: "fncaps::windows", window_title, "window not found, launching program");
    super::launch::open_program(program);
}
