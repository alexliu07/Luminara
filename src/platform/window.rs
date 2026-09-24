//! 窗口能力:全屏切换。
//! windows-reactor 不暴露 HWND/窗口样式,沿用 dialog.rs 的 GetActiveWindow 取窗。

use std::cell::RefCell;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, GetWindowPlacement, SWP_FRAMECHANGED, SWP_NOZORDER,
    SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT, WS_CAPTION,
    WS_THICKFRAME,
};

// 进入全屏前保存的窗口样式与位置(仅 UI 线程访问)。
thread_local! {
    static SAVED: RefCell<Option<(isize, WINDOWPLACEMENT)>> = const { RefCell::new(None) };
}

/// 切换全屏:进入时去标题栏/边框并铺满当前显示器,退出原样还原。返回切换后是否全屏。
pub fn toggle_fullscreen() -> bool {
    unsafe {
        let hwnd = GetActiveWindow();
        if hwnd.is_invalid() {
            return false;
        }
        if let Some((style, placement)) = SAVED.with(|saved| saved.borrow_mut().take()) {
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, style);
            let _ = SetWindowPlacement(hwnd, &placement);
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        // Win32 结构体不带 Default,须显式清零并自填 length/cbSize,否则 API 校验直接拒。
        let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut placement).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let rect = info.rcMonitor;
        let decorated = WS_CAPTION.0 as isize | WS_THICKFRAME.0 as isize;
        let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, style & !decorated);
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_FRAMECHANGED,
        );
        SAVED.with(|saved| *saved.borrow_mut() = Some((style, placement)));
        true
    }
}
