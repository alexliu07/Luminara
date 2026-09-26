//! 外挂字幕自绘管线:解析 → 栅格化 RGBA → PNG 覆盖层 + 后台渲染工作线程。
//!
//! 分工(方案定稿):
//! - 内嵌轨交 mpv 原生渲染(track-list + sid 切换),不进本文件;
//! - 外挂文本(SRT/ASS/VTT)走 oxideav-subtitle/oxideav-ass 解析 + Compositor 栅格化;
//! - 外挂图形(PGS/VobSub)走 libbitsub-core;
//! - 三家收口在 [`SubtitleEngine`] trait,上层不感知具体 crate。
//!
//! 三条铁律(由 Root + PlayerCore 配合):
//! ① mpv 侧 sub-auto=no,防 mpv 自动加载外挂造成双显;
//! ② 同一时刻只开一个字幕来源(选外挂 → sid=no;选内嵌 → 停工作线程、清覆盖层);
//! ③ mpv 的 sub-color/sub-border-color 与这里默认样式一致(白字黑边)。
//!
//! 纯 Rust,不绑平台:Linux 下可直接跑单测(见 tests)。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use oxideav_scribe::{Face, FaceChain};
use oxideav_subtitle::ir::SubtitleCue;
use oxideav_subtitle::{Compositor, srt, webvtt};

use crate::model::subtitle::{self, SubtitleFile, SubtitleKind};

/// 栅格化好的一帧覆盖层(直通 alpha RGBA,画布 = 视频区像素)。
pub struct SubtitleOverlay {
    /// 稳定的 cue 编号(同一条字幕重复渲染保持不变,供去重)。
    pub cue_id: u64,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// 渲染结果:没变化就不重发(省 PNG 编码)。
pub enum CueRender {
    Unchanged,
    Update(Option<SubtitleOverlay>),
}

/// 字幕引擎:一家 crate 一个实现,上层只见这个口。
pub trait SubtitleEngine: Send {
    /// 轨内字幕条数(菜单/调试用)。
    fn cue_count(&self) -> usize;
    /// 渲染某时刻的覆盖层;无字幕时 Update(None) 表示清空。
    fn render_at(&mut self, time: f64) -> CueRender;
}

/// 打开一个外挂字幕文件 → 引擎。解析在调用线程(即工作线程)完成。
pub fn open_engine(
    file: &SubtitleFile,
    canvas: (u32, u32),
) -> Result<Box<dyn SubtitleEngine>, String> {
    match file.kind {
        SubtitleKind::Srt | SubtitleKind::Ass | SubtitleKind::Vtt => {
            Ok(Box::new(TextEngine::open(file.kind, &file.path, canvas)?))
        }
        SubtitleKind::Pgs => Ok(Box::new(PgsEngine::open(&file.path, canvas)?)),
        SubtitleKind::VobSub => Ok(Box::new(VobSubEngine::open(&file.path, canvas)?)),
    }
}

// ---------------------------------------------------------------------------
// 文本字幕:oxideav 解析 + Compositor 栅格化
// ---------------------------------------------------------------------------

struct TextEngine {
    cues: Vec<SubtitleCue>,
    comp: Compositor,
    /// 上次渲染的 cue 下标;None=当前无字幕。
    active: Option<u64>,
    /// 是否已出过首帧(首帧无论如何都出,让上层确定初始态)。
    started: bool,
}

impl TextEngine {
    fn open(kind: SubtitleKind, path: &Path, canvas: (u32, u32)) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("读字幕失败: {error}"))?;
        let track = match kind {
            SubtitleKind::Srt => srt::parse(&bytes),
            SubtitleKind::Vtt => webvtt::parse(&bytes),
            SubtitleKind::Ass => oxideav_ass::parse(&bytes),
            SubtitleKind::Pgs | SubtitleKind::VobSub => return Err("非文本字幕".to_string()),
        }
        .map_err(|error| format!("解析字幕失败: {error:?}"))?;
        let mut cues = track.cues;
        cues.sort_by_key(|cue| cue.start_us);
        let mut comp = match load_face_chain() {
            Some(chain) => Compositor::with_face(canvas.0, canvas.1, chain),
            None => Compositor::new(canvas.0, canvas.1),
        };
        // 默认样式与 mpv 内嵌字幕对齐:白字黑边、底部安全边距。
        let font_size = (canvas.1 as f32 * 0.045).max(16.0);
        comp.font_size_px = font_size;
        comp.line_height_px = (font_size * 1.35) as u32;
        comp.bottom_margin_px = (canvas.1 as f32 * 0.05) as u32;
        comp.outline_px = 2;
        comp.default_color = [255, 255, 255, 255];
        comp.outline_color = [0, 0, 0, 255];
        Ok(Self {
            cues,
            comp,
            active: None,
            started: false,
        })
    }
}

impl SubtitleEngine for TextEngine {
    fn cue_count(&self) -> usize {
        self.cues.len()
    }

