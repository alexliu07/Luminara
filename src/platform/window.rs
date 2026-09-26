//! 窗口能力:全屏切换。
//! windows-reactor 不暴露 HWND/窗口样式,沿用 dialog.rs 的 GetActiveWindow 取窗。

use std::cell::{Cell, RefCell};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GWL_STYLE, GWLP_WNDPROC, GetWindowLongPtrW, GetWindowPlacement,
    SWP_FRAMECHANGED, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos,
    WINDOWPLACEMENT, WM_KEYDOWN, WM_SYSKEYDOWN, WNDPROC, WS_CAPTION, WS_THICKFRAME,
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

/// 当前是否全屏(SAVED 存在即全屏)。
pub fn is_fullscreen() -> bool {
    SAVED.with(|saved| saved.borrow().is_some())
}

// ---------------------------------------------------------------------------
// 播放页快捷键:子类化主窗口 WndProc,拦截按键回投给 UI。
//
// 踩坑:windows-reactor 不暴露键盘事件(内部有 KeyboardAccelerator 但没公开),
// 只能走 Win32 子类化;回调跑在 UI 线程(消息循环线程),可以直接打框架回调。
// ---------------------------------------------------------------------------

/// 快捷键动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// 空格:播放/暂停。
    TogglePause,
    /// Esc:退出全屏或返回。
    Escape,
    /// 左:快退 10s。
    SeekBack,
    /// 右:快进 10s。
    SeekForward,
}

thread_local! {
    static KEY_SINK: RefCell<Option<Box<dyn Fn(KeyAction)>>> = const { RefCell::new(None) };
    static KEY_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ORIG_PROC: Cell<WNDPROC> = const { Cell::new(None) };
}

/// 安装按键钩子(幂等;须在 UI 线程调用,窗口已创建后才有 HWND)。
pub fn install_key_hook(sink: Box<dyn Fn(KeyAction)>) {
    unsafe {
        let hwnd = GetActiveWindow();
        if hwnd.is_invalid() {
            return;
        }
        KEY_SINK.with(|slot| *slot.borrow_mut() = Some(sink));
        if ORIG_PROC.with(|proc| proc.get()).is_some() {
            return;
        }
        let orig: WNDPROC = std::mem::transmute(GetWindowLongPtrW(hwnd, GWLP_WNDPROC));
        ORIG_PROC.with(|proc| proc.set(orig));
        let _ = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hook_proc as usize as isize);
    }
}

/// 只在播放页期间拦截按键(防空格滚屏之类误伤常规页面)。
pub fn set_key_active(active: bool) {
    KEY_ACTIVE.with(|slot| slot.set(active));
}

unsafe extern "system" fn hook_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && KEY_ACTIVE.with(|slot| slot.get()) {
        let action = match wparam.0 as u32 {
            0x20 => Some(KeyAction::TogglePause),
            0x1B => Some(KeyAction::Escape),
            0x25 => Some(KeyAction::SeekBack),
            0x27 => Some(KeyAction::SeekForward),
            _ => None,
        };
        if let Some(action) = action {
            let handled = KEY_SINK.with(|slot| {
                if let Some(sink) = slot.borrow().as_ref() {
                    sink(action);
                    true
                } else {
                    false
                }
            });
            if handled {
                return LRESULT(0);
            }
        }
    }
    match ORIG_PROC.with(|proc| proc.get()) {
        Some(orig) => CallWindowProcW(Some(orig), hwnd, msg, wparam, lparam),
        None => LRESULT(0),
    }
}
