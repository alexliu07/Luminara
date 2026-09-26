use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows_reactor::*;

use crate::app::browse::{BrowseEntry, BrowseInput, BrowsePage};
use crate::app::home::{Home, HomeInput};
use crate::app::icon;
use crate::app::library::{Library, LibraryInput};
use crate::app::page::Page;
use crate::app::placeholder::Placeholder;
use crate::app::player::{OverlayImage, PlayerInput, PlayerPage, VideoPanelRef};
use crate::app::settings::{SettingsInput, SettingsPage};
use crate::model::format;
use crate::model::index;
use crate::model::library::{self, MediaLibrary};
use crate::model::media::{self, MediaItem};
use crate::model::settings::Settings;
use crate::model::subtitle::{self as submodel, SubtitleFile};
use crate::platform;
use crate::platform::subtitle::SubtitleWorker;

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

/// 播放状态:播放列表(同级文件)+ 进度快照。播放器页本身无状态。
struct PlayState {
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
    /// 进度条拖动中(拖动只更新 UI,松手一次 seek)。
    dragging: bool,
    /// 全屏中。
    fullscreen: bool,
    /// 顶底栏可见(全屏闲置隐藏、鼠标移动唤回)。
    chrome_visible: bool,
    /// 画面区上次点击时刻(毫秒,UNIX 纪元);双击=400ms 内两次。
    last_press_ms: i64,
    /// 字幕状态(菜单两类轨、选择、自绘覆盖层)。
    subtitle: SubtitleState,
}

/// 字幕选择值(重建菜单后仍能对上原选择)。
#[derive(Clone, PartialEq)]
enum SubtitleChoice {
    /// 内嵌轨(mpv 原生渲染)。
    Embedded(i64),
    /// 外挂字幕文件(自绘管线)。
    External(PathBuf),
}

/// 字幕状态。同一时刻只开一个字幕来源:
/// 选内嵌 → mpv sid 切换、自绘线程停;选外挂 → sid=no、自绘线程接管。
struct SubtitleState {
    /// 内嵌轨(sid, 标签)。
    embedded: Rc<Vec<(i64, String)>>,
    /// 外挂字幕文件(同名发现)。
    external: Rc<Vec<SubtitleFile>>,
    selected: Option<SubtitleChoice>,
    menu_open: bool,
    /// 外挂自绘覆盖层(PNG);内嵌轨由 mpv 渲染,此值恒 None。
    overlay: Option<OverlayImage>,
    /// 已见过的 track-list 版本;变了才重建内嵌轨列表。
    tracks_seen: u64,
    /// 覆盖层产出版本号(去重,只认更新的)。
    revision: u64,
}

impl SubtitleState {
    /// 打开文件时的初值:外挂字幕同名发现,内嵌轨等 track-list 就绪再补。
    fn for_media(path: &PathBuf) -> Self {
        Self {
            embedded: Rc::new(Vec::new()),
            external: Rc::new(submodel::discover(path)),
            selected: None,
            menu_open: false,
            overlay: None,
            tracks_seen: 0,
            revision: 0,
        }
    }

    /// 菜单标签:内嵌在前(语言/序号),外挂在后(文件名)。
    fn labels(&self) -> Rc<Vec<String>> {
        let mut labels: Vec<String> = self
            .embedded
            .iter()
            .map(|(_, label)| label.clone())
            .collect();
        labels.extend(self.external.iter().map(|file| file.label.clone()));
        Rc::new(labels)
    }

