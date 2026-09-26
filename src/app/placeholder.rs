use windows_reactor::*;

use crate::app::page::Page;

/// 尚未实现的页面占位。
pub struct Placeholder;

impl Component for Placeholder {
    type Input = Page;
    type Message = ();

    fn create(_input: &Page, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Page, _context: &mut ViewContext<Self>) -> View {
        Border::new().padding(Thickness::uniform(24.0)).content(
            StackPanel::new().spacing(12.0).children((
                TextBlock::new()
                    .text(input.title())
                    .font_size(28.0)
                    .font_weight(FontWeight::BOLD)
                    .foreground(ThemeBrush::PrimaryText),
                TextBlock::new()
                    .text("此页面建设中,敬请期待。")
                    .font_size(14.0)
                    .foreground(Color::rgb(150, 150, 150)),
            )),
        )
    }
}
