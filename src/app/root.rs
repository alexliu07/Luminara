use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows_reactor::*;

use crate::app::browse::{BrowseEntry, BrowseInput, BrowsePage};
use crate::app::home::{Home, HomeInput};
use crate::app::library::{Library, LibraryInput};
use crate::app::page::Page;
use crate::app::placeholder::Placeholder;
use crate::app::player::{PlayerInput, PlayerPage, VideoPanelRef};
use crate::app::settings::{SettingsInput, SettingsPage};
use crate::model::format;
use crate::model::index;
use crate::model::library::{self, MediaLibrary};
use crate::model::media::{self, MediaItem};
use crate::model::settings::Settings;
use crate::model::subtitle;
use crate::platform;

/// 浏览状态:记录当前浏览的媒体库及目录位置。
struct BrowseState {
    root_path: PathBuf,
    current_path: PathBuf,
    entries: Vec<BrowseEntry>,
}

/// 右键菜单状态:目标媒体库 + 弹出位置(窗口坐标)。
struct ContextMenuState {
    id: u64,
    x: f64,
    y: f64,
    /// 入场动画第二帧标记;false 为初始隐藏帧,true 后过渡到显示。
    shown: bool,
}

/// 播放状态:播放列表(同级文件)+ 进度快照 + 字幕。播放器页本身无状态。
struct PlayerState {
    /// 同目录受支持文件(按文件名序),上一个/下一个按它切换。
    playlist: Vec<PathBuf>,
    index: usize,
    /// 当前位置 / 总时长(秒)。
    position: f64,
    duration: f64,
    paused: bool,
    /// 是否有视频流(纯音频显示占位)。
    has_video: bool,
    /// 播到片尾(防自动切曲重入)。
    ended: bool,
    error: Option<String>,
    /// 拖动进度条期间的临时位置;引擎追上前压过轮询值,防滑块回跳。
    scrub: Option<f64>,
    fullscreen: bool,
    subtitle: SubtitleState,
}

/// 字幕状态:轨道列表 + 当前选择与已解析字幕。
struct SubtitleState {
    /// 发现的外挂字幕轨(顺序即菜单顺序)。
    tracks: Rc<Vec<subtitle::SubtitleFile>>,
    /// 菜单标签,与 tracks 同序。
    labels: Rc<Vec<String>>,
    /// 当前轨;None = 关闭字幕。
    selected: Option<usize>,
    /// 当前轨已解析字幕。
    cues: Rc<Vec<subtitle::Cue>>,
    /// 字幕菜单是否展开。
    menu_open: bool,
}

impl SubtitleState {
    /// 发现同名字幕并默认载入第一条轨。
    fn for_media(path: &std::path::Path) -> Self {
        let tracks = Rc::new(subtitle::discover(path));
        let labels = Rc::new(tracks.iter().map(|t| t.label.clone()).collect());
        let mut state = Self {
            tracks,
            labels,
            selected: None,
            cues: Rc::new(Vec::new()),
            menu_open: false,
        };
        if !state.tracks.is_empty() {
            state.select(Some(0));
        }
        state
    }

    /// 切换字幕轨并解析(None = 关闭)。解析失败按无字幕处理。
    fn select(&mut self, value: Option<usize>) {
        self.selected = value;
        self.cues = Rc::new(
            value
                .and_then(|index| self.tracks.get(index))
                .and_then(|file| {
                    let text = std::fs::read_to_string(&file.path).ok()?;
                    Some(subtitle::parse(file.kind, &text))
                })
                .unwrap_or_default(),
        );
    }

    /// 某时刻的字幕行;无则空串。
    fn text_at(&self, time: f64) -> String {
        subtitle::cue_at(&self.cues, time)
            .map(|cue| cue.text.clone())
            .unwrap_or_default()
    }
}

