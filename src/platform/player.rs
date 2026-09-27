//! libmpv 播放内核封装:实例、命令、属性观察、事件线程。
//!
//! 分工:本文件只管 mpv 核(解码/音频/内嵌字幕渲染);画面经 render API 交给
//! video.rs 的 GL 上下文;UI 层只发命令、读快照,不碰 mpv 内部。
//!
//! 踩坑:
//! - libmpv 要求 LC_NUMERIC=C,Windows CRT 默认即 C,不需要 setlocale。
//! - 框架的 LocalSender 不是 Send,mpv 事件线程不能直接回投消息;事件线程只写
//!   共享快照,UI 侧沿用 Root 的自续 tick 轮询(同旧 IndexDone 模式的接力)。
//! - mpv 内核 GPLv2+:发行需随包携带 mpv-2.dll 并提供源码获取方式。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use libmpv2::events::{Event, PropertyData};
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::{Format, Mpv, mpv_end_file_reason};

use super::video::{GlLoader, gl_get_proc_address};

/// 播放状态快照:UI 轮询取用,驱动进度条、连播与字幕。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerSnapshot {
    /// 当前位置(秒)。
    pub position: f64,
    /// 总时长(秒);未就绪为 0。
    pub duration: f64,
    pub paused: bool,
    /// 播到片尾(eof-reached)。
    pub ended: bool,
    /// 有无视频流;false 时画面区显示音频占位。
    pub has_video: bool,
    /// 解码/来源出错。
    pub error: bool,
    /// 文件已加载(轨列表可查)。
    pub file_loaded: bool,
    /// 每次 file-loaded 递增;UI 用它判断是否重查轨列表。
    pub tracks_version: u64,
}

/// 内嵌字幕轨(mpv track-list 中 type=sub 的项)。
#[derive(Clone, Debug, PartialEq)]
pub struct SubTrack {
    pub id: i64,
    pub title: String,
    pub lang: String,
    pub selected: bool,
}

/// 事件线程与 UI 共享的状态。
#[derive(Default)]
struct Shared {
    snap: Mutex<PlayerSnapshot>,
    /// time-pos 的 f64 位模式,给字幕工作线程低延迟读取(免锁)。
    time_bits: Arc<AtomicU64>,
}

/// mpv 实例 + 事件线程 + 渲染上下文。
///
/// 字段顺序即 Drop 顺序:render 必须先于 mpv 释放(render 上下文引用了 mpv 核)。
pub struct PlayerCore {
    render: Option<RenderContext<'static>>,
    mpv: Mpv,
    shared: Arc<Shared>,
    shutdown: Arc<AtomicBool>,
    event_thread: Option<JoinHandle<()>>,
}

impl Drop for PlayerCore {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.event_thread.take() {
            // 事件线程 wait_event 最多 250ms 醒一次,join 不会久等。
            let _ = handle.join();
        }
        self.render = None;
    }
}

impl PlayerCore {
    /// 创建实例并起事件线程。选项按内嵌场景调好:无终端、无配置、无输入绑定。
    pub fn new() -> Result<Self, String> {
        let mpv = Mpv::with_initializer(|init| {
            // 画面走 render API,由我们自己的交换链呈现。
            init.set_option("vo", String::from("libmpv"))?;
            // 铁律①:禁止 mpv 自动加载外挂字幕,外挂一律走自绘管线,防双显。
            init.set_option("sub-auto", String::from("no"))?;
            init.set_option("audio-file-auto", String::from("no"))?;
            init.set_option("osd-level", String::from("0"))?;
            init.set_option("terminal", String::from("no"))?;
            init.set_option("input-default-bindings", String::from("no"))?;
            init.set_option("input-cursor", String::from("no"))?;
            init.set_option("input-vo-keyboard", String::from("no"))?;
            // 内嵌字幕样式对齐自绘默认样式:白字黑边。
            init.set_option("sub-color", String::from("#FFFFFFFF"))?;
            init.set_option("sub-border-color", String::from("#FF000000"))?;
            init.set_option("sub-border-size", String::from("2"))?;
            Ok(())
        })
        .map_err(|error| format!("mpv 初始化失败: {error}"))?;

        let shared = Arc::new(Shared::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let event_thread = {
            let shared = Arc::clone(&shared);
            let shutdown = Arc::clone(&shutdown);
            // 不传名字:libmpv2 6.0.0 的 Some(name) 分支有悬垂指针 bug
            // (CString::new(name)?.as_ptr() 临时对象即刻释放),会让
            // mpv_create_client 读到已释放内存并返回 NULL;None 走 null
            // 分支由 mpv 自动生成名字,无此问题。
            let client = mpv
                .create_client(None)
                .map_err(|error| format!("mpv 事件客户端创建失败: {error}"))?;
            Some(std::thread::spawn(move || {
                event_loop(client, shared, shutdown);
            }))
        };

        Ok(Self {
            render: None,
            mpv,
            shared,
            shutdown,
            event_thread,
        })
    }

    /// 当前播放位置(秒)的位模式句柄,给字幕工作线程低延迟读取。
    pub fn time_bits(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.shared.time_bits)
    }

