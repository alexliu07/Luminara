use windows_reactor::*;

/// 设置页输入:当前设置值 + 变更回调(持久化由 Root 完成)。
#[derive(Clone, PartialEq)]
pub struct SettingsInput {
    pub scrape_mode: bool,
    pub tmdb_api_key: String,
    pub on_scrape_changed: Callback<bool>,
    pub on_api_key_changed: Callback<String>,
}

/// 设置页:刮削模式开关;开启时额外显示 TMDB API Key 输入框。
pub struct SettingsPage;

impl Component for SettingsPage {
    // 页面保持无状态(与 Home/Library 同风格),持久化在 Root。
    type Input = SettingsInput;
    type Message = ();

    fn create(_input: &Self::Input, _context: &ComponentContext<Self>) -> Self {
        Self
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, input: &Self::Input, _context: &mut ViewContext<Self>) -> View {
        let title = TextBlock::new()
            .text("设置")
            .font_size(28.0)
            .font_weight(FontWeight::BOLD)
            .foreground(ThemeBrush::PrimaryText);

        let scrape_row = StackPanel::new().spacing(4.0).children((
            ToggleSwitch::new()
                .is_on(input.scrape_mode)
                .on_toggled(input.on_scrape_changed.clone())
                .slots([SlotView::new(ToggleSwitchSlot::Header, "刮削模式")]),
            TextBlock::new()
                .text("扫描媒体库内容并补全元数据(扫描功能后续提供)。")
                .font_size(12.0)
                .foreground(Color::rgb(150, 150, 150)),
        ));

        let api_key_row = StackPanel::new().spacing(4.0).children((
            TextBlock::new()
                .text("TMDB API Key")
                .font_size(14.0)
                .foreground(ThemeBrush::PrimaryText),
            TextBox::new()
                .text(input.tmdb_api_key.clone())
                .placeholder_text("输入 TMDB API Key")
                .on_text_changed(input.on_api_key_changed.clone()),
            TextBlock::new()
                .text("刮削模式下用于访问 TMDB 元数据服务。")
                .font_size(12.0)
                .foreground(Color::rgb(150, 150, 150)),
        ));

        Border::new()
            .padding(Thickness::uniform(24.0))
            // 刮削模式开启时才显示 TMDB API Key 输入框。
            .content(if input.scrape_mode {
                StackPanel::new().spacing(16.0).children((title, scrape_row, api_key_row))
            } else {
                StackPanel::new().spacing(16.0).children((title, scrape_row))
            })
    }
}