pub struct Root {
    page: Page,
    library: Rc<Vec<MediaItem>>,
    /// 已添加的媒体库;变更即写盘持久化。
    libraries: Rc<Vec<MediaLibrary>>,
    next_library_id: u64,
    pane_open: bool,
    /// 应用设置;变更即写盘持久化。
    settings: Settings,
    /// 文件夹浏览状态;Some 时 Library 页显示浏览视图。
    browse: Option<BrowseState>,
    /// 后台索引扫描任务数;>0 时首页显示加载转圈。
    scans_in_flight: usize,
    /// 待移除确认的媒体库(id + 名称);Some 时弹确认对话框。
    pending_remove: Option<(u64, String)>,
    /// 右键菜单;Some 时显示菜单浮层。
    context_menu: Option<ContextMenuState>,
    /// 播放状态;Some 时显示播放器页(压过普通页面)。
    player: Option<PlayerState>,
    /// 播放内核(播放内核/Media Engine);Rc<RefCell> 与面板 Rendering 回调共享。
    player_core: Rc<RefCell<platform::PlayerCore>>,
    /// 视频面板引用(SwapChainPanel 挂交换链用)。
    video_ref: VideoPanelRef,
    /// 面板 Metrics/Rendering 观察句柄;必须保活,掉了回调就停(只取 Drop 语义)。
    #[allow(dead_code)]
    video_obs: Option<ElementObservation>,
}

pub enum Message {
    Select(Option<String>),
    PaneOpenChanged(bool),
    /// 媒体库页菜单项被点击(回传 label)。
    MenuInvoked(String),
    ScrapeChanged(bool),
    ApiKeyChanged(String),
    /// 打开媒体库浏览(回传 library id)。
    OpenLibrary(u64),
    /// 进入子目录(回传目录路径)。
    EnterDir(PathBuf),
    /// 返回上一级;在根目录时退回媒体库列表。
    GoBack,
    /// 一个媒体库索引扫描完成(后台线程回传)。
    IndexDone,
    /// 右键媒体库卡片,弹出上下文菜单(id + 光标窗口坐标)。
    OpenContextMenu(u64, f64, f64),
    /// 菜单入场动画第二帧(发布隐藏帧后回传,点亮 shown 触发过渡)。
    MenuShown,
    /// 菜单项被点击(回传 item key)。
    ContextAction(String),
    /// 关闭右键菜单(点空白处)。
    CloseContextMenu,
    /// 移除确认对话框关闭(回传按钮结果)。
    RemoveDialogClosed(ContentDialogResult),
    /// 打开媒体文件播放(浏览页点击回传路径)。
    OpenMediaFile(PathBuf),
    /// 播放器:切换播放/暂停。
    PlayerTogglePause,
    /// 播放器:快进/快退(秒,可负)。
    PlayerSeekBy(f64),
    /// 播放器:进度条拖动到新位置(秒)。
    PlayerScrub(f64),
    /// 播放器:上一个。
    PlayerPrev,
    /// 播放器:下一个。
    PlayerNext,
    /// 播放器:退出播放,回到浏览。
    PlayerExit,
    /// 播放器:切换全屏。
    PlayerFullscreen,
    /// 播放进度轮询(自续定时器回执)。
    PlayerTick,
    /// 字幕菜单开/关。
    SubtitleMenu(bool),
    /// 选择字幕轨(None = 关闭字幕)。
    SubtitleSelect(Option<usize>),
}

