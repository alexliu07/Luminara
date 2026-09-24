//! Media Foundation 播放内核(IMFMediaEngine)。
//!
//! 选 Media Engine 而不是 WinRT MediaPlayer 的原因:TransferVideoFrame 可以直接把帧
//! 转进我们自己交换链的后缓冲,渲染全程留在 UI 线程(SwapChainPanel 的 Rendering 回调),
//! 不用 VideoFrameAvailable 那套跨线程搬帧。解码覆盖 mp4/mov/webm/mp3/wav/aac/flac 等
//! 系统自带编解码器;mkv/avi/ogg 视机器上装的扩展而定——这里是系统能力,不是本层能许诺的。

use std::path::Path;
use std::sync::OnceLock;

use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_DXGI_MANAGER, MF_VERSION, MFCreateAttributes, MFSTARTUP_FULL, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{BSTR, implement};

use super::video::{PlatformResult, VideoHost, err_text};

/// 播放状态快照:UI 轮询取用,驱动进度条与字幕。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerSnapshot {
    /// 当前播放位置(秒)。
    pub position: f64,
    /// 总时长(秒);未就绪为 0。
    pub duration: f64,
    pub paused: bool,
    pub ended: bool,
    pub has_video: bool,
    /// 解码/来源出错。
    pub error: bool,
}

pub struct PlayerCore {
    host: Option<VideoHost>,
    engine: Option<IMFMediaEngine>,
    /// 引擎回调对象保活到进程结束(引擎可能只持弱引用)。
    _notify: Option<IMFMediaEngineNotify>,
    /// 是否有正在播的源;渲染与快照都先看它。
    active: bool,
}

impl PlayerCore {
    pub fn new() -> Self {
        Self {
            host: None,
            engine: None,
            _notify: None,
            active: false,
        }
    }

    /// 打开并自动播放一个本地文件。
    pub fn open(&mut self, path: &Path) -> PlatformResult<()> {
        self.ensure_engine()?;
        let engine = self.engine.as_ref().expect("刚初始化过");
        unsafe {
            engine.SetAutoPlay(true).map_err(err_text)?;
            engine
                .SetSource(&BSTR::from(file_uri(path)))
                .map_err(err_text)?;
        }
        self.active = true;
        Ok(())
    }

    /// 停止播放并回到片头(退出播放页时调用)。
    pub fn stop(&mut self) {
        if let Some(engine) = &self.engine {
            unsafe {
                let _ = engine.Pause();
                let _ = engine.SetCurrentTime(0.0);
            }
        }
        self.active = false;
    }

    pub fn toggle_pause(&mut self) {
        if let Some(engine) = &self.engine {
            unsafe {
                if engine.IsPaused().as_bool() {
                    let _ = engine.Play();
                } else {
                    let _ = engine.Pause();
                }
            }
        }
    }

    /// 跳到指定秒。
    pub fn seek_to(&mut self, time: f64) {
        if let Some(engine) = &self.engine {
            unsafe {
                let _ = engine.SetCurrentTime(time.max(0.0));
            }
        }
    }

    /// 快进/快退(秒,可负)。
    pub fn seek_by(&mut self, delta: f64) {
        let now = self.snapshot().position;
        self.seek_to(now + delta);
    }

    /// 轮询当前状态。非播放中返回默认值。
    pub fn snapshot(&self) -> PlayerSnapshot {
        let engine = match (&self.engine, self.active) {
            (Some(engine), true) => engine,
            _ => return PlayerSnapshot::default(),
        };
        unsafe {
            let duration = engine.GetDuration();
            PlayerSnapshot {
                position: engine.GetCurrentTime().max(0.0),
                // 加载中 GetDuration 会给出 inf/NaN,别让它混进 UI。
                duration: if duration.is_finite() && duration > 0.0 {
                    duration
                } else {
                    0.0
                },
                paused: engine.IsPaused().as_bool(),
                ended: engine.IsEnded().as_bool(),
                has_video: engine.HasVideo().as_bool(),
                error: engine
                    .GetError()
                    .map(|e| e.GetErrorCode() != 0)
                    .unwrap_or(false),
            }
        }
    }

    /// 渲染一帧:有新帧或脏标记才转帧并呈现。每收到一次面板 Rendering 调一次。
    pub fn render_frame(&mut self) {
        if !self.active {
            return;
        }
        let Some(host) = &mut self.host else {
            return;
        };
        let Some(engine) = &self.engine else {
            return;
        };
        unsafe {
            if !engine.HasVideo().as_bool() {
                return; // 纯音频:不碰交换链
            }
            // OnVideoStreamTick 只在有新帧时成功;暂停/尾帧靠脏标记补绘,防止换尺寸后残留黑块。
            if engine.OnVideoStreamTick().is_err() && !host.dirty() {
                return;
            }
            let (mut vw, mut vh) = (0u32, 0u32);
            let _ = engine.GetNativeVideoSize(Some(&mut vw), Some(&mut vh));
            host.present(engine, (vw, vh));
        }
    }

    /// 面板尺寸变化。返回 Some(交换链) 时调用方须重新 request_set_swap_chain,
    /// 接受后调用 set_attached。
    pub fn resize(&mut self, w: u32, h: u32) -> Option<windows::core::IUnknown> {
        self.ensure_host().ok()?;
        self.host.as_mut()?.resize(w, h)
    }

    pub fn set_attached(&mut self) {
        if let Some(host) = &mut self.host {
            host.set_attached();
        }
    }

    fn ensure_host(&mut self) -> PlatformResult<()> {
        if self.host.is_none() {
            self.host = Some(VideoHost::new()?);
        }
        Ok(())
    }

    fn ensure_engine(&mut self) -> PlatformResult<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        self.ensure_host()?;
        // MFStartup 按进程计数;启动一次到进程结束,不做 Shutdown(退出即回收)。
        static MF_INIT: OnceLock<PlatformResult<()>> = OnceLock::new();
        if let Err(error) = MF_INIT
            .get_or_init(|| unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(err_text) })
        {
            return Err(error.clone());
        }
        let host = self.host.as_ref().expect("刚初始化过");
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2).map_err(err_text)?;
            let attributes = attributes.ok_or_else(|| "创建媒体引擎属性失败".to_string())?;
            // 解码输出经 VideoHost 的 DXGI 设备管理器落到我们的 D3D 设备。
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, host.device_manager())
                .map_err(err_text)?;
            let notify: IMFMediaEngineNotify = EngineNotify.into();
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(err_text)?;

            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_ALL)
                    .map_err(err_text)?;
            // 首参 dwCreateFlags 保留,必须传 0。
            let engine = factory.CreateInstance(0, &attributes).map_err(err_text)?;
            self.engine = Some(engine);
            self._notify = Some(notify);
        }
        Ok(())
    }
}

/// 引擎事件回调。
///
/// 踩坑:EventNotify 从 MF 内部线程来,LocalSender 不是 Send,绝不能在这碰 UI;
/// 播放进度/结束/错误全部由 UI 线程轮询 snapshot() 获取,这里只当占位。
#[implement(IMFMediaEngineNotify)]
struct EngineNotify;

impl IMFMediaEngineNotify_Impl for EngineNotify_Impl {
    fn EventNotify(&self, _event: u32, _param1: usize, _param2: u32) -> windows::core::Result<()> {
        Ok(())
    }
}

/// 本地文件 → file:/// URI。
///
/// 踩坑:路径里的空格/非 ASCII 必须百分号编码,否则媒体引擎按 URL 解析直接拒绝来源。
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file:///");
    let path = path.to_string_lossy().replace('\\', "/");
    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(*byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}
