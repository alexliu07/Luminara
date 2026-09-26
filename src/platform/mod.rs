//! 平台相关能力:windows-reactor 未提供的 Win32/WinRT 交互。
//! 字幕引擎(subtitle)是纯 Rust,不绑平台;其余模块仅 Windows。

#[cfg(windows)]
pub mod dialog;
#[cfg(windows)]
pub mod player;
#[cfg(windows)]
pub mod video;
#[cfg(windows)]
pub mod window;

#[cfg(windows)]
pub use dialog::pick_folder;
#[cfg(windows)]
pub use player::PlayerCore;
#[cfg(windows)]
pub use window::toggle_fullscreen;

/// windows 0.62 的 IUnknown → 框架 windows-core 0.100 的 IUnknown。
///
/// 踩坑:框架依赖 windows-core 0.100,`windows` crate 只有 0.62 系,同名类型不互通;
/// 两边 IUnknown 都是 repr(transparent) 单指针,只能 clone(AddRef)后转裸指针换皮,
/// 所有权连同那一次引用一起交给框架侧 Release。
#[cfg(windows)]
pub fn to_framework_unknown(unknown: &windows::core::IUnknown) -> wr_core::IUnknown {
    let owned = unknown.clone(); // +1 引用,转移给框架
    let raw = windows::core::Interface::into_raw(owned);
    unsafe { wr_core::Interface::from_raw(raw) }
}