impl Component for Root {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), context: &ComponentContext<Self>) -> Self {
        let libraries = library::load_libraries();
        let next_library_id = libraries.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        // 启动时为每个媒体库后台重建索引;首页加载转圈随扫描结束而消失。
        let scans_in_flight = libraries.len();
        for lib in &libraries {
            Self::spawn_index_build(context, lib.id, PathBuf::from(&lib.path));
        }
        // 播放内核与视频面板:Rendering/Metrics 回调直接驱动渲染,不经消息泵。
        let player_core = Rc::new(RefCell::new(platform::PlayerCore::new()));
        let video_ref = VideoPanelRef(Rc::new(ElementRef::<SwapChainPanel>::new()));
        let video_obs = {
            let core = Rc::clone(&player_core);
            let panel = Rc::clone(&video_ref.0);
            video_ref.0.observe_surface(move |event| {
                let mut core = core.borrow_mut();
                match event {
                    SwapChainPanelEvent::Metrics {
                        width,
                        height,
                        scale_x,
                        scale_y,
                    } => {
                        let w = (width * scale_x as f64).round().max(1.0) as u32;
                        let h = (height * scale_y as f64).round().max(1.0) as u32;
                        if let Some(swap_chain) = core.resize(w, h) {
                            // 挂接被拒(面板还没发布)就留着 needs_attach,下次 Metrics 重试。
                            let attached = panel.request_set_swap_chain(
                                platform::to_framework_unknown(&swap_chain),
                                |_| {},
                            );
                            if attached {
                                core.set_attached();
                            }
                        }
                    }
                    // 每个合成帧一拍:有新帧就转进交换链呈现。
                    SwapChainPanelEvent::Rendering => core.render_frame(),
                }
            })
        };
        Self {
            page: Page::Home,
            library: Rc::new(media::sample_library()),
            libraries: Rc::new(libraries),
            next_library_id,
            pane_open: true,
            settings: Settings::load(),
            browse: None,
            scans_in_flight,
            pending_remove: None,
            context_menu: None,
            player: None,
            player_core,
            video_ref,
            video_obs: Some(video_obs),
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Select(tag) => {
                if let Some(page) = tag.as_deref().and_then(Page::from_tag) {
                    self.page = page;
                }
                // 导航切换顺带退出播放,免得播放器压住新页面。
                self.exit_player();
            }
            Message::PaneOpenChanged(open) => self.pane_open = open,
            Message::MenuInvoked(label) => {
                if label == "从本地添加" {
                    self.add_local_folder(context);
                }
            }
            Message::ScrapeChanged(on) => {
                self.settings.scrape_mode = on;
                self.settings.save();
            }
            Message::ApiKeyChanged(key) => {
                self.settings.tmdb_api_key = key;
                self.settings.save();
            }
            Message::OpenLibrary(id) => {
                // 菜单/确认期间忽略打开:右键松开也会触发 on_click,防误入浏览。
                if self.pending_remove.is_some() || self.context_menu.is_some() {
                    return;
                }
                if let Some(lib) = self.libraries.iter().find(|l| l.id == id) {
                    let root = PathBuf::from(&lib.path);
                    let entries = read_entries(&root);
                    self.browse = Some(BrowseState {
                        root_path: root.clone(),
                        current_path: root,
                        entries,
                    });
                }
            }
            Message::EnterDir(path) => {
                if let Some(state) = &mut self.browse {
                    let entries = read_entries(&path);
                    state.current_path = path;
                    state.entries = entries;
                }
            }
            Message::IndexDone => {
                self.scans_in_flight = self.scans_in_flight.saturating_sub(1);
            }
            Message::OpenContextMenu(id, x, y) => {
                // 先以隐藏帧弹出;发布后 use_effect 回 MenuShown 点亮,借过渡淡入。
                self.context_menu = Some(ContextMenuState {
                    id,
                    x,
                    y,
                    shown: false,
                });
            }
            Message::MenuShown => {
                if let Some(menu) = &mut self.context_menu {
                    menu.shown = true;
                }
            }
            Message::ContextAction(key) => {
                if let Some(menu) = self.context_menu.take() {
                    // 后续菜单项在这里分发。
                    if key == "remove" {
                        if let Some(lib) = self.libraries.iter().find(|l| l.id == menu.id) {
                            self.pending_remove = Some((menu.id, lib.name.clone()));
                        }
                    }
                }
            }
            Message::CloseContextMenu => {
                self.context_menu = None;
            }
            Message::RemoveDialogClosed(result) => {
                if let Some((id, _)) = self.pending_remove.take() {
                    if result == ContentDialogResult::Primary {
                        // 仅删除数据记录与索引,本地文件不动。
                        Rc::make_mut(&mut self.libraries).retain(|lib| lib.id != id);
                        library::save_libraries(&self.libraries);
                        index::remove(id);
                    }
                }
            }
            Message::GoBack => {
                if let Some(state) = &mut self.browse {
                    if state.current_path == state.root_path {
                        self.browse = None;
                    } else {
                        if let Some(parent) = state.current_path.parent() {
                            let parent = parent.to_path_buf();
                            let entries = read_entries(&parent);
                            state.current_path = parent;
                            state.entries = entries;
                        }
                    }
                }
            }
            Message::OpenMediaFile(path) => {
                self.start_playback(path);
                Self::arm_tick(context);
            }
            Message::PlayerTogglePause => {
                self.player_core.borrow_mut().toggle_pause();
                self.refresh_snapshot();
            }
            Message::PlayerSeekBy(delta) => {
                self.player_core.borrow_mut().seek_by(delta);
                if let Some(state) = &mut self.player {
                    state.ended = false;
                    state.scrub = None;
                }
                self.refresh_snapshot();
            }
            Message::PlayerScrub(time) => {
                self.player_core.borrow_mut().seek_to(time);
                if let Some(state) = &mut self.player {
                    state.ended = false;
                    // 拖动值先顶上显示,等引擎位置追上来再放开,防进度条回跳。
                    state.scrub = Some(time);
                }
            }
            Message::PlayerPrev => self.switch_track(-1),
            Message::PlayerNext => self.switch_track(1),
            Message::PlayerExit => self.exit_player(),
            Message::PlayerFullscreen => {
                let fullscreen = platform::toggle_fullscreen();
                if let Some(state) = &mut self.player {
                    state.fullscreen = fullscreen;
                }
            }
            Message::PlayerTick => {
                self.refresh_snapshot();
                if self.player.is_some() {
                    Self::arm_tick(context);
                }
            }
            Message::SubtitleMenu(open) => {
                if let Some(state) = &mut self.player {
                    state.subtitle.menu_open = open;
                }
            }
            Message::SubtitleSelect(value) => {
                if let Some(state) = &mut self.player {
                    state.subtitle.select(value);
                    state.subtitle.menu_open = false;
                }
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("Luminara");
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(1100.0, 720.0),
        );

        let menu = Page::ALL.map(|page| {
            KeyedView::new(
                page.tag(),
                NavigationViewItem::new()
                    .tag(page.tag())
                    .is_selected(page == self.page)
                    .slots([
                        SlotView::new(
                            NavigationViewItemSlot::Icon,
                            SymbolIcon::new().symbol(page.icon()),
                        ),
                        SlotView::new(NavigationViewItemSlot::Content, page.title()),
                    ]),
            )
        });

        // 播放中压过普通页面;返回即回浏览现场(browse 状态未动)。
        let content = if let Some(state) = &self.player {
            let path = &state.playlist[state.index];
            View::component::<PlayerPage>(PlayerInput {
                title: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                position: state.position,
                duration: state.duration,
                paused: state.paused,
                has_video: state.has_video,
                error: state.error.clone(),
                subtitle_text: state.subtitle.text_at(state.position),
                subtitle_labels: Rc::clone(&state.subtitle.labels),
                subtitle_selected: state.subtitle.selected,
                subtitle_menu_open: state.subtitle.menu_open,
                fullscreen: state.fullscreen,
                at_first: state.index == 0,
                at_last: state.index + 1 >= state.playlist.len(),
                video_ref: self.video_ref.clone(),
                on_back: context.callback(|_: ()| Message::PlayerExit),
                on_toggle_pause: context.callback(|_: ()| Message::PlayerTogglePause),
                on_seek: context.callback(Message::PlayerScrub),
                on_seek_by: context.callback(Message::PlayerSeekBy),
                on_prev: context.callback(|_: ()| Message::PlayerPrev),
                on_next: context.callback(|_: ()| Message::PlayerNext),
                on_fullscreen: context.callback(|_: ()| Message::PlayerFullscreen),
                on_subtitle_menu: context.callback(|_: ()| Message::SubtitleMenu(true)),
                on_subtitle_select: context.callback(Message::SubtitleSelect),
                on_close_subtitle_menu: context.callback(|_: ()| Message::SubtitleMenu(false)),
            })
        } else {
            match self.page {
                Page::Home => View::component::<Home>(HomeInput {
                    items: Rc::clone(&self.library),
                    scanning: self.scans_in_flight > 0,
                }),
                Page::Library => {
                    if let Some(state) = &self.browse {
                        let dir_name = state
                            .current_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| state.current_path.to_string_lossy().into_owned());
                        View::component::<BrowsePage>(BrowseInput {
                            dir_name,
                            dir_path: state.current_path.to_string_lossy().into_owned(),
                            at_root: state.current_path == state.root_path,
                            entries: Rc::new(state.entries.clone()),
                            on_enter: context.callback(Message::EnterDir),
                            on_back: context.callback(|_: ()| Message::GoBack),
                        })
                    } else {
                        View::component::<Library>(LibraryInput {
                            libraries: Rc::clone(&self.libraries),
                            on_menu: context.callback(Message::MenuInvoked),
                            on_open: context.callback(Message::OpenLibrary),
                            on_context: context.callback(|(id, x, y): (u64, f64, f64)| {
                                Message::OpenContextMenu(id, x, y)
                            }),
                        })
                    }
                }
                Page::Settings => View::component::<SettingsPage>(SettingsInput {
                    scrape_mode: self.settings.scrape_mode,
                    tmdb_api_key: self.settings.tmdb_api_key.clone(),
                    on_scrape_changed: context.callback(Message::ScrapeChanged),
                    on_api_key_changed: context.callback(Message::ApiKeyChanged),
                }),
                other => View::component::<Placeholder>(other),
            }
        };

