//! 视频输出:mpv OpenGL render API → ANGLE(GL ES)FBO → 读回 → D3D11 组合交换链,
//! 经框架 ElementRef<SwapChainPanel>::request_set_swap_chain 嵌进 WinUI 视频区。
//!
//! 为什么这么走:
//! - windows-reactor 不暴露窗口句柄,CreateSwapChainForHwnd 那条路走不通;
//!   SwapChainPanel 只收组合交换链(CreateSwapChainForComposition),同旧 MF 方案。
//! - mpv 只认 OpenGL render API;Windows 桌面 WGL 拿现代 GL 上下文要先建隐藏窗口
//!   再 wglGetProcAddress,而 ANGLE(libEGL/libGLESv2,随包分发)直接给 GL ES 3 上下文。
//! - GL 与 D3D11 跨 API 共享纹理要 EGL 扩展,零拷贝链路留作性能优化;当前用
//!   glReadPixels 读回到系统内存再 UpdateSubresource 进交换链后缓冲,链路最短、
//!   语义最直白(性能余量见报告)。
//!
//! 未在 Windows 实测:ANGLE DLL 的装载、FBO 渲染、行序(见 flip_rows 注释)。

use std::ffi::{CString, c_char, c_void};
use std::ptr;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGIAdapter, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{IUnknown, Interface, PCSTR};

/// 平台层错误统一为可展示文案。
pub type PlatformResult<T> = Result<T, String>;

/// Win32/HRESULT 错误 → 文案。
pub(crate) fn err_text(error: windows::core::Error) -> String {
    error.to_string()
}

// ---------------------------------------------------------------------------
// ANGLE / GL ES 动态装载(不链接任何 GL 导入库,运行时取函数指针)
// ---------------------------------------------------------------------------

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContext = *mut c_void;
type EglSurface = *mut c_void;

const EGL_DEFAULT_DISPLAY: *mut c_void = ptr::null_mut();
const EGL_NO_CONTEXT: EglContext = ptr::null_mut();
const EGL_NONE: i32 = 0x3038;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_PBUFFER_BIT: i32 = 0x0001;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_DEPTH_SIZE: i32 = 0x3025;
const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;
const EGL_WIDTH: i32 = 0x3057;
const EGL_HEIGHT: i32 = 0x3056;
const EGL_OPENGL_ES_API: u32 = 0x30A0;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_RGBA8: u32 = 0x8058;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;

// EGL/GLES 函数指针类型(EGLAPIENTRY 在 x64 下即 system V 之外的标准约定)。
type FnGetDisplay = unsafe extern "system" fn(*mut c_void) -> EglDisplay;
type FnInitialize = unsafe extern "system" fn(EglDisplay, *mut i32, *mut i32) -> u32;
type FnBindApi = unsafe extern "system" fn(u32) -> u32;
type FnChooseConfig =
    unsafe extern "system" fn(EglDisplay, *const i32, *mut EglConfig, i32, *mut i32) -> u32;
type FnCreateContext =
    unsafe extern "system" fn(EglDisplay, EglConfig, EglContext, *const i32) -> EglContext;
type FnCreatePbuffer = unsafe extern "system" fn(EglDisplay, EglConfig, *const i32) -> EglSurface;
type FnMakeCurrent =
    unsafe extern "system" fn(EglDisplay, EglSurface, EglSurface, EglContext) -> u32;
type FnGetProc = unsafe extern "system" fn(*const c_char) -> *mut c_void;

type FnGenBuffers = unsafe extern "system" fn(i32, *mut u32);
type FnBindBuffer2 = unsafe extern "system" fn(u32, u32);
type FnRenderbufferStorage = unsafe extern "system" fn(u32, u32, i32, i32);
type FnFramebufferRenderbuffer = unsafe extern "system" fn(u32, u32, u32, u32);
type FnCheckFramebuffer = unsafe extern "system" fn(u32) -> u32;
type FnDeleteBuffers = unsafe extern "system" fn(i32, *const u32);
type FnReadPixels = unsafe extern "system" fn(i32, i32, i32, i32, u32, u32, *mut c_void);
type FnPixelStorei = unsafe extern "system" fn(u32, i32);

/// GL 装载器:库句柄 + 用到的函数指针。传给 mpv 的 get_proc_address 也用它。
#[derive(Clone, Copy)]
pub struct GlLoader {
    _egl: HMODULE,
    gles: HMODULE,
    get_proc: FnGetProc,
}

