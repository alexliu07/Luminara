//! 播放器页:视频画面(SwapChainPanel)+ 进度/控制条 + 字幕叠加。
//! 页面无状态:播放列表、进度、字幕选择全在 Root,这里只负责排版与发消息。

use std::rc::Rc;
use std::time::Duration;
use windows_reactor::*;

/// ElementRef 没有 PartialEq,而 Input 需要;引用同一性比较即可。
#[derive(Clone)]
pub struct VideoPanelRef(pub Rc<ElementRef<SwapChainPanel>>);

impl PartialEq for VideoPanelRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// 播放器页输入:播放快照 + 回调。文案全部极简。
#[derive(Clone, PartialEq)]
pub struct PlayerInput {
    pub title: String,
    /// 当前位置 / 总时长(秒)。
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    /// 有无视频流;false 时画面区只显示「音频」。
    pub has_video: bool,
    /// 出错文案;Some 时显示在画面区。
    pub error: Option<String>,
    /// 当前字幕行(Root 已按位置求好);空串不显示。
    pub subtitle_text: String,
    /// 字幕轨标签,下标即轨号。
    pub subtitle_labels: Rc<Vec<String>>,
    /// 当前字幕轨;None = 关闭字幕。
    pub subtitle_selected: Option<usize>,
    pub subtitle_menu_open: bool,
    pub fullscreen: bool,
    /// 播放列表到头/到尾(禁用上一个/下一个)。
    pub at_first: bool,
    pub at_last: bool,
    pub video_ref: VideoPanelRef,
    pub on_back: Callback<()>,
    pub on_toggle_pause: Callback<()>,
    /// 进度条拖动(秒)。
    pub on_seek: Callback<f64>,
    /// 快进/快退(秒,可负)。
    pub on_seek_by: Callback<f64>,
    pub on_prev: Callback<()>,
    pub on_next: Callback<()>,
    pub on_fullscreen: Callback<()>,
    pub on_subtitle_menu: Callback<()>,
    pub on_subtitle_select: Callback<Option<usize>>,
    pub on_close_subtitle_menu: Callback<()>,
}

/// 快进/快退步长(秒)。
const SEEK_STEP: f64 = 10.0;

pub struct PlayerPage;

impl Component for PlayerPage {
    type Input = PlayerInput;
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
                Button::new()
                    .on_click(input.on_back.clone())
                    .content(SymbolIcon::new().symbol(Symbol::Back)),
                TextBlock::new()
                    .text(input.title.clone())
                    .font_size(20.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(ThemeBrush::PrimaryText)
                    .vertical_alignment(VerticalAlignment::Center)
                    .max_lines(1)
                    .text_trimming(TextTrimming::CharacterEllipsis),
            ));

        // 画面区:SwapChainPanel 打底,提示/字幕叠同一格(Grid 兄弟叠层不占布局)。
        // children 只收定长元组/数组(没有 Vec 进口),无内容的层用空串占位。
        let message = if let Some(error) = &input.error {
            error.clone()
        } else if input.has_video {
            String::new()
        } else {
            "音频".to_string()
        };
        let video_area = Border::new()
            .corner_radius(CornerRadius::uniform(8.0))
            .background(Color::rgb(0, 0, 0))
            .grid_row(1)
            .content(
                Grid::new().children((
                    SwapChainPanel::new().element_ref(&input.video_ref.0),
                    TextBlock::new()
                        .text(message)
                        .font_size(16.0)
                        .foreground(ThemeBrush::PrimaryText)
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .vertical_alignment(VerticalAlignment::Center),
                    TextBlock::new()
                        .text(input.subtitle_text.clone())
                        .font_size(18.0)
                        .foreground(Color::rgb(255, 255, 255))
                        .text_wrapping(TextWrapping::Wrap)
                        .max_lines(2)
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .vertical_alignment(VerticalAlignment::Bottom)
                        .margin(Thickness::new(24.0, 0.0, 24.0, 24.0)),
                )),
            );

        // 进度行:当前时间 + 进度条 + 总时长。
        let progress = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                time_label(input.position),
                Slider::new()
                    .minimum(0.0)
                    .maximum(input.duration.max(1.0))
                    .value(input.position.clamp(0.0, input.duration.max(1.0)))
                    .on_value_changed(input.on_seek.clone()),
                time_label(input.duration),
            ));

        // 控制行:上一个 快退 播放/暂停 快进 下一个 全屏 字幕。
        let controls = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .horizontal_alignment(HorizontalAlignment::Center)
            .children([
                icon_row(
                    Symbol::Previous,
                    input.on_prev.clone(),
                    ButtonStyle::Default,
                    !input.at_first,
                ),
                seek_row(false, input.on_seek_by.clone()),
                icon_row(
                    if input.paused {
                        Symbol::Play
                    } else {
                        Symbol::Pause
                    },
                    input.on_toggle_pause.clone(),
                    ButtonStyle::Accent,
                    true,
                ),
                seek_row(true, input.on_seek_by.clone()),
                icon_row(
                    Symbol::Next,
                    input.on_next.clone(),
                    ButtonStyle::Default,
                    !input.at_last,
                ),
                icon_row(
                    if input.fullscreen {
                        Symbol::BackToWindow
                    } else {
                        Symbol::FullScreen
                    },
                    input.on_fullscreen.clone(),
                    ButtonStyle::Default,
                    true,
                ),
                icon_row(
                    Symbol::ClosedCaption,
                    input.on_subtitle_menu.clone(),
                    ButtonStyle::Default,
                    true,
                ),
            ]);

        let bottom = StackPanel::new()
            .spacing(8.0)
            .grid_row(2)
            .children((progress, controls));

        let content = Grid::new()
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .children((title_row, video_area, bottom));

        if input.subtitle_menu_open {
            Grid::new().children((content, subtitle_menu(input)))
        } else {
            content
        }
    }
}