    /// 当前选择对应的菜单下标(高亮用)。
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        let offset = self.embedded.len();
        match selected {
            SubtitleChoice::Embedded(id) => self.embedded.iter().position(|(sid, _)| sid == id),
            SubtitleChoice::External(path) => self
                .external
                .iter()
                .position(|file| &file.path == path)
                .map(|index| index + offset),
        }
    }

    /// 菜单下标 → 选择值。
    fn choice_at(&self, index: usize) -> Option<SubtitleChoice> {
        if let Some((sid, _)) = self.embedded.get(index) {
            Some(SubtitleChoice::Embedded(*sid))
        } else {
            self.external
                .get(index - self.embedded.len())
                .map(|file| SubtitleChoice::External(file.path.clone()))
        }
    }

    /// 外挂文件按路径取(交给自绘线程)。
    fn external_file(&self, path: &PathBuf) -> Option<SubtitleFile> {
        self.external
            .iter()
            .find(|file| &file.path == path)
            .cloned()
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
    player: Option<PlayState>,
    /// 播放内核(mpv);Rc<RefCell> 与面板 Rendering 回调共享。None = 初始化失败。
    player_core: Rc<RefCell<Option<platform::PlayerCore>>>,
    /// 外挂字幕自绘工作线程(解析/栅格化全在后台)。
    subtitle_worker: SubtitleWorker,
    /// 视频输出(D3D11 交换链 + GL);与 Rendering 回调共享。
    video_host: Rc<RefCell<Option<platform::VideoHost>>>,
    /// 视频面板引用(SwapChainPanel 挂交换链用)。
    video_ref: VideoPanelRef,
    /// 面板 Metrics/Rendering 观察句柄;必须保活,掉了回调就停(只取 Drop 语义)。
    #[allow(dead_code)]
    video_obs: Option<ElementObservation>,
    /// 播放内核/视频输出初始化失败文案;打开文件时显示在画面区。
    video_init_error: Option<String>,
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
    /// 进度条拖动开始(压住引擎位置显示)。
    PlayerScrubStart,
    /// 进度条拖动中(回传秒;只更新 UI)。
    PlayerScrub(f64),
    /// 进度条松手(提交一次 seek)。
    PlayerScrubCommit,
    /// 播放器:上一个。
    PlayerPrev,
    /// 播放器:下一个。
    PlayerNext,
    /// 播放器:切换全屏。
    PlayerFullscreen,
    /// 点击画面区(双击判定 → 切全屏)。
    PlayerVideoPress,
    /// 鼠标在画面区移动(唤回顶底栏)。
    PlayerChromeShow,
    /// 全屏闲置隐藏顶底栏(自续定时器回执)。
    PlayerChromeHide,
    /// 字幕菜单开/关。
    SubtitleMenuToggle,
    /// 关闭字幕菜单(点空白处)。
    SubtitleMenuClose,
    /// 选字幕轨(菜单下标);None = 关闭字幕。
    SubtitleSelect(Option<usize>),
    /// 播放页快捷键(空格/Esc/左右)。
    PlayerKey(platform::KeyAction),
    /// 播放器:退出播放,回到浏览。
    PlayerExit,
    /// 播放进度轮询(自续定时器回执)。
    PlayerTick,
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
        // 播放内核与视频输出:Rendering/Metrics 回调直接驱动渲染,不经消息泵。
        let player_core = Rc::new(RefCell::new(None));
        let video_host = Rc::new(RefCell::new(None));
        let mut video_init_error = None;
        let mut core_slot = match platform::PlayerCore::new() {
            Ok(core) => Some(core),
            Err(error) => {
                video_init_error = Some(error);
                None
            }
        };
        let mut host_slot = match platform::VideoHost::new() {
            Ok(host) => Some(host),
            Err(error) => {
                if video_init_error.is_none() {
                    video_init_error = Some(error);
                }
                None
            }
        };
        if let (Some(core), Some(host)) = (core_slot.as_mut(), host_slot.as_mut()) {
            match host.init_gl() {
                Ok(loader) => {
                    if let Err(error) = core.init_video(loader) {
                        video_init_error = Some(error);
                    }
                }
                Err(error) => {
                    // GL(ANGLE)不可用:音频仍可播,画面区显示错误文案。
                    video_init_error = Some(error);
                }
            }
        }
        *player_core.borrow_mut() = core_slot;
        *video_host.borrow_mut() = host_slot;
        // 字幕工作线程的时间源:mpv 事件线程写的 time-pos 原子位模式。
        // 内核没起来就给个哑时钟,反正也不会有播放。
        let time_bits = player_core
            .borrow()
            .as_ref()
            .map(|core| core.time_bits())
            .unwrap_or_default();
        let video_ref = VideoPanelRef(Rc::new(ElementRef::<SwapChainPanel>::new()));
        let video_obs = {
            let host_slot = Rc::clone(&video_host);
            let core_slot = Rc::clone(&player_core);
            let panel = Rc::clone(&video_ref.0);
            video_ref.0.observe_surface(move |event| match event {
                SwapChainPanelEvent::Metrics {
                    width,
                    height,
                    scale_x,
                    scale_y,
                } => {
                    let w = (width * scale_x as f64).round().max(1.0) as u32;
                    let h = (height * scale_y as f64).round().max(1.0) as u32;
                    if let Some(host) = host_slot.borrow_mut().as_mut() {
                        if let Some(swap_chain) = host.resize(w, h) {
                            // 挂接被拒(面板还没发布)就留着 needs_attach,下次 Metrics 重试。
                            let attached = panel.request_set_swap_chain(
                                platform::to_framework_unknown(&swap_chain),
                                |_| {},
                            );
                            if attached {
                                host.set_attached();
                            }
                        }
                    }
                }
                // 每个合成帧一拍:有新帧就渲染进交换链呈现。
                SwapChainPanelEvent::Rendering => {
                    let mut host_slot = host_slot.borrow_mut();
                    let mut core_slot = core_slot.borrow_mut();
                    if let (Some(host), Some(core)) = (host_slot.as_mut(), core_slot.as_mut()) {
                        if core.render_dirty() || host.dirty() {
                            let _ = host.present(|w, h| core.render_frame(w, h));
                        }
                    }
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
            subtitle_worker: SubtitleWorker::new(time_bits),
            video_host,
            video_ref,
            video_obs: Some(video_obs),
            video_init_error,
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
                // 播放页快捷键钩子:首次打开时装上,之后只开关激活位。
                Self::ensure_key_hook(context);
                platform::set_key_active(true);
                self.start_playback(path);
                Self::arm_tick(context);
            }
            Message::PlayerTogglePause => {
                if let Some(core) = self.player_core.borrow_mut().as_mut() {
                    core.toggle_pause();
                }
                self.refresh_snapshot();
            }
            Message::PlayerSeekBy(delta) => {
                if let Some(core) = self.player_core.borrow_mut().as_mut() {
                    core.seek_by(delta);
                }
                if let Some(state) = &mut self.player {
                    state.ended = false;
                    state.scrub = None;
                }
                self.refresh_snapshot();
            }
            Message::PlayerScrubStart => {
                if let Some(state) = &mut self.player {
                    state.dragging = true;
                }
            }
            Message::PlayerScrub(time) => {
                // 拖动中只更新 UI;echo(自身 value 写回)在非拖动态直接忽略。
                if let Some(state) = &mut self.player {
                    if state.dragging {
                        state.scrub = Some(time);
                    }
                }
            }
            Message::PlayerScrubCommit => {
                if let Some(state) = &mut self.player {
                    state.dragging = false;
                    if let Some(target) = state.scrub.take() {
                        if let Some(core) = self.player_core.borrow_mut().as_mut() {
                            core.seek_to(target);
                        }
                        if let Some(state) = &mut self.player {
                            state.position = target;
                            state.ended = false;
                        }
                    }
                }
            }
            Message::PlayerExit => self.exit_player(),
            Message::PlayerPrev => self.switch_track(-1),
            Message::PlayerNext => self.switch_track(1),
            Message::SubtitleMenuToggle => {
                if let Some(state) = &mut self.player {
                    state.subtitle.menu_open = !state.subtitle.menu_open;
                }
            }
            Message::SubtitleMenuClose => {
                if let Some(state) = &mut self.player {
                    state.subtitle.menu_open = false;
                }
            }
            Message::SubtitleSelect(index) => self.select_subtitle(index),
            Message::PlayerFullscreen => self.toggle_fullscreen_state(),
            Message::PlayerVideoPress => {
                let now = now_ms();
                let mut double_click = false;
                if let Some(state) = &mut self.player {
                    if now - state.last_press_ms <= 400 {
                        state.last_press_ms = 0;
                        double_click = true;
                    } else {
                        state.last_press_ms = now;
                    }
                }
                if double_click {
                    self.toggle_fullscreen_state();
                }
            }
            Message::PlayerChromeShow => {
                if let Some(state) = &mut self.player {
                    state.chrome_visible = true;
                }
                Self::arm_chrome_hide(context);
            }
            Message::PlayerChromeHide => {
                // 只在全屏且没被中途退出时隐藏。
                if let Some(state) = &mut self.player {
                    if state.fullscreen {
                        state.chrome_visible = false;
                    }
                }
            }
            Message::PlayerKey(action) => match action {
                platform::KeyAction::TogglePause => {
                    if let Some(core) = self.player_core.borrow_mut().as_mut() {
                        core.toggle_pause();
                    }
                    self.refresh_snapshot();
                }
                platform::KeyAction::Escape => {
                    if self.player.as_ref().is_some_and(|state| state.fullscreen) {
                        self.toggle_fullscreen_state();
                    } else {
                        self.exit_player();
                    }
                }
                platform::KeyAction::SeekBack => {
                    if let Some(core) = self.player_core.borrow_mut().as_mut() {
                        core.seek_by(-10.0);
                    }
                    if let Some(state) = &mut self.player {
                        state.ended = false;
                        state.scrub = None;
                    }
                }
                platform::KeyAction::SeekForward => {
                    if let Some(core) = self.player_core.borrow_mut().as_mut() {
                        core.seek_by(10.0);
                    }
                    if let Some(state) = &mut self.player {
                        state.ended = false;
                        state.scrub = None;
                    }
                }
            },
            Message::PlayerTick => {
                self.refresh_snapshot();
                self.drain_subtitle_out();
                if self.player.is_some() {
                    Self::arm_tick(context);
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
                        SlotView::new(NavigationViewItemSlot::Icon, icon::path_icon(page.icon())),
                        SlotView::new(NavigationViewItemSlot::Content, page.title()),
                    ]),
            )
        });

        // 播放中压过普通页面;返回即回浏览现场(browse 状态未动)。
        let content = if let Some(state) = &self.player {
            let path = &state.playlist[state.index];
            let title = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let error = state
                .error
                .clone()
                .or_else(|| self.video_init_error.clone());
            View::component::<PlayerPage>(PlayerInput {
                title,
                position: state.position,
                duration: state.duration,
                paused: state.paused,
                has_video: state.has_video,
                error,
                scrub: state.scrub,
                fullscreen: state.fullscreen,
                chrome_visible: state.chrome_visible,
                at_first: state.index == 0,
                at_last: state.index + 1 >= state.playlist.len(),
                subtitle_labels: state.subtitle.labels(),
                subtitle_selected: state.subtitle.selected_index(),
                subtitle_menu_open: state.subtitle.menu_open,
                overlay: state.subtitle.overlay.clone(),
                video_ref: self.video_ref.clone(),
                on_back: context.callback(|_: ()| Message::PlayerExit),
                on_toggle_pause: context.callback(|_: ()| Message::PlayerTogglePause),
                on_seek_by: context.callback(Message::PlayerSeekBy),
                on_scrub_start: context.callback(|_: ()| Message::PlayerScrubStart),
                on_scrub: context.callback(Message::PlayerScrub),
                on_scrub_commit: context.callback(|_: ()| Message::PlayerScrubCommit),
                on_prev: context.callback(|_: ()| Message::PlayerPrev),
                on_next: context.callback(|_: ()| Message::PlayerNext),
                on_fullscreen: context.callback(|_: ()| Message::PlayerFullscreen),
                on_subtitle_menu: context.callback(|_: ()| Message::SubtitleMenuToggle),
                on_subtitle_select: context.callback(Message::SubtitleSelect),
                on_close_subtitle_menu: context.callback(|_: ()| Message::SubtitleMenuClose),
                on_video_press: context.callback(|_: ()| Message::PlayerVideoPress),
                on_pointer_moved: context.callback(|_: ()| Message::PlayerChromeShow),
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
                            on_open: context.callback(Message::OpenMediaFile),
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
        let playlist = format::collect_siblings(&path);
        let Some(index) = playlist.iter().position(|p| p == &path) else {
            return;
        };
        self.player = Some(PlayState {
            playlist,
            index,
            position: 0.0,
            duration: 0.0,
            paused: false,
            has_video: false,
            ended: false,
            error: None,
            scrub: None,
            dragging: false,
            fullscreen: self.player.as_ref().is_some_and(|p| p.fullscreen),
            chrome_visible: true,
            last_press_ms: 0,
            subtitle: SubtitleState::for_media(&path),
        });
        self.open_track();
    }

    /// 切到当前 index 的曲目:重置快照,交给内核。
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
        // 字幕随文件走:外挂重新发现,自绘线程先停(不打断播放)。
        state.subtitle = SubtitleState::for_media(&path);
        self.subtitle_worker.set_source(None);
        if let Some(core) = self.player_core.borrow_mut().as_mut() {
            if let Err(error) = core.open(&path) {
                if let Some(state) = &mut self.player {
                    state.error = Some(error);
                }
            }
        } else if let Some(state) = &mut self.player {
            state.error = self.video_init_error.clone();
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

    /// 切换全屏;退出全屏顺带把顶底栏唤回。
    fn toggle_fullscreen_state(&mut self) {
        let fullscreen = platform::toggle_fullscreen();
        if let Some(state) = &mut self.player {
            state.fullscreen = fullscreen;
            state.chrome_visible = true;
        }
    }

    /// 选字幕轨(铁律②:同一时刻只开一个来源)。
    /// 内嵌 → mpv 原生渲染,自绘线程停;外挂 → sid=no,自绘线程接管。
    fn select_subtitle(&mut self, index: Option<usize>) {
        let canvas = self
            .video_host
            .borrow()
            .as_ref()
            .map(|host| host.size_px())
            .filter(|(width, height)| *width > 0 && *height > 0)
            .unwrap_or((1280, 720));
        let Some(state) = &mut self.player else {
            return;
        };
        state.subtitle.menu_open = false;
        state.subtitle.selected = index.and_then(|index| state.subtitle.choice_at(index));
        state.subtitle.overlay = None;
        let mut source = None;
        let mut sid = None;
        match &state.subtitle.selected {
            Some(SubtitleChoice::Embedded(id)) => sid = Some(*id),
            Some(SubtitleChoice::External(path)) => {
                source = state
                    .subtitle
                    .external_file(path)
                    .map(|file| (file, canvas));
            }
            None => {}
        }
        self.subtitle_worker.set_source(source);
        if let Some(core) = self.player_core.borrow_mut().as_mut() {
            core.set_sub_track(sid);
        }
    }

    /// track-list 变了:重建内嵌轨菜单。
    /// 用户没选过就如实跟随 mpv 自动选择(它默认会开第一条内嵌轨)。
    fn rebuild_subtitle_menu(&mut self, version: u64, tracks: Vec<platform::player::SubTrack>) {
        let Some(state) = &mut self.player else {
            return;
        };
        state.subtitle.tracks_seen = version;
        let embedded = tracks
            .iter()
            .map(|track| {
                let label = match (track.lang.is_empty(), track.title.is_empty()) {
                    (false, false) => format!("内嵌 {}({})", track.lang, track.title),
                    (false, true) => format!("内嵌 {}", track.lang),
                    (true, false) => format!("内嵌 {}", track.title),
                    (true, true) => format!("内嵌 #{}", track.id),
                };
                (track.id, label)
            })
            .collect();
        let keep = match &state.subtitle.selected {
            Some(SubtitleChoice::Embedded(id)) => tracks.iter().any(|track| track.id == *id),
            Some(SubtitleChoice::External(_)) => true,
            None => false,
        };
        state.subtitle.embedded = Rc::new(embedded);
        if !keep {
            state.subtitle.selected = tracks
                .iter()
                .find(|track| track.selected)
                .map(|track| SubtitleChoice::Embedded(track.id));
        }
    }

    /// 收工作线程的覆盖层产出(只认更新的版本)。
    fn drain_subtitle_out(&mut self) {
        let Some(out) = self.subtitle_worker.take_out() else {
            return;
        };
        if let Some(state) = &mut self.player {
            if state.subtitle.revision < out.revision {
                state.subtitle.revision = out.revision;
                state.subtitle.overlay = out.png.map(|png| OverlayImage(png.into()));
            }
        }
    }

    /// 退出播放:停内核、退全屏、关播放器页。
    fn exit_player(&mut self) {
        if self.player.is_none() {
            return;
        }
        if let Some(core) = self.player_core.borrow_mut().as_mut() {
            core.stop();
        }
        if self.player.as_ref().is_some_and(|state| state.fullscreen) {
            platform::toggle_fullscreen();
        }
        platform::set_key_active(false);
        self.player = None;
    }

    /// 轮询内核快照:更新进度/状态。
    fn refresh_snapshot(&mut self) {
        let core_guard = self.player_core.borrow();
        let Some(core) = core_guard.as_ref() else {
            return;
        };
        let snapshot = core.snapshot();
        // track-list 版本变了才拉内嵌轨(每 250ms 轮询,不能每次全量取)。
        let track_update = match &self.player {
            Some(state) if snapshot.tracks_version != state.subtitle.tracks_seen => {
                Some((snapshot.tracks_version, core.list_sub_tracks()))
            }
            _ => None,
        };
        drop(core_guard);
        if let Some((version, tracks)) = track_update {
            self.rebuild_subtitle_menu(version, tracks);
        }
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
            Some(target) if !state.dragging && (snapshot.position - target).abs() >= 0.75 => {
                state.position = target;
            }
            Some(_) if state.dragging => {}
            _ => {
                state.scrub = None;
                state.position = snapshot.position;
            }
        }
        // eof 自动连播:切下一个;到底则停在片尾。
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
            for _ in 0..10 {
                if token.is_cancelled() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Message::PlayerTick
        });
    }

    /// 2.5s 自续定时器:全屏鼠标移动后延时隐藏顶底栏。
    fn arm_chrome_hide(context: &ComponentContext<Self>) {
        context.spawn_background(|token| {
            for _ in 0..250 {
                if token.is_cancelled() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Message::PlayerChromeHide
        });
    }

    /// 首次进播放页时装按键钩子(幂等);回调回投 PlayerKey。
    fn ensure_key_hook(context: &ComponentContext<Self>) {
        let callback = context.sender().callback(Message::PlayerKey);
        platform::install_key_hook(Box::new(move |action| {
            let _ = callback.call(action);
        }));
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

/// 当前时刻(毫秒,UNIX 纪元)。双击判定用。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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
