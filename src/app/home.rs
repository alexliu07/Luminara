use std::rc::Rc;
use windows_reactor::*;

use crate::app::card::{media_card, Cover};
use crate::model::media::MediaItem;

/// 首页输入:作品列表 + 是否正在后台扫描索引。
#[derive(Clone, PartialEq)]
pub struct HomeInput {
    pub items: Rc<Vec<MediaItem>>,
    /// 后台索引扫描进行中;为 true 时标题行右侧显示加载转圈。
    pub scanning: bool,
}

/// 首页:作品封面墙。
pub struct Home;

impl Component for Home {
    type Input = HomeInput;
    // TODO(阶段 2):点击卡片发 Play(id)。
    type Message = ();

    fn create(_input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Self::Input, _context: &mut ViewContext<Self>) -> View {
        let cards = input
            .items
            .iter()
            .map(|item| KeyedView::new(item.id, cover_card(item)));

        let title = TextBlock::new()
            .text("全部作品")
            .font_size(28.0)
            .font_weight(FontWeight::BOLD)
            .foreground(ThemeBrush::PrimaryText);

        // 标题行:标题居左;后台扫描时右上角显示加载转圈。
        let title_row = if input.scanning {
            Grid::new()
                .columns([GridLength::STAR, GridLength::Auto])
                .children((
                    title,
                    ProgressRing::new()
                        .is_active(true)
                        .is_indeterminate(true)
                        .width(20.0)
                        .height(20.0)
                        .grid_column(1)
                        .vertical_alignment(VerticalAlignment::Center),
                ))
        } else {
            Grid::new().columns([GridLength::STAR]).children((title,))
        };

        Border::new()
            .padding(Thickness::uniform(24.0))
            .content(
                StackPanel::new().spacing(16.0).children((
                    title_row,
                    GridView::new().collection_slot(GridViewSlot::Items, cards),
                )),
            )
    }
}

/// 单个作品卡片:封面 + 标题 + 作者。本期纯色占位;二期按 cover_path 分支 Cover::image。
fn cover_card(item: &MediaItem) -> View {
    media_card(&item.title, &item.creator, Cover::solid(item.cover_rgb))
}