    fn render_at(&mut self, time: f64) -> CueRender {
        let now_us = (time * 1_000_000.0) as i64;
        // 时间有序,二分定位后往回找仍覆盖当前时刻的条目(容忍少量重叠)。
        let end = self.cues.partition_point(|cue| cue.start_us <= now_us);
        let active = self.cues[..end]
            .iter()
            .enumerate()
            .rev()
            .take(16)
            .find(|(_, cue)| now_us < cue.end_us)
            .map(|(index, _)| index as u64);
        if active == self.active && self.started {
            return CueRender::Unchanged;
        }
        self.started = true;
        self.active = active;
        match active {
            Some(index) => {
                let cue = &self.cues[index as usize];
                let rgba = self.comp.render(cue);
                CueRender::Update(Some(SubtitleOverlay {
                    cue_id: index,
                    rgba,
                    width: self.comp.width,
                    height: self.comp.height,
                }))
            }
            None => CueRender::Update(None),
        }
    }
}

/// 系统字体链:西文在前、CJK 回退;取不到就退化为内置点阵字体(Latin-1)。
///
/// 顺序:segoeui(西文)→ 微软雅黑/黑体/宋体(CJK)。TTC 集合能不能进
/// Face::from_ttf_bytes 取决于 oxideav-scribe,失败就跳过换下一个。
fn load_face_chain() -> Option<FaceChain> {
    if let Ok(extra) = std::env::var("LUMINARA_FONT") {
        if let Some(face) = load_face(&PathBuf::from(extra)) {
            return Some(FaceChain::new(face));
        }
    }
    let fonts_dir = std::env::var("WINDIR")
        .map(|dir| PathBuf::from(dir).join("Fonts"))
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\Fonts"));
    let candidates = [
        "segoeui.ttf",
        "msyh.ttc",
        "simhei.ttf",
        "simsun.ttc",
        "msyhbd.ttc",
        "Deng.ttf",
    ];
    let mut chain: Option<FaceChain> = None;
    for name in candidates {
        if let Some(face) = load_face(&fonts_dir.join(name)) {
            chain = Some(match chain {
                Some(chain) => chain.push_fallback(face),
                None => FaceChain::new(face),
            });
        }
    }
    chain
}

fn load_face(path: &Path) -> Option<Face> {
    let bytes = std::fs::read(path).ok()?;
    Face::from_ttf_bytes(bytes).ok()
}

// ---------------------------------------------------------------------------
// 图形字幕:libbitsub-core(PGS / VobSub)——M5 实现
// ---------------------------------------------------------------------------

struct PgsEngine {
    path: PathBuf,
    canvas: (u32, u32),
}

impl PgsEngine {
    fn open(path: &Path, canvas: (u32, u32)) -> Result<Self, String> {
        Ok(Self {
            path: path.to_path_buf(),
            canvas,
        })
    }
}

impl SubtitleEngine for PgsEngine {
    fn cue_count(&self) -> usize {
        0
    }

    fn render_at(&mut self, _time: f64) -> CueRender {
        let _ = (&self.path, self.canvas);
        CueRender::Update(None)
    }
}

struct VobSubEngine {
    path: PathBuf,
    canvas: (u32, u32),
}

impl VobSubEngine {
    fn open(path: &Path, canvas: (u32, u32)) -> Result<Self, String> {
        Ok(Self {
            path: path.to_path_buf(),
            canvas,
        })
    }
}

impl SubtitleEngine for VobSubEngine {
    fn cue_count(&self) -> usize {
        0
    }

    fn render_at(&mut self, _time: f64) -> CueRender {
        let _ = (&self.path, self.canvas);
        CueRender::Update(None)
    }
}

// ---------------------------------------------------------------------------
// PNG 编码 + 后台渲染工作线程
// ---------------------------------------------------------------------------

/// RGBA 覆盖层 → PNG(Image::source_data 吃编码图)。失败返回 None。
pub fn encode_png(overlay: &SubtitleOverlay) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, overlay.width, overlay.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&overlay.rgba).ok()?;
    // finish 消耗 writer,借用到此结束,才能把 out 移出去。
    writer.finish().ok()?;
    Some(out)
}

/// 工作线程命令(最新覆盖旧:换轨只需最后一次生效)。
pub enum WorkerCmd {
    /// 载入外挂字幕源;None = 关闭(清覆盖层)。
    SetSource(Option<(SubtitleFile, (u32, u32))>),
}

/// 工作线程产出:PNG 覆盖层;None = 清空。revision 单调递增供 UI 去重。
pub struct WorkerOut {
    pub png: Option<Vec<u8>>,
    pub revision: u64,
}