impl GlLoader {
    /// 装载 ANGLE:libEGL.dll + libGLESv2.dll(随包分发)。
    pub fn load() -> PlatformResult<Self> {
        unsafe {
            let egl = LoadLibraryW(windows::core::w!("libEGL.dll"))
                .map_err(|error| format!("装载 libEGL.dll 失败: {error}"))?;
            let gles = LoadLibraryW(windows::core::w!("libGLESv2.dll"))
                .map_err(|error| format!("装载 libGLESv2.dll 失败: {error}"))?;
            let get_proc: FnGetProc = transmute_proc(get_proc_address(egl, "eglGetProcAddress"))?;
            Ok(Self {
                _egl: egl,
                gles,
                get_proc,
            })
        }
    }
}

/// mpv render API 的 get_proc_address:eglGetProcAddress 优先,回退 libGLESv2 导出。
pub fn gl_get_proc_address(loader: &GlLoader, name: &str) -> *mut c_void {
    let Ok(c_name) = CString::new(name) else {
        return ptr::null_mut();
    };
    unsafe {
        let proc = (loader.get_proc)(c_name.as_ptr());
        if !proc.is_null() {
            return proc;
        }
        get_proc_address(loader.gles, name).map_or(ptr::null_mut(), |p| p as *mut c_void)
    }
}

/// GetProcAddress → 具体函数指针;取不到或类型不符时 None。
unsafe fn get_proc_address(module: HMODULE, name: &str) -> Option<isize> {
    let c_name = CString::new(name).ok()?;
    let proc = unsafe { GetProcAddress(module, PCSTR(c_name.as_ptr() as _))? };
    Some(proc as isize)
}

unsafe fn transmute_proc<T: Copy>(addr: Option<isize>) -> PlatformResult<T> {
    let addr = addr.ok_or_else(|| "ANGLE 导出函数缺失".to_string())?;
    Ok(unsafe { std::mem::transmute_copy(&addr) })
}

// ---------------------------------------------------------------------------
// GL 渲染面:FBO + 读回缓冲
// ---------------------------------------------------------------------------

struct GlSurface {
    display: EglDisplay,
    context: EglContext,
    surface: EglSurface,
    loader: GlLoader,
    fbo: u32,
    rbo: u32,
    size: (i32, i32),
    readback: Vec<u8>,
}

impl GlSurface {
    fn new() -> PlatformResult<Self> {
        let loader = GlLoader::load()?;
        unsafe {
            let get_display: FnGetDisplay =
                transmute_proc(get_proc_address(loader._egl, "eglGetDisplay"))?;
            let initialize: FnInitialize =
                transmute_proc(get_proc_address(loader._egl, "eglInitialize"))?;
            let bind_api: FnBindApi = transmute_proc(get_proc_address(loader._egl, "eglBindAPI"))?;
            let choose_config: FnChooseConfig =
                transmute_proc(get_proc_address(loader._egl, "eglChooseConfig"))?;
            let create_context: FnCreateContext =
                transmute_proc(get_proc_address(loader._egl, "eglCreateContext"))?;
            let create_pbuffer: FnCreatePbuffer =
                transmute_proc(get_proc_address(loader._egl, "eglCreatePbufferSurface"))?;
            let make_current: FnMakeCurrent =
                transmute_proc(get_proc_address(loader._egl, "eglMakeCurrent"))?;

            let display = get_display(EGL_DEFAULT_DISPLAY);
            if display.is_null() {
                return Err("EGL 取默认显示失败".to_string());
            }
            let (mut major, mut minor) = (0, 0);
            if initialize(display, &mut major, &mut minor) == 0 {
                return Err("EGL 初始化失败".to_string());
            }
            if bind_api(EGL_OPENGL_ES_API) == 0 {
                return Err("EGL 绑定 GL ES API 失败".to_string());
            }
            let config_attribs = [
                EGL_SURFACE_TYPE,
                EGL_PBUFFER_BIT,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES2_BIT,
                EGL_RED_SIZE,
                8,
                EGL_GREEN_SIZE,
                8,
                EGL_BLUE_SIZE,
                8,
                EGL_ALPHA_SIZE,
                8,
                EGL_DEPTH_SIZE,
                0,
                EGL_NONE,
            ];
            let mut config: EglConfig = ptr::null_mut();
            let mut count = 0;
            if choose_config(display, config_attribs.as_ptr(), &mut config, 1, &mut count) == 0
                || count == 0
            {
                return Err("EGL 选像素格式失败".to_string());
            }
            // GL ES 3.0 上下文(mpv 渲染器按 GLES2 起步,3.0 兼容其全部所需)。
            let context_attribs = [
                EGL_CONTEXT_MAJOR_VERSION,
                3,
                EGL_CONTEXT_MINOR_VERSION,
                0,
                EGL_NONE,
            ];
            let context = create_context(display, config, EGL_NO_CONTEXT, context_attribs.as_ptr());
            if context.is_null() {
                return Err("EGL 创建 GL ES 上下文失败".to_string());
            }
            // 离屏 pbuffer 只为让上下文可绑定;真正画面渲到 FBO。
            let pbuffer_attribs = [EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE];
            let surface = create_pbuffer(display, config, pbuffer_attribs.as_ptr());
            if surface.is_null() {
                return Err("EGL 创建 pbuffer 失败".to_string());
            }
            if make_current(display, surface, surface, context) == 0 {
                return Err("EGL 绑定上下文失败".to_string());
            }
            Ok(Self {
                display,
                context,
                surface,
                loader,
                fbo: 0,
                rbo: 0,
                size: (0, 0),
                readback: Vec::new(),
            })
        }
    }

