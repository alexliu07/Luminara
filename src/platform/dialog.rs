//! 系统对话框。

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};

/// 弹出系统「选择文件夹」对话框。返回所选文件夹完整路径;取消返回 None。
/// 必须在 UI 线程调用(WinUI3 进程已以 STA 初始化 COM,无需再 CoInitialize)。
pub fn pick_folder() -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;
        dialog
            .SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
            .ok()?;
        // windows-reactor 不暴露自身 HWND,用当前活动窗口作属主
        dialog.Show(Some(GetActiveWindow())).ok()?; // 用户取消 → Err → None
        let item = dialog.GetResult().ok()?;
        item.GetDisplayName(SIGDN_FILESYSPATH)
            .ok()?
            .to_string()
            .ok()
    }
}
