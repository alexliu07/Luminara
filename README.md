# Luminara

Windows 媒体库 / 本地播放器（Rust + WinUI3，UI 框架 windows-reactor）。

## 构建

- 工具链：`stable-x86_64-pc-windows-msvc`（VS Build Tools + Windows SDK）
- 准备 libmpv 导入库（一次性）：
  1. 取 mpv Windows 64 位构建包里的 `mpv-2.dll`（mpv ≥ 0.35）；若包内无 `mpv.def`，从 dll 导出生成：
     ```powershell
     $names = (dumpbin /exports mpv-2.dll) | Select-String '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }
     @("EXPORTS") + $names | Set-Content mpv.def
     ```
  2. 在 "x64 Native Tools Command Prompt" 里生成导入库：
     ```
     lib /def:mpv.def /out:mpv.lib /machine:x64
     ```
- 设环境变量后构建（build.rs 自动加链接搜索路径）：
  ```
  set MPV_LIB_DIR=C:\path\to\mpv-lib-dir
  cargo build
  ```

## 运行

Luminara.exe 同目录放三个 dll：

- `mpv-2.dll`（播放内核）
- `libEGL.dll`、`libGLESv2.dll`（ANGLE，视频渲染上下文）

## 许可

mpv 内核为 GPLv2+；对外分发包含 `mpv-2.dll` 的成品时需提供其源码获取方式。
