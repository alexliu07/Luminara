use std::rc::Rc;
use windows_reactor::*;

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

/// 单个作品卡片:封面占位 + 标题 + 作者。
fn cover_card(item: &MediaItem) -> View {
    let (r, g, b) = item.cover_rgb;
    Border::new()
        .padding(Thickness::uniform(6.0))
        .content(
            Grid::new()
                .columns([GridLength::Pixel(160.0)])
                .rows([
                    GridLength::Pixel(160.0),
                    GridLength::Auto,
                    GridLength::Auto,
                ])
                .children((
                    Border::new()
                        .corner_radius(CornerRadius::uniform(8.0))
                        .background(Color::rgb(r, g, b))
                        .grid_row(0),
                    TextBlock::new()
                        .text(item.title.clone())
                        .font_size(15.0)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .foreground(ThemeBrush::PrimaryText)
                        .text_wrapping(TextWrapping::Wrap)
                        .grid_row(1),
                    TextBlock::new()
                        .text(item.creator.clone())
                        .font_size(12.0)
                        .foreground(Color::rgb(150, 150, 150))
                        .text_wrapping(TextWrapping::Wrap)
                        .grid_row(2),
                )),
        )
}
