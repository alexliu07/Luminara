use crate::app::icon::Icon;

/// 侧边栏导航的页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Library,
    Playlists,
    Settings,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Home, Page::Library, Page::Playlists, Page::Settings];

    /// NavigationView 选中回调按 tag 路由。
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "home" => Some(Self::Home),
            "library" => Some(Self::Library),
            "playlists" => Some(Self::Playlists),
            "settings" => Some(Self::Settings),
            _ => None,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Library => "library",
            Self::Playlists => "playlists",
            Self::Settings => "settings",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "首页",
            Self::Library => "媒体库",
            Self::Playlists => "播放列表",
            Self::Settings => "设置",
        }
    }

    /// 手绘 SVG 图标(见 icon.rs;不用 emoji/图标字体)。
    pub fn icon(self) -> Icon {
        match self {
            Self::Home => Icon::Home,
            Self::Library => Icon::Library,
            Self::Playlists => Icon::Playlists,
            Self::Settings => Icon::Settings,
        }
    }
}
