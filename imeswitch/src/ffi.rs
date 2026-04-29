#![cfg(target_os = "windows")]

type Hwnd = isize;
type Lresult = isize;
type Hmonitor = isize;

const WM_INPUTLANGCHANGEREQUEST: u32 = 0x0050;
const WM_IME_CONTROL: u32 = 0x0283;
const KEYEVENTF_KEYUP: u32 = 0x0002;
const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
const MONITOR_DEFAULTTONEAREST: u32 = 2;
const FULLSCREEN_TOLERANCE: i32 = 1;

const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_SETCONVERSIONMODE: usize = 0x0002;
const VK_LMENU: u8 = 0xA4;
const VK_RMENU: u8 = 0xA5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: Rect,
    rc_work: Rect,
    dw_flags: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
    fn GetKeyboardLayout(thread_id: u32) -> isize;
    fn GetMonitorInfoW(monitor: Hmonitor, monitor_info: *mut MonitorInfo) -> i32;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> Hmonitor;
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> Lresult;
    fn keybd_event(bvk: u8, bscan: u8, dwflags: u32, dwextrainfo: usize);
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(
        hwnd: Hwnd,
        dwattribute: u32,
        pvattribute: *mut Rect,
        cbattribute: u32,
    ) -> i32;
}

#[link(name = "imm32")]
unsafe extern "system" {
    fn ImmGetDefaultIMEWnd(hwnd: Hwnd) -> Hwnd;
}

/// 获取当前前台窗口句柄。
///
/// 返回 `None` 表示没有可用前台窗口。
pub fn foreground_window() -> Option<Hwnd> {
    let hwnd = unsafe { GetForegroundWindow() };
    (hwnd != 0).then_some(hwnd)
}

fn roughly_matches(a: i32, b: i32) -> bool {
    (a - b).abs() <= FULLSCREEN_TOLERANCE
}

/// 判断前台窗口是否基本覆盖整块显示器。
///
/// 这里使用窗口可见边界与显示器边界做比较，主要用于识别游戏或全屏应用，
/// 以便让左右 `Alt` 直接透传给前台程序。
pub fn foreground_window_is_fullscreen() -> bool {
    let Some(hwnd) = foreground_window() else {
        return false;
    };

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor == 0 {
        return false;
    }

    let mut monitor_info = MonitorInfo {
        cb_size: std::mem::size_of::<MonitorInfo>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut monitor_info) } == 0 {
        return false;
    }

    let mut frame = Rect::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame,
            std::mem::size_of::<Rect>() as u32,
        )
    } != 0
    {
        return false;
    }

    roughly_matches(frame.left, monitor_info.rc_monitor.left)
        && roughly_matches(frame.top, monitor_info.rc_monitor.top)
        && roughly_matches(frame.right, monitor_info.rc_monitor.right)
        && roughly_matches(frame.bottom, monitor_info.rc_monitor.bottom)
}

/// 获取前台窗口所属线程的输入法布局 ID（HKL 低 16 位）。
///
/// 常见值：`1033`（英文）、`2052`（简体中文）。
/// 返回 `None` 表示无法获取前台窗口或线程信息。
pub fn current_layout_id() -> Option<u16> {
    let hwnd = foreground_window()?;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if thread_id == 0 {
        return None;
    }

    let hkl = unsafe { GetKeyboardLayout(thread_id) as usize };
    Some((hkl & 0xFFFF) as u16)
}

/// 获取当前前台窗口 IME 的转换模式（`IMC_GETCONVERSIONMODE`）。
///
/// 对微软拼音常见语义：
/// - `bit0 == 0`：英文
/// - `bit0 == 1`：中文
///
/// 返回 `None` 表示无法拿到 IME 默认窗口。
pub fn get_input_mode() -> Option<isize> {
    let hwnd = foreground_window()?;
    let ime_hwnd = unsafe { ImmGetDefaultIMEWnd(hwnd) };
    if ime_hwnd == 0 {
        return None;
    }

    let mode = unsafe { SendMessageW(ime_hwnd, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0) };
    Some(mode)
}

/// 设置前台窗口 IME 转换模式（`IMC_SETCONVERSIONMODE`）。
///
/// 例如 `mode = 1` 可请求切到中文模式。
/// 返回 `false` 表示参数非法或无法定位目标 IME 窗口。
pub fn switch_input_mode(mode: isize) -> bool {
    if mode < 0 {
        return false;
    }

    let Some(hwnd) = foreground_window() else {
        return false;
    };
    let ime_hwnd = unsafe { ImmGetDefaultIMEWnd(hwnd) };
    if ime_hwnd == 0 {
        return false;
    }

    unsafe {
        SendMessageW(ime_hwnd, WM_IME_CONTROL, IMC_SETCONVERSIONMODE, mode);
    }
    true
}

/// 向前台窗口发送输入法切换请求（`WM_INPUTLANGCHANGEREQUEST`）。
///
/// `locale` 使用语言区域标识，例如 `1033`（英文）、`2052`（简体中文）。
/// 返回 `true` 仅表示消息投递成功，不保证目标应用一定完成切换。
pub fn switch_input_method(locale: u32) -> bool {
    let Some(hwnd) = foreground_window() else {
        return false;
    };

    let ok = unsafe { PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, locale as isize) };

    ok != 0
}

/// 向系统注入左/右 Alt 键事件，用于把被拦截的 Alt 组合键还给前台应用。
pub fn emit_alt_key(is_left: bool, key_up: bool) {
    let vk = if is_left { VK_LMENU } else { VK_RMENU };
    let flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
    unsafe {
        keybd_event(vk, 0, flags, 0);
    }
}