        let shell = NavigationView::new()
            .pane_display_mode(NavigationViewPaneDisplayMode::Left)
            .pane_title("Luminara")
            .is_settings_visible(false)
            // is_pane_open 必须受控:windows-reactor 会在重渲染时把未声明的
            // 双向属性 ClearProperty 清回默认值(true),导致折叠按钮点不动。
            .is_pane_open(self.pane_open)
            .on_is_pane_open_changed(context.callback(Message::PaneOpenChanged))
            .on_selected_tag_changed(context.callback(Message::Select))
            .slots([
                SlotView::collection(NavigationViewSlot::MenuItems, menu),
                SlotView::new(NavigationViewSlot::Content, content),
            ]);

        // 移除确认对话框常驻挂载,按 pending_remove 开关(框架无 Visibility,靠 is_open)。
        // 须挂在 Children 角色面板下(框架约束),Grid 叠层不占布局;打开时 owner 取 Grid,XamlRoot 可用。
        let dialog_text = match &self.pending_remove {
            Some((_, name)) => {
                format!("将移除媒体库「{name}」的数据记录与索引。\n本地文件不会被删除。")
            }
            None => String::new(),
        };
        let remove_dialog = ContentDialog::new()
            .is_open(self.pending_remove.is_some())
            .title("移除媒体库")
            .primary_button_text("移除")
            .secondary_button_text("取消")
            .on_closed(context.callback(Message::RemoveDialogClosed))
            .content(
                TextBlock::new()
                    .text(dialog_text)
                    .font_size(14.0)
                    .foreground(ThemeBrush::PrimaryText),
            );

