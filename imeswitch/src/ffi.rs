#![cfg(target_os = "windows")]

type Hwnd = isize;
type Lresult = isize;

const WM_INPUTLANGCHANGEREQUEST: u32 = 0x0050;
const WM_IME_CONTROL: u32 = 0x0283;
const KEYEVENTF_KEYUP: u32 = 0x0002;

const IMC_GETCONVERSIONMODE: usize = 0x0001;
const IMC_SETCONVERSIONMODE: usize = 0x0002;
const VK_LMENU: u8 = 0xA4;
const VK_RMENU: u8 = 0xA5;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
    fn GetKeyboardLayout(thread_id: u32) -> isize;
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> Lresult;
    fn keybd_event(bvk: u8, bscan: u8, dwflags: u32, dwextrainfo: usize);
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
