//! 平台相关能力:windows-reactor 未提供的 Win32/WinRT 交互。

pub mod dialog;
pub mod player;
pub mod video;
pub mod window;

pub use dialog::pick_folder;
pub use player::{PlayerCore, PlayerSnapshot};
pub use window::toggle_fullscreen;

/// windows 0.62 的 IUnknown → 框架 windows-core 0.100 的 IUnknown。
///
/// 踩坑:框架依赖 windows-core 0.100,`windows` crate 只有 0.62 系,同名类型不互通;
/// 两边 IUnknown 都是 repr(transparent) 单指针,只能 clone(AddRef)后转裸指针换皮,
/// 所有权连同那一次引用一起交给框架侧 Release。
pub fn to_framework_unknown(unknown: &windows::core::IUnknown) -> wr_core::IUnknown {
    let owned = unknown.clone(); // +1 引用,转移给框架
    let raw = windows::core::Interface::into_raw(owned);
    unsafe { wr_core::Interface::from_raw(raw) }
}