        // 菜单入场动画:先发布隐藏帧,发布完成后回 MenuShown 点亮,借过渡淡入。
        let sender = context.sender();
        context.use_effect("menu-entrance", self.context_menu.is_some(), move || {
            sender.send(Message::MenuShown);
            None
        });

        if let Some(state) = &self.context_menu {
            let overlay = context_menu_overlay(
                state,
                context.callback(Message::ContextAction),
                context.callback(|_: ()| Message::CloseContextMenu),
            );
            Grid::new().children((shell, remove_dialog, overlay))
        } else {
            Grid::new().children((shell, remove_dialog))
        }
    }
}

impl Root {
    /// 打开媒体文件:建播放列表(同级受支持文件按名序)并起播。
    fn start_playback(&mut self, path: PathBuf) {
        let Some(dir) = path.parent() else {
            return;
        };
        let playlist = read_media_files(dir);
        let Some(index) = playlist.iter().position(|p| p == &path) else {
            return;
        };
        // 已在播放中换文件时保留全屏态,避免每换一集都抖一次窗口。
        let fullscreen = self.player.as_ref().is_some_and(|p| p.fullscreen);
        self.player = Some(PlayerState {
            playlist,
            index,
            position: 0.0,
            duration: 0.0,
            paused: false,
            has_video: false,
            ended: false,
            error: None,
            scrub: None,
            fullscreen,
            subtitle: SubtitleState::for_media(&path),
        });
        self.open_track();
    }

