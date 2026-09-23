use std::rc::Rc;
use windows_reactor::*;

use crate::app::card::{media_card, Cover};
use crate::model::media::MediaItem;

/// 首页:作品封面墙。
pub struct Home;

impl Component for Home {
    type Input = Rc<Vec<MediaItem>>;
    // TODO(阶段 2):点击卡片发 Play(id)。
    type Message = ();

    fn create(_input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Self::Input, _context: &mut ViewContext<Self>) -> View {
        let cards = input
            .iter()
            .map(|item| KeyedView::new(item.id, cover_card(item)));

        Border::new()
            .padding(Thickness::uniform(24.0))
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("全部作品")
                        .font_size(28.0)
                        .font_weight(FontWeight::BOLD)
                        .foreground(ThemeBrush::PrimaryText),
                    GridView::new().collection_slot(GridViewSlot::Items, cards),
                )),
            )
    }
}

/// 单个作品卡片:封面 + 标题 + 作者。本期纯色占位;二期按 cover_path 分支 Cover::image。
fn cover_card(item: &MediaItem) -> View {
    media_card(&item.title, &item.creator, Cover::solid(item.cover_rgb))
}