    /// 打开并自动播放一个本地文件。
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let path = path
            .to_str()
            .ok_or_else(|| "路径不是合法 UTF-8".to_string())?;
        {
            let mut snap = self.shared.snap.lock().unwrap();
            snap.ended = false;
            snap.error = false;
            snap.file_loaded = false;
        }
        self.mpv
            .command("loadfile", &[path, "replace"])
            .map_err(|error| format!("加载文件失败: {error}"))
    }

    /// 停止播放(退出播放页/换曲前调用,防句柄泄漏)。
    pub fn stop(&mut self) {
        let _ = self.mpv.command("stop", &[]);
        let mut snap = self.shared.snap.lock().unwrap();
        *snap = PlayerSnapshot {
            paused: true,
            tracks_version: snap.tracks_version,
            ..Default::default()
        };
    }

    pub fn toggle_pause(&mut self) {
        let paused = self.shared.snap.lock().unwrap().paused;
        let _ = self.mpv.set_property("pause", !paused);
    }

    /// 跳到指定秒(绝对定位)。
    pub fn seek_to(&mut self, time: f64) {
        let target = if time.is_finite() { time.max(0.0) } else { 0.0 };
        let _ = self.mpv.command("seek", &[&target.to_string(), "absolute"]);
        // seek 后 eof-reached 自动复位;这里补一道,防连播状态残留。
        self.shared.snap.lock().unwrap().ended = false;
    }

    /// 快进/快退(秒,可负)。
    pub fn seek_by(&mut self, delta: f64) {
        let _ = self.mpv.command("seek", &[&delta.to_string(), "relative"]);
        self.shared.snap.lock().unwrap().ended = false;
    }

    /// 切换播放/暂停。
    pub fn set_paused(&mut self, paused: bool) {
        let _ = self.mpv.set_property("pause", paused);
    }

    /// 轮询当前状态。
    pub fn snapshot(&self) -> PlayerSnapshot {
        *self.shared.snap.lock().unwrap()
    }

    /// 列出内嵌字幕轨(track-list 里 type=sub 的项)。
    pub fn list_sub_tracks(&self) -> Vec<SubTrack> {
        let count: i64 = self.mpv.get_property("track-list/count").unwrap_or(0);
        let mut tracks = Vec::new();
        for index in 0..count.max(0) {
            let kind: String = self
                .mpv
                .get_property(&format!("track-list/{index}/type"))
                .unwrap_or_default();
            if kind != "sub" {
                continue;
            }
            let id: i64 = self
                .mpv
                .get_property(&format!("track-list/{index}/id"))
                .unwrap_or(-1);
            let title: String = self
                .mpv
                .get_property(&format!("track-list/{index}/title"))
                .unwrap_or_default();
            let lang: String = self
                .mpv
                .get_property(&format!("track-list/{index}/lang"))
                .unwrap_or_default();
            let selected: bool = self
                .mpv
                .get_property(&format!("track-list/{index}/selected"))
                .unwrap_or(false);
            tracks.push(SubTrack {
                id,
                title,
                lang,
                selected,
            });
        }
        tracks
    }

    /// 切内嵌字幕轨:Some(id) 开指定轨,None 关闭(sid=no)。
    /// 铁律②:同一时刻只开一个字幕来源,选内嵌前必须清掉自绘覆盖层(Root 负责)。
    pub fn set_sub_track(&mut self, id: Option<i64>) {
        let result = match id {
            Some(id) => self.mpv.set_property("sid", id),
            None => self.mpv.set_property("sid", String::from("no")),
        };
        let _ = result;
    }

    /// 挂接视频输出:在 GL 上下文就绪后创建 render 上下文。
    ///
    /// # Safety(生命周期)
    /// `RenderContext<'static>` 是把库签名里的 `RenderContext<'a>` 借命延长:
    /// mpv_render_context 内部自持核引用,真正的约束是「render 上下文先于 mpv
    /// 释放」,由本结构体的 Drop 顺序保证(render 字段先 drop)。
    pub fn init_video(&mut self, loader: GlLoader) -> Result<(), String> {
        if self.render.is_some() {
            return Ok(());
        }
        let params = [
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: gl_get_proc_address,
                ctx: loader,
            }),
        ];
        let render = self
            .mpv
            .create_render_context(params)
            .map_err(|error| format!("mpv render 上下文创建失败: {error}"))?;
        self.render = Some(unsafe {
            std::mem::transmute::<RenderContext<'_>, RenderContext<'static>>(render)
        });
        Ok(())
    }

    /// 画面是否需要强制重绘(尺寸变化/刚创建时暂停也得补一帧)。
    pub fn render_dirty(&self) -> bool {
        self.render
            .as_ref()
            .and_then(|render| render.update().ok())
            .is_some_and(|flags| flags & libmpv2::render::mpv_render_update::Frame != 0)
    }

    /// 渲染一帧到当前绑定的 FBO(由 video.rs 负责 GL 上下文与 FBO)。
    pub fn render_frame(&mut self, width: i32, height: i32) -> Result<(), String> {
        let Some(render) = self.render.as_ref() else {
            return Ok(());
        };
        render
            .render::<GlLoader>(0, width, height, false)
            .map_err(|error| format!("mpv 渲染失败: {error}"))
    }
}