    /// 切到当前 index 的曲目:重置快照、重载字幕、交给内核。
    fn open_track(&mut self) {
        let Some(state) = &mut self.player else {
            return;
        };
        let path = state.playlist[state.index].clone();
        state.position = 0.0;
        state.duration = 0.0;
        state.paused = false;
        state.has_video = false;
        state.ended = false;
        state.error = None;
        state.scrub = None;
        state.subtitle = SubtitleState::for_media(&path);
        let result = self.player_core.borrow_mut().open(&path);
        if let Err(error) = result {
            if let Some(state) = &mut self.player {
                state.error = Some(error);
            }
        }
    }

    /// 上一个/下一个:在同级文件列表内移动,到头/到尾不动。
    fn switch_track(&mut self, delta: i64) {
        let Some(state) = &mut self.player else {
            return;
        };
        let next = state.index as i64 + delta;
        if !(0..state.playlist.len() as i64).contains(&next) {
            return;
        }
        state.index = next as usize;
        self.open_track();
    }

    /// 退出播放:停内核、退全屏、关播放器页。
    fn exit_player(&mut self) {
        if self.player.is_none() {
            return;
        }
        self.player_core.borrow_mut().stop();
        if self.player.as_ref().is_some_and(|p| p.fullscreen) {
            platform::toggle_fullscreen();
        }
        self.player = None;
    }

    /// 轮询内核快照:更新进度/状态,播完自动切下一个。
    fn refresh_snapshot(&mut self) {
        let snapshot = self.player_core.borrow().snapshot();
        let Some(state) = &mut self.player else {
            return;
        };
        state.has_video = snapshot.has_video;
        state.paused = snapshot.paused;
        state.duration = snapshot.duration;
        state.error = if snapshot.error {
            Some("无法播放该文件。".to_string())
        } else {
            None
        };
        // 拖动进度条时引擎位置还没追上目标,继续压住显示,防滑块回跳。
        match state.scrub {
            Some(target) if (snapshot.position - target).abs() >= 0.75 => {
                state.position = target;
            }
            _ => {
                state.scrub = None;
                state.position = snapshot.position;
            }
        }
        let mut advance = false;
        if snapshot.ended && !snapshot.error {
            if !state.ended {
                state.ended = true;
                advance = state.index + 1 < state.playlist.len();
                if advance {
                    state.index += 1;
                }
            }
        } else {
            state.ended = false;
        }
        if advance {
            self.open_track();
        }
    }

