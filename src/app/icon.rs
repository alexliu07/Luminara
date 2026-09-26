//! 手绘 SVG 图标:全部为手写路径数据(24×24 网格),不用 emoji、不用图标字体、不引图标库。
//!
//! 踩坑:WinUI 的 PathIcon 只接受填充几何,没有描边;为保持描边观感,
//! 每个笔画都手工展开成「描边带」形状(即把 2px 宽的线写成闭合多边形),
//! 圆环用外圆顺时针 + 内圆逆时针的双子路径(非零环绕数挖空,不依赖 fill-rule)。

use windows_reactor::*;

/// 图标集。手绘路径,24×24 网格,视觉重量≈2px 描边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// 返回(←)。
    Back,
    /// 播放(▶ 实心三角)。
    Play,
    /// 暂停(两竖条)。
    Pause,
    /// 上一个(⏮ 竖条+左三角)。
    Prev,
    /// 下一个(⏭ 右三角+竖条)。
    Next,
    /// 后退 10s(环形箭头向左)。
    SeekBack,
    /// 前进 10s(环形箭头向右)。
    SeekForward,
    /// 全屏(四角括号)。
    Fullscreen,
    /// 退出全屏(四角内收)。
    FullscreenExit,
    /// 字幕(框+两行)。
    Subtitles,
    /// 首页(房子)。
    Home,
    /// 媒体库(叠放卡片)。
    Library,
    /// 播放列表(行+点)。
    Playlists,
    /// 设置(齿轮)。
    Settings,
}

/// 图标路径数据(SVG path 语法,XAML 几何迷你语言同构)。
pub fn path_data(icon: Icon) -> &'static str {
    match icon {
        // ←:横带 + 折角箭头带
        Icon::Back => "M18 11 H8 V13 H18 Z M12 6 L14 8 L10 12 L14 16 L12 18 L6 12 Z",
        Icon::Play => "M8.5 5.8 L18.6 12 L8.5 18.2 Z",
        Icon::Pause => "M8 5.5 H11 V18.5 H8 Z M13 5.5 H16 V18.5 H13 Z",
        Icon::Prev => "M6.5 5.5 H9 V18.5 H6.5 Z M19 6.5 L11.5 12 L19 17.5 Z",
        Icon::Next => "M17.5 5.5 H15 V18.5 H17.5 Z M5 6.5 L12.5 12 L5 17.5 Z",
        // 环带(外圆逆 + 内圆顺挖空)+ 端头箭头三角
        Icon::SeekBack => {
            "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4 Z M12 6 A6 6 0 1 1 12 18 A6 6 0 1 1 12 6 Z \
             M6.6 2.6 L10.8 5.4 L6.6 8.2 Z"
        }
        Icon::SeekForward => {
            "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4 Z M12 6 A6 6 0 1 1 12 18 A6 6 0 1 1 12 6 Z \
             M17.4 2.6 L13.2 5.4 L17.4 8.2 Z"
        }
        // 四角括号
        Icon::Fullscreen => {
            "M4 9 V4 H9 V6 H6 V9 Z M15 4 H20 V9 H18 V6 H15 Z \
             M4 15 V20 H9 V18 H6 V15 Z M20 15 V20 H15 V18 H18 V15 Z"
        }
        // 四角内收
        Icon::FullscreenExit => {
            "M9 9 V4 H11 V7 H14 V9 Z M15 9 V7 H18 V4 H20 V9 Z \
             M9 15 V20 H11 V17 H14 V15 Z M15 15 V17 H18 V20 H20 V15 Z"
        }
        // 字幕:圆角框带 + 两行字带
        Icon::Subtitles => {
            "M3 5 H21 V19 H3 Z M5 7 V17 H19 V7 Z \
             M6.5 10.5 H13 V12.5 H6.5 Z M6.5 14 H17.5 V16 H6.5 Z"
        }
        // 房子:屋顶带 + 两墙 + 底边 + 门
        Icon::Home => {
            "M12 3.5 L21 11 L19.5 12.8 L12 6.4 L4.5 12.8 L3 11 Z \
             M6 12 H8 V20 H6 Z M16 12 H18 V20 H16 Z M6 18 H18 V20 H6 Z \
             M10.5 15 H13.5 V20 H10.5 Z"
        }
        // 叠放卡片:后卡边带 + 前卡框带
        Icon::Library => {
            "M8 4 H20 V16 H18 V6 H8 Z \
             M4 7 H18 V19 H4 Z M6 9 V17 H16 V9 Z"
        }
        // 列表:三点三行
        Icon::Playlists => {
            "M4 6 H6.5 V8 H4 Z M4 11 H6.5 V13 H4 Z M4 16 H6.5 V18 H4 Z \
             M9 6 H20 V8 H9 Z M9 11 H20 V13 H9 Z M9 16 H20 V18 H9 Z"
        }
        // 齿轮:环带 + 四辐条
        Icon::Settings => {
            "M12 5 A7 7 0 1 0 12 19 A7 7 0 1 0 12 5 Z M12 7.2 A4.8 4.8 0 1 1 12 16.8 A4.8 4.8 0 1 1 12 7.2 Z \
             M11 2.5 H13 V6 H11 Z M11 18 H13 V21.5 H11 Z M2.5 11 H6 V13 H2.5 Z M18 11 H21.5 V13 H18 Z"
        }
    }
}

/// 造一个手绘图标(PathIcon,颜色随按钮/主题)。
pub fn path_icon(icon: Icon) -> PathIcon {
    PathIcon::new().data(path_data(icon))
}
