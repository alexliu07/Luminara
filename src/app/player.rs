//! 播放器页:视频画面(SwapChainPanel)+ 浮层顶栏/底栏 + 进度与控制条。
//! 页面无状态:播放列表、进度、全屏、字幕选择全在 Root,这里只负责排版与发消息。
//! 文案全部极简;图标全部手绘(见 icon.rs)。

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use windows_reactor::*;

use crate::app::icon::{self, Icon};

/// ElementRef 没有 PartialEq,而 Input 需要;引用同一性比较即可。
#[derive(Clone)]
pub struct VideoPanelRef(pub Rc<ElementRef<SwapChainPanel>>);

impl PartialEq for VideoPanelRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// 字幕覆盖层(PNG 编码的 RGBA 帧)。按指针比较:内容动辄几 MB,不能逐字节比。
#[derive(Clone)]
pub struct OverlayImage(pub Arc<[u8]>);

impl PartialEq for OverlayImage {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// 快进/快退步长(秒)。
const SEEK_STEP: f64 = 10.0;

/// 播放器页输入:播放快照 + 回调。
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
    /// 拖动进度条中的临时位置;None 用 position。
    pub scrub: Option<f64>,
    /// 全屏中(图标切换 + 顶底栏自动隐藏)。
    pub fullscreen: bool,
    /// 顶底栏可见(全屏时鼠标移动唤回,闲置隐藏)。
    pub chrome_visible: bool,
    /// 播放列表到头/到尾(禁用上一个/下一个)。
    pub at_first: bool,
    pub at_last: bool,
    /// 字幕轨标签(内嵌在前、外挂在后),下标即选择值。
    pub subtitle_labels: Rc<Vec<String>>,
    /// 当前字幕选择;None = 关闭。
    pub subtitle_selected: Option<usize>,
    /// 字幕菜单是否展开。
    pub subtitle_menu_open: bool,
    /// 外挂自绘覆盖层(PNG);内嵌轨由 mpv 渲染,无此项。
    pub overlay: Option<OverlayImage>,
    pub video_ref: VideoPanelRef,
    pub on_back: Callback<()>,
    pub on_toggle_pause: Callback<()>,
    /// 快退/快进(秒,可负)。
    pub on_seek_by: Callback<f64>,
    /// 拖动开始(压住引擎位置,防回跳)。
    pub on_scrub_start: Callback<()>,
    /// 拖动中(只更新 UI)。
    pub on_scrub: Callback<f64>,
    /// 松手(提交一次 seek)。
    pub on_scrub_commit: Callback<()>,
    pub on_prev: Callback<()>,
    pub on_next: Callback<()>,
    pub on_fullscreen: Callback<()>,
    /// 字幕菜单开/关。
    pub on_subtitle_menu: Callback<()>,
    /// 选字幕轨;None = 关闭字幕。
    pub on_subtitle_select: Callback<Option<usize>>,
    pub on_close_subtitle_menu: Callback<()>,
    /// 点击画面(双击判定在 Root:400ms 内两次=切全屏)。
    pub on_video_press: Callback<()>,
    /// 鼠标在画面区移动(唤回顶底栏)。
    pub on_pointer_moved: Callback<()>,
}

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
                icon_button(Icon::Back, input.on_back.clone(), true),
                TextBlock::new()
                    .text(input.title.clone())
                    .font_size(20.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(ThemeBrush::PrimaryText)
                    .vertical_alignment(VerticalAlignment::Center)
                    .max_lines(1)
                    .text_trimming(TextTrimming::CharacterEllipsis),
            ));

        // 画面区:SwapChainPanel 打底,外挂字幕覆盖层与提示叠同一格
        // (Grid 兄弟叠层不占布局)。内嵌字幕由 mpv 渲进画面,无需覆盖层。
        let message = if let Some(error) = &input.error {
            error.clone()
        } else if input.has_video {
            String::new()
        } else {
            "音频".to_string()
        };
        let message_text = TextBlock::new()
            .text(message)
            .font_size(16.0)
            .foreground(ThemeBrush::PrimaryText)
            .horizontal_alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Center);
        let video_grid = match &input.overlay {
            Some(overlay) => Grid::new().children((
                SwapChainPanel::new().element_ref(&input.video_ref.0),
                Image::new()
                    .source_data(EncodedImage::new(Arc::clone(&overlay.0)))
                    .stretch(Stretch::Fill),
                message_text,
            )),
            None => Grid::new().children((
                SwapChainPanel::new().element_ref(&input.video_ref.0),
                message_text,
            )),
        };
        let video_area = Border::new()
            .corner_radius(CornerRadius::uniform(8.0))
            .background(Color::rgb(0, 0, 0))
            .grid_row(1)
            .on_pointer_pressed({
                let on_press = input.on_video_press.clone();
                move |_info: PointerEventInfo| {
                    let _ = on_press.call(());
                }
            })
            .on_pointer_moved({
                let on_moved = input.on_pointer_moved.clone();
                move |_info: PointerEventInfo| {
                    let _ = on_moved.call(());
                }
            })
            .content(video_grid);

        // 进度行:当前时间 + 进度条 + 总时长。拖动只更新 UI,松手才 seek。
        let shown = input.scrub.unwrap_or(input.position);
        let progress = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                time_label(shown),
                Border::new()
                    .on_pointer_pressed({
                        let on_start = input.on_scrub_start.clone();
                        move |_info: PointerEventInfo| {
                            let _ = on_start.call(());
                        }
                    })
                    .on_pointer_released({
                        let on_commit = input.on_scrub_commit.clone();
                        move |_info: PointerEventInfo| {
                            let _ = on_commit.call(());
                        }
                    })
                    .content(
                        Slider::new()
                            .minimum(0.0)
                            .maximum(input.duration.max(1.0))
                            .value(shown.clamp(0.0, input.duration.max(1.0)))
                            .on_value_changed(input.on_scrub.clone()),
                    ),
                time_label(input.duration),
            ));

        // 控制行:上一个 快退 播放/暂停 快进 下一个 全屏 字幕。
        let controls = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .horizontal_alignment(HorizontalAlignment::Center)
            .children([
                icon_button(Icon::Prev, input.on_prev.clone(), !input.at_first),
                seek_button(Icon::SeekBack, -SEEK_STEP, input.on_seek_by.clone()),
                icon_button(
                    if input.paused {
                        Icon::Play
                    } else {
                        Icon::Pause
                    },
                    input.on_toggle_pause.clone(),
                    true,
                ),
                seek_button(Icon::SeekForward, SEEK_STEP, input.on_seek_by.clone()),
                icon_button(Icon::Next, input.on_next.clone(), !input.at_last),
                icon_button(
                    if input.fullscreen {
                        Icon::FullscreenExit
                    } else {
                        Icon::Fullscreen
                    },
                    input.on_fullscreen.clone(),
                    true,
                ),
                icon_button(
                    Icon::Subtitles,
                    Callback::new({
                        let on_menu = input.on_subtitle_menu.clone();
                        move |()| {
                            let _ = on_menu.call(());
                        }
                    }),
                    true,
                ),
            ]);

        let bottom = StackPanel::new()
            .spacing(8.0)
            .grid_row(2)
            .children((progress, controls));

        // 顶底栏:全屏闲置时整块不渲染(免得透明层吃点击),鼠标移动唤回。
        let content = Grid::new()
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .children((
                chrome_slot(input.chrome_visible, title_row.into()),
                video_area,
                chrome_slot(input.chrome_visible, bottom.into()),
            ));

        if input.subtitle_menu_open {
            Grid::new().children((content, subtitle_menu(input)))
        } else {
            content
        }
    }
}

/// 顶底栏槽位:隐藏时置零高占位(无内容层用空 Border)。
fn chrome_slot(visible: bool, view: View) -> View {
    if visible {
        view
    } else {
        Border::new().height(0.0).into()
    }
}

/// 图标按钮(.content 是终点方法,可用性排在它前面)。
pub fn icon_button(icon: Icon, on_click: Callback<()>, enabled: bool) -> View {
    Button::new()
        .on_click(on_click)
        .is_enabled(enabled)
        .content(icon::path_icon(icon))
}

/// 快退/快进按钮:点击回传 delta 秒。
fn seek_button(icon: Icon, delta: f64, on_seek_by: Callback<f64>) -> View {
    icon_button(
        icon,
        Callback::new(move |()| {
            let _ = on_seek_by.call(delta);
        }),
        true,
    )
}

/// 时间标签(固定宽,防止跳动)。秒 → `h:mm:ss` / `mm:ss`。
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