    /// 250ms 自续定时器:spawn_background 回一条 PlayerTick,收尾再派下一个。
    /// 坑:框架没有 interval API,LocalSender 是 Rc 派的(非 Send),后台线程不能直接发消息,
    /// 只能靠这条「完成回执」接力驱动进度轮询。
    fn arm_tick(context: &ComponentContext<Self>) {
        context.spawn_background(|token| {
            for _ in 0..25 {
                if token.is_cancelled() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Message::PlayerTick
        });
    }

    /// 弹系统文件夹选择框,选中的文件夹按路径去重后加入媒体库;取消或重复则无变化。
    /// 选择框模态自带消息循环,update 内同步调用安全(LocalSender 队列化投递)。
    /// 添加后后台线程扫描目录树并持久化索引。
    fn add_local_folder(&mut self, context: &ComponentContext<Self>) {
        let Some(path) = platform::pick_folder() else {
            return;
        };
        if self.libraries.iter().any(|lib| lib.path == path) {
            return;
        }
        let library = MediaLibrary::from_path(self.next_library_id, path);
        let lib_id = library.id;
        let scan_path = PathBuf::from(&library.path);
        self.next_library_id += 1;
        Rc::make_mut(&mut self.libraries).push(library);
        library::save_libraries(&self.libraries);
        // 后台扫描目录树并写索引文件;首页加载转圈随扫描结束而消失。
        self.scans_in_flight += 1;
        Self::spawn_index_build(context, lib_id, scan_path);
    }

    /// 后台重建某媒体库的文件索引;完成回 IndexDone 更新加载指示。
    fn spawn_index_build(context: &ComponentContext<Self>, lib_id: u64, scan_path: PathBuf) {
        // Message 必须 Send。
        context.spawn_background(move |_token| {
            let entries = index::build(&scan_path);
            index::save(lib_id, &entries);
            Message::IndexDone
        });
    }
}

/// 右键菜单浮层:全屏点击捕获层(点空白关闭)+ 光标处菜单卡片。
/// 后续菜单项往 items 里加,ContextAction 按 key 分发。
fn context_menu_overlay(
    state: &ContextMenuState,
    on_action: Callback<String>,
    on_close: Callback<()>,
) -> View {
    let items = [("remove", "删除媒体库")];
    let buttons = items.map(|(key, label)| {
        let on_action = on_action.clone();
        let key = key.to_string();
        Button::new()
            .on_click(move || {
                let _ = on_action.call(key.clone());
            })
            .content(label)
    });
    Grid::new()
        // 关闭时整层移除,借退出过渡淡出。
        .exit_transition(ExitTransition::fade(Duration::from_millis(150)))
        .children((
            // 近乎透明的全屏层:吃掉外部点击以关闭菜单。
            Border::new()
                .background(Color::argb(1, 0, 0, 0))
                .on_pointer_released(move |_info: PointerEventInfo| {
                    let _ = on_close.call(());
                }),
            Border::new()
                // 必须显式 Left/Top:默认 Stretch 时,XAML 对定了宽/高的元素按居中排,
                // margin 定位会偏到剩余空间中间,离光标很远。
                .horizontal_alignment(HorizontalAlignment::Left)
                .vertical_alignment(VerticalAlignment::Top)
                // 入场过渡:隐藏帧不设 transition(瞬时落到 0),第二帧先装 transition 再
                // 改 opacity(props 先于 element_state 应用),借隐式动画 0→1 淡入。
                .opacity_transition(if state.shown {
                    Some(Duration::from_millis(150))
                } else {
                    None
                })
                .opacity(if state.shown { 1.0 } else { 0.0 })
                .corner_radius(CornerRadius::uniform(8.0))
                .background(Color::rgb(40, 40, 40))
                .padding(Thickness::uniform(4.0))
                .margin(Thickness::new(state.x, state.y, 0.0, 0.0))
                .content(StackPanel::new().spacing(2.0).children(buttons)),
        ))
}

/// 一个目录里的可播放媒体文件,按文件名排序(上一个/下一个的切换顺序)。
fn read_media_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read_dir
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            let is_file = item.metadata().map(|meta| meta.is_file()).unwrap_or(false);
            (is_file && format::is_supported(&name)).then(|| item.path())
        })
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
}

/// 读取一个目录的条目(子目录 + 文件),排序:目录在前、文件在后,各按名称。
fn read_entries(dir: &std::path::Path) -> Vec<BrowseEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for item in read_dir.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        let path = item.path();
        let Ok(meta) = item.metadata() else {
            continue;
        };
        let is_dir = meta.is_dir();
        let entry = BrowseEntry {
            name,
            path,
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
        };
        if is_dir {
            dirs.push(entry);
        } else {
            files.push(entry);
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.extend(files);
    dirs
}
