use std::path::PathBuf;
use std::rc::Rc;
use windows_reactor::*;

use crate::app::card::{Cover, media_card_action};
use crate::model::format;

/// 浏览页的一个条目(文件或子目录)。
#[derive(Clone, Debug, PartialEq)]
pub struct BrowseEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// 浏览页输入:当前目录条目 + 导航回调。
#[derive(Clone, PartialEq)]
pub struct BrowseInput {
    /// 当前目录名(标题行显示)。
    pub dir_name: String,
    /// 当前目录完整路径(副标题显示)。
    pub dir_path: String,
    /// 当前是否在根目录(决定是否显示「返回上一级」)。
    pub at_root: bool,
    pub entries: Rc<Vec<BrowseEntry>>,
    /// 点击子目录卡片进入。
    pub on_enter: Callback<PathBuf>,
    /// 点击受支持的媒体文件卡片(进播放页);其余文件不回传。
    pub on_open: Callback<PathBuf>,
    /// 返回上一级;在根目录时退回媒体库列表。
    pub on_back: Callback<()>,
}

/// 文件夹浏览页:与媒体库同风格的 GridView 卡片网格。
pub struct BrowsePage;

impl Component for BrowsePage {
    type Input = BrowseInput;
    type Message = ();

    fn create(_input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Self::Input, _context: &mut ViewContext<Self>) -> View {
        let title = TextBlock::new()
            .text(input.dir_name.clone())
            .font_size(28.0)
            .font_weight(FontWeight::BOLD)
            .foreground(ThemeBrush::PrimaryText);

        // 标题行:根目录显示「返回媒体库」;子目录显示「返回上一级」。
        let back_label = if input.at_root {
            "返回媒体库"
        } else {
            "返回上一级"
        };
        let title_row = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                title,
                Button::new()
                    .on_click(input.on_back.clone())
                    .content(back_label),
            ));

        let path_hint = TextBlock::new()
            .text(input.dir_path.clone())
            .font_size(12.0)
            .foreground(Color::rgb(150, 150, 150))
            .max_lines(1)
            .text_trimming(TextTrimming::CharacterEllipsis);

        let grid = GridView::new().collection_slot(
            GridViewSlot::Items,
            input.entries.iter().enumerate().map(|(i, entry)| {
                let cover = if entry.is_dir {
                    Cover::folder(hash_rgb(&entry.path.to_string_lossy()))
                } else {
                    Cover::solid(hash_rgb(&entry.path.to_string_lossy()))
                };
                let subtitle = if entry.is_dir {
                    "文件夹".to_string()
                } else {
                    human_size(entry.size)
                };
                let on_click = if entry.is_dir {
                    let on_enter = input.on_enter.clone();
                    let path = entry.path.clone();
                    Callback::new(move |()| {
                        let _ = on_enter.call(path.clone());
                    })
                } else if format::is_supported(&entry.name) {
                    // 受支持媒体文件 → 进播放页;字幕等其余文件显示但点击不响应。
                    let on_open = input.on_open.clone();
                    let path = entry.path.clone();
                    Callback::new(move |()| {
                        let _ = on_open.call(path.clone());
                    })
                } else {
                    Callback::new(|()| {})
                };
                KeyedView::new(
                    i,
                    media_card_action(&entry.name, &subtitle, cover, on_click),
                )
            }),
        );

        Border::new()
            .padding(Thickness::uniform(24.0))
            .content(if input.entries.is_empty() {
                StackPanel::new().spacing(16.0).children((
                    title_row,
                    path_hint,
                    TextBlock::new()
                        .text("此文件夹为空。")
                        .font_size(13.0)
                        .foreground(Color::rgb(150, 150, 150)),
                ))
            } else {
                StackPanel::new()
                    .spacing(16.0)
                    .children((title_row, path_hint, grid))
            })
    }
}

/// 与 model/library.rs 相同的 FNV-1a 哈希,保证同一路径封面色稳定。
fn hash_rgb(path: &str) -> (u8, u8, u8) {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let channel = |shift: u32| 30 + ((hash >> shift) as u8 % 101);
    (channel(0), channel(8), channel(16))
}

/// 人类可读文件大小。
fn human_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
