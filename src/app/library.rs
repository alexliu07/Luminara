use std::rc::Rc;
use windows_reactor::*;

use crate::app::card::{media_card, Cover};
use crate::model::library::MediaLibrary;

#[derive(Clone, PartialEq)]
pub struct LibraryInput {
    pub libraries: Rc<Vec<MediaLibrary>>,
    /// 菜单项点击(回传 label),由 Root 创建并处理。
    pub on_menu: Callback<String>,
}

/// 媒体库页:已添加的本地文件夹卡片 + 「添加媒体库」菜单。
pub struct Library;

impl Component for Library {
    type Input = LibraryInput;
    // 页面保持无状态(与 Home/Placeholder 同风格),增删逻辑在 Root。
    type Message = ();

    fn create(_input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Self::Input, _context: &mut ViewContext<Self>) -> View {
        let title_row = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                TextBlock::new()
                    .text("媒体库")
                    .font_size(28.0)
                    .font_weight(FontWeight::BOLD)
                    .foreground(ThemeBrush::PrimaryText),
                Button::new().content("添加媒体库").menu(Menu::new(
                    [MenuItem::submenu(
                        "select-source",
                        "选择来源",
                        [MenuItem::item("from-local", "从本地添加")],
                    )],
                    input.on_menu.clone(),
                )),
            ));

        let grid = GridView::new().collection_slot(
            GridViewSlot::Items,
            input.libraries.iter().map(|lib| {
                KeyedView::new(
                    lib.id,
                    media_card(&lib.name, &lib.path, Cover::folder(lib.cover_rgb)),
                )
            }),
        );

        Border::new()
            .padding(Thickness::uniform(24.0))
            .content(if input.libraries.is_empty() {
                StackPanel::new().spacing(16.0).children((
                    title_row,
                    TextBlock::new()
                        .text("还没有添加媒体库,点击「添加媒体库」开始。")
                        .font_size(13.0)
                        .foreground(Color::rgb(150, 150, 150)),
                    grid,
                ))
            } else {
                StackPanel::new()
                    .spacing(16.0)
                    .children((title_row, grid))
            })
    }
}