    /// (重新)分配 FBO 附件到目标像素尺寸。
    fn ensure_size(&mut self, width: i32, height: i32) -> PlatformResult<()> {
        if self.size == (width, height) {
            return Ok(());
        }
        unsafe {
            let make_current: FnMakeCurrent =
                transmute_proc(get_proc_address(self.loader._egl, "eglMakeCurrent"))?;
            if make_current(self.display, self.surface, self.surface, self.context) == 0 {
                return Err("EGL 绑定上下文失败".to_string());
            }
            let gen_buffers: FnGenBuffers =
                transmute_proc(get_proc_address(self.loader.gles, "glGenFramebuffers"))?;
            let bind_fb: FnBindBuffer2 =
                transmute_proc(get_proc_address(self.loader.gles, "glBindFramebuffer"))?;
            let gen_rb: FnGenBuffers =
                transmute_proc(get_proc_address(self.loader.gles, "glGenRenderbuffers"))?;
            let bind_rb: FnBindBuffer2 =
                transmute_proc(get_proc_address(self.loader.gles, "glBindRenderbuffer"))?;
            let storage: FnRenderbufferStorage =
                transmute_proc(get_proc_address(self.loader.gles, "glRenderbufferStorage"))?;
            let attach: FnFramebufferRenderbuffer = transmute_proc(get_proc_address(
                self.loader.gles,
                "glFramebufferRenderbuffer",
            ))?;
            let check: FnCheckFramebuffer = transmute_proc(get_proc_address(
                self.loader.gles,
                "glCheckFramebufferStatus",
            ))?;
            let delete_rb: FnDeleteBuffers =
                transmute_proc(get_proc_address(self.loader.gles, "glDeleteRenderbuffers"))?;
            let delete_fb: FnDeleteBuffers =
                transmute_proc(get_proc_address(self.loader.gles, "glDeleteFramebuffers"))?;

            if self.fbo != 0 {
                delete_fb(1, &self.fbo);
                delete_rb(1, &self.rbo);
            }
            let (mut fbo, mut rbo) = (0, 0);
            gen_buffers(1, &mut fbo);
            gen_rb(1, &mut rbo);
            bind_fb(GL_FRAMEBUFFER, fbo);
            bind_rb(GL_RENDERBUFFER, rbo);
            storage(GL_RENDERBUFFER, GL_RGBA8, width, height);
            attach(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, rbo);
            if check(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                return Err("GL 帧缓冲不完整".to_string());
            }
            bind_fb(GL_FRAMEBUFFER, 0);
            self.fbo = fbo;
            self.rbo = rbo;
            self.size = (width, height);
            self.readback
                .resize(width as usize * height as usize * 4, 0);
        }
        Ok(())
    }

    /// 渲染一帧进 FBO 并读回 RGBA(行序:见 VideoHost::present 的 flip_rows)。
    fn render_and_read(
        &mut self,
        render: impl FnOnce(i32, i32) -> PlatformResult<()>,
        width: i32,
        height: i32,
    ) -> PlatformResult<&[u8]> {
        self.ensure_size(width, height)?;
        unsafe {
            let make_current: FnMakeCurrent =
                transmute_proc(get_proc_address(self.loader._egl, "eglMakeCurrent"))?;
            if make_current(self.display, self.surface, self.surface, self.context) == 0 {
                return Err("EGL 绑定上下文失败".to_string());
            }
            let bind_fb: FnBindBuffer2 =
                transmute_proc(get_proc_address(self.loader.gles, "glBindFramebuffer"))?;
            let pixel_storei: FnPixelStorei =
                transmute_proc(get_proc_address(self.loader.gles, "glPixelStorei"))?;
            let read_pixels: FnReadPixels =
                transmute_proc(get_proc_address(self.loader.gles, "glReadPixels"))?;

            bind_fb(GL_FRAMEBUFFER, self.fbo);
            render(width, height)?;
            // GL 默认 4 字节行对齐;RGBA8 行宽恒为 4 的倍数,置 1 最稳。
            pixel_storei(GL_PACK_ALIGNMENT, 1);
            read_pixels(
                0,
                0,
                width,
                height,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                self.readback.as_mut_ptr() as *mut c_void,
            );
            bind_fb(GL_FRAMEBUFFER, 0);
        }
        Ok(&self.readback)
    }
}

