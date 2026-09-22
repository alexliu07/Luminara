/// 媒体作品。纯数据层,不依赖 UI。
#[derive(Clone, Debug, PartialEq)]
pub struct MediaItem {
    pub id: u64,
    pub title: String,
    pub creator: String,
    /// 占位封面底色 (r, g, b);后续阶段替换为真实封面图路径。
    pub cover_rgb: (u8, u8, u8),
}

impl MediaItem {
    fn new(id: u64, title: &str, creator: &str, cover_rgb: (u8, u8, u8)) -> Self {
        Self {
            id,
            title: title.to_string(),
            creator: creator.to_string(),
            cover_rgb,
        }
    }
}

/// 示例媒体库(后续阶段替换为真实扫描/数据库加载)。
pub fn sample_library() -> Vec<MediaItem> {
    vec![
        MediaItem::new(1, "星夜巡光", "Luminara Ensemble", (54, 62, 128)),
        MediaItem::new(2, "雨声收藏家", "Mori & The Drops", (38, 96, 110)),
        MediaItem::new(3, "北纬四十度", "Paper Compass", (122, 74, 42)),
        MediaItem::new(4, "玻璃温室", "Hana Glasshouse", (58, 108, 82)),
        MediaItem::new(5, "午夜列车", "Nightline", (74, 52, 110)),
        MediaItem::new(6, "纸上群青", "Ultramarine Notes", (40, 74, 140)),
        MediaItem::new(7, "回声剧场", "Echo Theater", (128, 58, 70)),
        MediaItem::new(8, "风的形状", "Aero Sketch", (66, 96, 96)),
    ]
}