/// 图标按钮(.content 是终点方法,样式/可用性都得排在它前面)。
fn icon_row(symbol: Symbol, on_click: Callback<()>, style: ButtonStyle, enabled: bool) -> View {
    Button::new()
        .on_click(on_click)
        .style(style)
        .is_enabled(enabled)
        .content(SymbolIcon::new().symbol(symbol))
}

/// 快退/快进按钮:点击回传 ±SEEK_STEP 秒。
fn seek_row(forward: bool, on_seek_by: Callback<f64>) -> View {
    let delta = if forward { SEEK_STEP } else { -SEEK_STEP };
    icon_row(
        if forward {
            Symbol::Forward
        } else {
            Symbol::Back
        },
        Callback::new(move |()| {
            let _ = on_seek_by.call(delta);
        }),
        ButtonStyle::Default,
        true,
    )
}

/// 时间标签(固定宽,防止跳动)。
fn time_label(seconds: f64) -> TextBlock {
    TextBlock::new()
        .text(format_time(seconds))
        .font_size(12.0)
        .foreground(Color::rgb(150, 150, 150))
        .width(52.0)
        .vertical_alignment(VerticalAlignment::Center)
}

/// 秒 → `h:mm:ss` / `mm:ss`。
fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, total % 3600 / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// 字幕选择菜单:透明捕获层(点空白关闭)+ 右下角菜单卡片。选中项用 Accent 高亮。
fn subtitle_menu(input: &PlayerInput) -> View {
    let mut options: Vec<(Option<usize>, String)> = vec![(None, "关闭".to_string())];
    options.extend(
        input
            .subtitle_labels
            .iter()
            .enumerate()
            .map(|(index, label)| (Some(index), label.clone())),
    );
    // 选项数不定,children 只收定长,借 ListBox 的 collection_slot 收迭代器。
    let rows = options
        .into_iter()
        .enumerate()
        .map(|(key, (value, label))| KeyedView::new(key, subtitle_item(&label, value, input)));
    Grid::new()
        // 关闭时整层移除,借退出过渡淡出。
        .exit_transition(ExitTransition::fade(Duration::from_millis(150)))
        .children((
            Border::new()
                .background(Color::argb(1, 0, 0, 0))
                .on_pointer_released({
                    let on_close = input.on_close_subtitle_menu.clone();
                    move |_info: PointerEventInfo| {
                        let _ = on_close.call(());
                    }
                }),
            Border::new()
                .horizontal_alignment(HorizontalAlignment::Right)
                .vertical_alignment(VerticalAlignment::Bottom)
                .corner_radius(CornerRadius::uniform(8.0))
                .background(Color::rgb(40, 40, 40))
                .padding(Thickness::uniform(4.0))
                .margin(Thickness::new(0.0, 0.0, 24.0, 24.0))
                .content(ListBox::new().collection_slot(ListBoxSlot::Items, rows)),
        ))
}

/// 字幕菜单一项。
fn subtitle_item(label: &str, value: Option<usize>, input: &PlayerInput) -> View {
    let on_select = input.on_subtitle_select.clone();
    Button::new()
        .style(if input.subtitle_selected == value {
            ButtonStyle::Accent
        } else {
            ButtonStyle::Default
        })
        .on_click(move || {
            let _ = on_select.call(value);
        })
        .content(label)
}