// ---------------------------------------------------------------------------
// D3D11 组合交换链
// ---------------------------------------------------------------------------

pub struct VideoHost {
    device: ID3D11Device,
    #[allow(dead_code)]
    context: ID3D11DeviceContext,
    factory: IDXGIFactory2,
    swapchain: Option<IDXGISwapChain1>,
    size_px: (u32, u32),
    /// 交换链新建/重建后还没挂到面板;挂上前不呈现。
    needs_attach: bool,
    /// 画面需要重绘(尺寸变化/刚创建);呈现一次后清掉。
    dirty: bool,
    gl: Option<GlSurface>,
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
            Ok(Self {
                device,
                context,
                factory,
                swapchain: None,
                size_px: (0, 0),
                needs_attach: false,
                dirty: true,
                gl: None,
            })
        }
    }

    /// 画面是否需要强制重绘(尺寸变化/刚创建)。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 初始化 GL 渲染面;返回 mpv 用的装载器。
    pub fn init_gl(&mut self) -> PlatformResult<GlLoader> {
        if self.gl.is_none() {
            self.gl = Some(GlSurface::new()?);
        }
        self.gl
            .as_ref()
            .map(|gl| gl.loader)
            .ok_or_else(|| "GL 渲染面未初始化".to_string())
    }

    /// 面板尺寸变化。返回 Some(交换链) 时调用方须重新 request_set_swap_chain,
    /// 接受后调用 set_attached。
    pub fn resize(&mut self, width: u32, height: u32) -> Option<IUnknown> {
        let (width, height) = (width.max(2), height.max(2));
        if (width, height) == self.size_px && self.swapchain.is_some() {
            return None;
        }
        // 能 ResizeBuffers 就不重建(重建要重新挂面板);失败(后缓冲引用没放干净等)再重建。
        let resized = self.swapchain.as_ref().is_some_and(|swapchain| unsafe {
            swapchain
                .ResizeBuffers(
                    0,
                    width,
                    height,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .is_ok()
        });
        self.size_px = (width, height);
        if !resized && self.recreate_swapchain().is_err() {
            return None;
        }
        self.dirty = true;
        if self.needs_attach {
            self.attach_handle()
        } else {
            None
        }
    }

    /// 交换链的 IUnknown 句柄(已 AddRef);交给 mod.rs 换皮后挂面板。
    fn attach_handle(&self) -> Option<IUnknown> {
        self.swapchain
            .as_ref()
            .and_then(|swapchain| swapchain.cast::<IUnknown>().ok())
    }

    fn recreate_swapchain(&mut self) -> PlatformResult<IUnknown> {
        unsafe {
            let (width, height) = self.size_px;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
            };
            let _ = self.swapchain.take();
            let swapchain = self
                .factory
                .CreateSwapChainForComposition(&self.device, &desc, None::<&IDXGIOutput>)
                .map_err(err_text)?;
            self.swapchain = Some(swapchain);
            self.needs_attach = true;
            self.attach_handle()
                .ok_or_else(|| "交换链句柄获取失败".to_string())
        }
    }

    pub fn set_attached(&mut self) {
        self.needs_attach = false;
    }

    /// 渲染一帧并呈现:mpv 渲到 GL FBO,读回后拷进交换链后缓冲。
    ///
    /// 行序:mpv render 的 flip=false 表示「视频顶行在 FBO 第 0 行」,glReadPixels
    /// 按 FBO 行序返回(第 0 行先出),正好是 D3D 纹理要求的顶行在前——如 Windows
    /// 上下画面颠倒,把 flip_rows 一处改成逐行倒序即可。
    pub fn present(
        &mut self,
        render: impl FnOnce(i32, i32) -> PlatformResult<()>,
    ) -> PlatformResult<()> {
        let (width, height) = (self.size_px.0 as i32, self.size_px.1 as i32);
        if width <= 0 || height <= 0 || self.needs_attach {
            return Ok(());
        }
        let Some(gl) = self.gl.as_mut() else {
            return Ok(());
        };
        let pixels: Vec<u8> = gl.render_and_read(render, width, height)?.to_vec();
        let Some(swapchain) = self.swapchain.as_ref() else {
            return Ok(());
        };
        unsafe {
            let backbuffer: ID3D11Texture2D = swapchain.GetBuffer(0).map_err(err_text)?;
            self.context.UpdateSubresource(
                Some(&ID3D11Resource::from(backbuffer.clone())),
                0,
                None,
                pixels.as_ptr() as *const c_void,
                (width * 4) as u32,
                0,
            );
            swapchain
                .Present(1, DXGI_PRESENT(0))
                .ok()
                .map_err(err_text)?;
        }
        self.dirty = false;
        Ok(())
    }
}