/// 外挂字幕渲染工作线程:解析/栅格化全在后台,不阻塞播放。
///
/// 时间来自 mpv 事件线程写的 time-pos 原子位模式(免锁);轮询 30ms,
/// 覆盖层延迟 ≈30ms + 栅格化/编码耗时,满足 ~100ms 目标。
pub struct SubtitleWorker {
    cmd: Arc<Mutex<Option<WorkerCmd>>>,
    out: Arc<Mutex<Option<WorkerOut>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SubtitleWorker {
    pub fn new(time_bits: Arc<AtomicU64>) -> Self {
        let cmd = Arc::new(Mutex::new(None));
        let out = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let cmd = Arc::clone(&cmd);
            let out = Arc::clone(&out);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || worker_loop(cmd, out, stop, time_bits))
        };
        Self {
            cmd,
            out,
            stop,
            handle: Some(handle),
        }
    }

    /// 换字幕源(后台解析);None = 关闭。
    pub fn set_source(&self, source: Option<(SubtitleFile, (u32, u32))>) {
        *self.cmd.lock().unwrap() = Some(WorkerCmd::SetSource(source));
    }

    /// 取走最新产出(取完置空)。
    pub fn take_out(&self) -> Option<WorkerOut> {
        self.out.lock().unwrap().take()
    }
}

impl Drop for SubtitleWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    cmd: Arc<Mutex<Option<WorkerCmd>>>,
    out: Arc<Mutex<Option<WorkerOut>>>,
    stop: Arc<AtomicBool>,
    time_bits: Arc<AtomicU64>,
) {
    let mut engine: Option<Box<dyn SubtitleEngine>> = None;
    let mut queue: VecDeque<WorkerOut> = VecDeque::new();
    let mut revision: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        if let Some(WorkerCmd::SetSource(source)) = cmd.lock().unwrap().take() {
            // 换轨:丢弃旧引擎/旧缓存,先清一次覆盖层,不打断播放。
            engine = source.and_then(|(file, canvas)| open_engine(&file, canvas).ok());
            revision += 1;
            queue.push_back(WorkerOut {
                png: None,
                revision,
            });
        }
        if let Some(engine) = engine.as_mut() {
            let time = f64::from_bits(time_bits.load(Ordering::Relaxed));
            match engine.render_at(time) {
                CueRender::Update(Some(overlay)) => {
                    revision += 1;
                    let png = encode_png(&overlay);
                    queue.push_back(WorkerOut { png, revision });
                }
                CueRender::Update(None) => {
                    revision += 1;
                    queue.push_back(WorkerOut {
                        png: None,
                        revision,
                    });
                }
                CueRender::Unchanged => {}
            }
        }
        // 最新产出覆盖旧的:队列只留最后一条,UI 永远拿最新。
        if let Some(last) = queue.pop_back() {
            queue.clear();
            *out.lock().unwrap() = Some(last);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_srt() -> &'static str {
        "1\n00:00:01,000 --> 00:00:03,000\n你好\n\n2\n00:00:04,000 --> 00:00:06,000\n第二句\n"
    }

    #[test]
    fn 文本字幕解析与按时刻渲染() {
        let dir = std::env::temp_dir().join(format!("luminara-eng-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.srt");
        std::fs::write(&path, sample_srt()).unwrap();

        let file = SubtitleFile {
            path,
            label: "a".to_string(),
            kind: SubtitleKind::Srt,
        };
        let mut engine = open_engine(&file, (64, 48)).unwrap();
        assert_eq!(engine.cue_count(), 2);

        // 无字幕时刻:第一次返回清空,随后无变化。
        assert!(matches!(engine.render_at(0.5), CueRender::Update(None)));
        assert!(matches!(engine.render_at(0.6), CueRender::Unchanged));

        // cue 内:返回 RGBA 画布。
        match engine.render_at(1.5) {
            CueRender::Update(Some(overlay)) => {
                assert_eq!(overlay.rgba.len(), (64 * 48 * 4) as usize);
                // PNG 编码可用且是 PNG 魔数。
                let png = encode_png(&overlay).expect("png 编码");
                assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            }
            _ => panic!("cue 内应返回覆盖层"),
        }
        // 同一 cue 不重发。
        assert!(matches!(engine.render_at(2.0), CueRender::Unchanged));
        // cue 外 → 清空。
        assert!(matches!(engine.render_at(3.5), CueRender::Update(None)));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn 工作线程换源与产出() {
        let dir = std::env::temp_dir().join(format!("luminara-wkr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("b.srt");
        std::fs::write(&path, sample_srt()).unwrap();

        let time_bits = Arc::new(AtomicU64::new(0f64.to_bits()));
        let worker = SubtitleWorker::new(Arc::clone(&time_bits));
        worker.set_source(Some((
            SubtitleFile {
                path,
                label: "b".to_string(),
                kind: SubtitleKind::Srt,
            },
            (32, 24),
        )));
        time_bits.store(1.5f64.to_bits(), Ordering::Relaxed);
        // 等工作线程跑出产出(30ms 轮询,给足余量)。
        let mut got = None;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            if let Some(out) = worker.take_out() {
                got = Some(out);
                break;
            }
        }
        let out = got.expect("工作线程应有产出");
        assert!(out.png.is_some());
        worker.set_source(None);
        std::thread::sleep(Duration::from_millis(100));
        let cleared = worker.take_out();
        assert!(cleared.is_some_and(|out| out.png.is_none()));

        drop(worker);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