/// 事件线程:wait_event 循环,把属性变化写进共享快照。
fn event_loop(client: Mpv, shared: Arc<Shared>, shutdown: Arc<AtomicBool>) {
    observe_all(&client);
    let mut snap = PlayerSnapshot::default();
    while !shutdown.load(Ordering::Relaxed) {
        let event = client.wait_event(0.25);
        let Some(event) = event else {
            continue;
        };
        let Ok(event) = event else {
            // 单条事件解析失败(如非 UTF-8 文本)不值得打断播放,跳过。
            continue;
        };
        match event {
            Event::PropertyChange { name, change, .. } => match (name, change) {
                ("time-pos", PropertyData::Double(value)) => {
                    if value.is_finite() {
                        snap.position = value;
                        shared.time_bits.store(value.to_bits(), Ordering::Relaxed);
                    }
                }
                ("duration", PropertyData::Double(value)) => {
                    snap.duration = if value.is_finite() && value > 0.0 {
                        value
                    } else {
                        0.0
                    };
                }
                ("pause", PropertyData::Flag(value)) => snap.paused = value,
                ("eof-reached", PropertyData::Flag(value)) => snap.ended = value,
                ("vid", PropertyData::Int64(value)) => snap.has_video = value > 0,
                _ => {}
            },
            Event::FileLoaded => {
                snap.file_loaded = true;
                snap.error = false;
                snap.ended = false;
                snap.tracks_version += 1;
            }
            Event::EndFile(reason) => {
                if reason == mpv_end_file_reason::Error {
                    snap.error = true;
                }
            }
            Event::Shutdown => break,
            _ => {}
        }
        *shared.snap.lock().unwrap() = snap;
    }
}

/// 观察属性:time-pos 高频订阅,其余低频。事件线程起跑时注册。
fn observe_all(client: &Mpv) {
    let _ = client.observe_property("time-pos", Format::Double, 1);
    let _ = client.observe_property("duration", Format::Double, 2);
    let _ = client.observe_property("pause", Format::Flag, 3);
    let _ = client.observe_property("eof-reached", Format::Flag, 4);
    let _ = client.observe_property("vid", Format::Int64, 5);
}

impl PlayerCore {
    /// 上一个/下一个、连播等场景用:是否有正在播的文件。
    pub fn is_loaded(&self) -> bool {
        self.shared.snap.lock().unwrap().file_loaded
    }
}
