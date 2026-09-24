//! D3D11 视频输出:为 Media Engine 提供设备、交换链与呈现。
//!
//! 踩坑:交换链必须走 CreateSwapChainForComposition(flip 模型),再经框架
//! ElementRef<SwapChainPanel>::request_set_swap_chain 挂到面板;
//! CreateSwapChainForHwnd 那条路走不通——windows-reactor 不暴露窗口句柄。

use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGIAdapter, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIDeviceManager, IMFMediaEngine, MFARGB, MFCreateDXGIDeviceManager,
};
use windows::core::{IUnknown, Interface};

/// 平台层错误统一为可展示文案。
pub type PlatformResult<T> = Result<T, String>;

/// Win32/HRESULT 错误 → 文案。
pub(crate) fn err_text(error: windows::core::Error) -> String {
    error.to_string()
}

pub struct VideoHost {
    device: ID3D11Device,
    /// 仅创建期使用;D3D11 设备本身线程安全,这里全在 UI 线程。
    #[allow(dead_code)]
    context: ID3D11DeviceContext,
    factory: IDXGIFactory2,
    /// 媒体引擎解码输出经它落到我们的设备上。
    device_manager: IMFDXGIDeviceManager,
    swapchain: Option<IDXGISwapChain1>,
    size_px: (u32, u32),
    /// 交换链新建/重建后还没挂到面板;挂上前不呈现。
    needs_attach: bool,
    /// 画面需要重绘(尺寸变化/刚创建)。
    dirty: bool,
}

impl VideoHost {
    pub fn new() -> PlatformResult<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE(std::ptr::null_mut()),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(err_text)?;
            let device = device.ok_or_else(|| "D3D11CreateDevice 未返回设备".to_string())?;
            let context = context.ok_or_else(|| "D3D11CreateDevice 未返回上下文".to_string())?;
            let factory = CreateDXGIFactory1::<IDXGIFactory2>().map_err(err_text)?;

            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(err_text)?;
            let device_manager = manager.ok_or_else(|| "创建 DXGI 设备管理器失败".to_string())?;
            device_manager
                .ResetDevice(&device, token)
                .map_err(err_text)?;

            Ok(Self {
                device,
                context,
                factory,
                device_manager,
                swapchain: None,
                size_px: (0, 0),
                needs_attach: false,
                dirty: true,
            })
        }
    }

    pub fn device(&self) -> &ID3D11Device {
        &self.device
    }

    pub fn device_manager(&self) -> &IMFDXGIDeviceManager {
        &self.device_manager
    }

    /// 画面是否需要强制重绘(暂停/无新帧时也得补一帧)。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 面板已接受 request_set_swap_chain,可以开始呈现。
    pub fn set_attached(&mut self) {
        self.needs_attach = false;
    }

    /// 面板尺寸变化。返回 Some(交换链句柄) 时调用方须重新 request_set_swap_chain,
    /// 成功后再 set_attached;失败留着 needs_attach,下次 Metrics 重试。
    pub fn resize(&mut self, w: u32, h: u32) -> Option<IUnknown> {
        let (w, h) = (w.max(2), h.max(2));
        if self.size_px != (w, h) {
            let resized = self.swapchain.as_ref().is_some_and(|sc| unsafe {
                sc.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
                    .is_ok()
            });
            if resized {
                self.size_px = (w, h);
                self.dirty = true;
            } else {
                // 没有交换链,或 ResizeBuffers 失败(后缓冲引用没放干净等)→ 直接重建。
                if self.create_swapchain(w, h).is_err() {
                    return None;
                }
                self.size_px = (w, h);
                self.dirty = true;
                self.needs_attach = true;
            }
        }
        if self.needs_attach {
            self.attach_handle()
        } else {
            None
        }
    }

    /// 把媒体引擎当前帧转到后缓冲并呈现;画面按原始比例信箱式居中。
    pub fn present(&mut self, engine: &IMFMediaEngine, video_size: (u32, u32)) {
        if self.needs_attach {
            return; // 还没挂上面板,呈现只会在交换链里积压
        }
        let Some(swapchain) = &self.swapchain else {
            return;
        };
        unsafe {
            let Ok(back_buffer) = swapchain.GetBuffer::<ID3D11Texture2D>(0) else {
                return;
            };
            let dst = letterbox(self.size_px, video_size);
            let border = MFARGB {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbAlpha: 255,
            };
            if engine
                .TransferVideoFrame(&back_buffer, None, &dst, Some(&border as *const MFARGB))
                .is_ok()
            {
                self.dirty = false;
            }
            // 同步间隔 1:压到垂直刷新;CompositionTarget.Rendering 本身按帧来,不会积压。
            let _ = swapchain.Present(1, DXGI_PRESENT(0));
        }
    }

    fn create_swapchain(&mut self, w: u32, h: u32) -> PlatformResult<()> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w,
            Height: h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            // SwapChainPanel 托管的固定配方:STRETCH + FLIP_SEQUENTIAL。
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let swapchain = unsafe {
            self.factory
                .CreateSwapChainForComposition(&self.device, &desc, None::<&IDXGIOutput>)
                .map_err(err_text)?
        };
        self.swapchain = Some(swapchain);
        Ok(())
    }

    /// 交换链的 IUnknown 句柄(已 AddRef);交给 mod.rs 换皮后挂面板。
    fn attach_handle(&self) -> Option<IUnknown> {
        self.swapchain
            .as_ref()
            .and_then(|sc| sc.cast::<IUnknown>().ok())
    }
}

/// 信箱式布局:视频按原始比例缩放居中,余量留黑边。
fn letterbox(panel: (u32, u32), video: (u32, u32)) -> RECT {
    let (pw, ph) = (panel.0.max(1) as f64, panel.1.max(1) as f64);
    let (vw, vh) = (video.0.max(1) as f64, video.1.max(1) as f64);
    let scale = (pw / vw).min(ph / vh);
    let (w, h) = ((vw * scale).round() as i32, (vh * scale).round() as i32);
    let (x, y) = ((pw as i32 - w) / 2, (ph as i32 - h) / 2);
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
