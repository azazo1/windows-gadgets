use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, WM_INPUTLANGCHANGEREQUEST,
};

pub fn switch_im() {
    if get_input_method() == Some(1033) {
        switch_input_method(2052);
    } else {
        switch_input_method(1033);
    }
}

pub fn get_input_method() -> Option<u16> {
    let hwnd = foreground_window()?;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, None) };
    if thread_id == 0 {
        return None;
    }

    let current_layout = unsafe { GetKeyboardLayout(thread_id) };
    let locale = (current_layout.0 as usize & 0x0000_FFFF) as u16;
    tracing::debug!(target: "fncaps::ime", %locale, "current keyboard layout");
    Some(locale)
}

pub fn switch_input_method(locale: u32) {
    let Some(hwnd) = foreground_window() else {
        tracing::debug!(target: "fncaps::ime", "no foreground window, skip switch_input_method");
        return;
    };

    unsafe {
        let _ = PostMessageW(
            hwnd,
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(locale as isize),
        );
    }
    tracing::info!(target: "fncaps::ime", %locale, "posted WM_INPUTLANGCHANGEREQUEST");
}

fn foreground_window() -> Option<windows::Win32::Foundation::HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() { None } else { Some(hwnd) }
}
