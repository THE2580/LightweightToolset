//! 「原生提示实验」工具：用纯 Win32 分层窗口复刻快捷键提示浮窗，用来对比原生绘制与 WebView 版本的观感。
//!
//! 这个模块刻意与 `shortcut_tip` 完全隔离：不复用它的窗口、不接它的按键链路、不改它的预设。
//! 窗口在自己的工作线程上创建并跑消息循环，因此不经过 tauri 的 `WebviewWindowBuilder`，也不参与
//! `window_service::begin_window_build` 的让路逻辑。
//!
//! 已知上限（本版就是实验品，够看观感即可）：
//! - 动画只有淡入淡出，没有 Web 版按方位（上/下/左/右）的位移与闪烁；
//! - 图标用基础图形拼出三类（键盘 / 复制 / 粘贴），不是完整的 lucide 图标集；
//! - 只出现在主显示器工作区底部居中，与现版本口径一致；
//! - 没有多屏跟随、没有 `×N` 累计的跳动动画。
//! 升级方向：需要完整图标集时构建期把 SVG 栅格化成 sprite 贴图；需要方位动画时给窗口加位移插值。

use std::sync::{
    mpsc,
    Mutex,
};
use std::time::{Duration, Instant};
use std::thread;

use serde::Deserialize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextColor, TextOutW,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE,
    HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

// 提高系统计时器精度到 1ms：默认 WM_TIMER 实际是 ~15.6ms 且会被合并，
// 动画看起来就会一顿一顿的。
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    GetSystemMetrics, KillTimer, PostMessageW, RegisterClassExW, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    HWND_TOPMOST, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SW_SHOWNOACTIVATE, SW_HIDE,
    ULW_ALPHA,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS_NAME: &str = "LightweightToolsetNativeTip";
const TIMER_ANIM: usize = 1;

/// 与 Web 版一致的展示节奏：显示 2000ms，淡入 120ms、淡出 220ms。
const DISPLAY_MS: u64 = 2000;
const FADE_IN_MS: u64 = 200;
const FADE_OUT_MS: u64 = 300;
const ANIM_TICK_MS: u32 = 8;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTipPayload {
    pub title: String,
    #[serde(default)]
    pub keys: Option<String>,
    #[serde(default = "default_count")]
    pub count: u32,
    /// `light` / `dark`，由前端解析好再传，原生侧不猜系统主题。
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 背景模糊半径（px，按 DPI 缩放前），缺省 4。
    #[serde(default)]
    pub blur_radius: Option<u32>,
    /// 卡片底色不透明度 0.4–1.0，缺省跟随主题（浅 0.92 / 深 0.78）。
    #[serde(default)]
    pub tint_alpha: Option<f32>,
    /// 动画：`fade`（整体淡入淡出）/ `slide_up`（从下往上滑入）/ `slide_down`（从上往下）。
    #[serde(default)]
    pub animation: Option<String>,
    /// 屏幕坐标（物理像素，窗口左上角）；不给就用主屏底部居中。
    #[serde(default)]
    pub position: Option<[i32; 2]>,
    /// 常驻模式：定位预览用，不自动淡出（由前端显式关闭）。
    #[serde(default)]
    pub persistent: Option<bool>,
    /// 停留时长（毫秒），缺省 2000，夹在 800–10000。
    #[serde(default)]
    pub duration_ms: Option<u32>,
    /// 键帽配色覆盖（0xRRGGBB）：底色 / 描边 / 文字；不给就用主题内置配色。
    #[serde(default)]
    pub chip_bg: Option<u32>,
    #[serde(default)]
    pub chip_border: Option<u32>,
    #[serde(default)]
    pub chip_text: Option<u32>,
    /// 强制图标：`keyboard` / `copy` / `paste`；不给就按快捷键内容推断。
    #[serde(default)]
    pub icon: Option<String>,
    /// 定位预览：直接出现、不播进入动画，且不受「定位期间压制弹出」影响。
    #[serde(default)]
    pub preview: Option<bool>,
}

/// 定位模式开关：开启期间所有普通弹出请求都被丢弃（将来接真实快捷键时同样生效）。
static POSITIONING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_positioning(active: bool) {
    POSITIONING.store(active, std::sync::atomic::Ordering::SeqCst);
}

fn positioning_active() -> bool {
    POSITIONING.load(std::sync::atomic::Ordering::SeqCst)
}

/// 显示多久后开始淡出：缺省 2 秒，夹在 0.8–10 秒。
fn payload_duration_ms(payload: &NativeTipPayload) -> u64 {
    payload.duration_ms.map(u64::from).unwrap_or(DISPLAY_MS).clamp(800, 10_000)
}

/// 缺省模糊半径 4px；上限 24，避免糊成一团。
fn payload_blur_radius(payload: &NativeTipPayload) -> u32 {
    payload.blur_radius.unwrap_or(4).min(24)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Animation {
    Fade,
    SlideUp,
    SlideDown,
}

fn payload_animation(payload: &NativeTipPayload) -> Animation {
    match payload.animation.as_deref() {
        Some("slide_up") => Animation::SlideUp,
        Some("slide_down") => Animation::SlideDown,
        _ => Animation::Fade,
    }
}

fn default_count() -> u32 {
    1
}

fn default_theme() -> String {
    "dark".to_owned()
}

struct Worker {
    hwnd: isize,
    join: thread::JoinHandle<()>,
}

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);
/// 窗口的唯一"待执行意图"：显示或隐藏。命令线程只覆盖这个槽并递增序号，
/// 窗口线程每次醒来只取最新一条——迟到的请求从机制上不可能盖掉更新的请求。
enum TiplRequest {
    Show(NativeTipPayload),
    Hide,
}

static REQUEST: Mutex<Option<(u64, TiplRequest)>> = Mutex::new(None);
static NEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static APPLIED_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 显示一次原生提示浮窗（同一次显示期间重复替换内容并重新计时）。
pub fn show(payload: NativeTipPayload) -> Result<(), String> {
    // 定位模式的边界必须闭合：
    // - 编辑中只接受预览，普通弹出全部丢弃；
    // - 已经退出编辑态后，迟到的预览请求也必须丢弃（否则会闪一下示例弹窗）。
    let is_preview = payload.preview == Some(true);
    let positioning = positioning_active();
    if is_preview && !positioning {
        return Ok(());
    }
    if positioning && !is_preview {
        return Ok(());
    }
    let hwnd = ensure_worker().ok_or_else(|| "原生提示窗口创建失败".to_owned())?;
    push_request(TiplRequest::Show(payload));
    unsafe {
        PostMessageW(Some(HWND(hwnd as *mut core::ffi::c_void)), WM_APP + 1, WPARAM(0), LPARAM(0))
            .map_err(|error| format!("原生提示窗口消息发送失败: {error}"))?;
    }
    Ok(())
}

/// 收起提示窗（定位预览关闭时用）。
///
/// 只写入"最新意图"，不等待执行：如果紧接着又有一次显示请求，它的序号更大，
/// 窗口线程只会执行更新的那一条，不会出现"迟到的 hide 把新弹窗收掉"。
pub fn hide() {
    let hwnd = WORKER.lock().ok().and_then(|worker| worker.as_ref().map(|worker| worker.hwnd));
    let Some(hwnd) = hwnd else {
        return;
    };
    push_request(TiplRequest::Hide);
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut core::ffi::c_void)), WM_APP + 1, WPARAM(0), LPARAM(0));
    }
}

fn push_request(request: TiplRequest) {
    let seq = NEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut slot) = REQUEST.lock() {
        *slot = Some((seq, request));
    }
}


/// 主屏工作区与提示窗尺寸（物理像素），供前端定位滑块算范围。
pub fn metrics() -> (i32, i32, i32, i32, i32, i32, i32, i32) {
    unsafe {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        };
        let scale = GetDpiForSystem() as f32 / 96.0;
        let (width, height) = window_size(scale);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let (left, top, right, bottom) = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let work = info.rcWork;
            (work.left, work.top, work.right, work.bottom)
        } else {
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        };
        // 默认位置与窗口自身定位逻辑保持一致（工作区底部居中 + 16px 间距）。
        let (default_x, default_y) = window_position(width, height, None);
        (left, top, right, bottom, width, height, default_x, default_y)
    }
}

/// 工具停用时结束工作线程并销毁窗口。
pub fn stop() {
    let worker = WORKER.lock().ok().and_then(|mut worker| worker.take());
    if let Some(worker) = worker {
        unsafe {
            let _ = PostMessageW(Some(HWND(worker.hwnd as *mut core::ffi::c_void)), WM_APP + 2, WPARAM(0), LPARAM(0));
        }
        let _ = worker.join.join();
    }
}

/// 确保工作线程与窗口已就绪，返回窗口句柄。
fn ensure_worker() -> Option<isize> {
    if let Ok(worker) = WORKER.lock() {
        if let Some(worker) = worker.as_ref() {
            return Some(worker.hwnd);
        }
    }
    let (tx, rx) = mpsc::channel::<isize>();
    let join = thread::Builder::new()
        .name("native-tip".to_owned())
        .spawn(move || {
            // 窗口建好之后才通知调用方，保证后续 PostMessage 不会打到不存在的窗口。
            run_message_loop(tx);
        });
    let Ok(join) = join else {
        return None;
    };
    let Ok(hwnd) = rx.recv() else {
        return None;
    };
    if hwnd == 0 {
        let _ = join.join();
        return None;
    }
    if let Ok(mut worker) = WORKER.lock() {
        *worker = Some(Worker { hwnd, join });
    }
    Some(hwnd)
}

struct Backdrop {
    // 与窗口同尺寸的屏幕快照（DIB 字节序 BGRA），只在新一次显示开始时抓，避免把自己抓进去。
    pixels: Vec<u32>,
}

struct TipState {
    hwnd: HWND,
    visible: bool,
    alpha: f32,
    /// 图标/徽标跳动的进度（0 → 1，300ms），按真实时间推进。
    pop: f32,
    phase: Phase,
    /// 当前阶段开始时刻：动画进度一律由真实时间算，缩放/帧率都不影响时长。
    phase_started: Instant,
    /// 停留截止时刻（拖到定位模式会顺延）。
    hold_deadline: Option<Instant>,
    pop_started: Instant,
    payload: Option<NativeTipPayload>,
    backdrop: Option<Backdrop>,
    /// 已渲染帧缓存：内容不变时只换整体透明度。
    frame: FrameCache,
    /// 淡入起始透明度（中途重触发时从当前值续上，避免卡在半透明）。
    fade_from: f32,
    /// 当前内容是定位预览：静态显示，不跑任何动画。
    preview: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Hidden,
    FadeIn,
    Hold,
    FadeOut,
}

impl TipState {
    fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            visible: false,
            alpha: 0.0,
            pop: 1.0,
            phase: Phase::Hidden,
            phase_started: Instant::now(),
            hold_deadline: None,
            pop_started: Instant::now(),
            payload: None,
            backdrop: None,
            frame: FrameCache {
                surface: None,
                key: None,
                last_position: None,
                last_alpha: None,
            },
            fade_from: 0.0,
            preview: false,
        }
    }

    fn start(&mut self, payload: NativeTipPayload) {
        let blur = payload_blur_radius(&payload);
        let position = payload.position;
        let persistent = payload.persistent.unwrap_or(false);
        let preview = payload.preview.unwrap_or(false);
        let now = Instant::now();
        self.payload = Some(payload);
        // 常驻模式（定位预览）永不自动收起，由前端发隐藏命令结束。
        self.hold_deadline = if persistent {
            None
        } else {
            Some(now + Duration::from_millis(payload_duration_ms(self.payload.as_ref().expect("payload set"))))
        };
        self.pop_started = now;
        // 定位预览完全静态：图标/徽标直接是最终形态，不做跳动。
        self.pop = if preview { 1.0 } else { 0.0 };
        // 只有「已经稳定显示」时才走只更新内容的分支；正在淡出/淡入中途再次触发，
        // 一律从当前透明度续上淡入——否则会卡在退出过程中的半透明状态。
        let settled = self.visible && self.phase == Phase::Hold && self.alpha >= 0.999;
        let fading_in = self.visible && self.phase == Phase::FadeIn;
        if settled {
            // 同一条提示重复触发：窗口保持不动，只更新内容、重置停留时间并播放跳动。
            // 之前这里会从头淡入，观感上像「又弹了一次」，与 Web 版不一致。
            self.phase = Phase::Hold;
        } else if fading_in && !preview {
            // 正在淡入时又来了一个请求：沿用当前进度，绝不重放动画
            // （重复请求/连续触发时会出现"弹两次"）。
            self.phase = Phase::FadeIn;
        } else if preview && self.visible {
            // 定位预览：窗口已在显示，直接改内容与位置，不播动画。
            self.phase = Phase::Hold;
            self.alpha = 1.0;
            self.fade_from = 1.0;
        } else {
            self.phase_started = now;
            if preview {
                // 定位预览直接出现，不播进入动画。
                self.fade_from = 1.0;
                self.alpha = 1.0;
                self.phase = Phase::Hold;
            } else {
                // 从当前透明度继续淡入，避免半透明卡顿/闪一下。
                self.fade_from = if self.visible { self.alpha } else { 0.0 };
                self.alpha = self.fade_from;
                self.phase = Phase::FadeIn;
            }
            // 抓背景必须发生在窗口可见之前，否则会把上一帧自己糊进画面。
            // 窗口尺寸固定，这里仍是幂等重设（建窗时用的是默认尺寸）。
            let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
            let (width, height) = window_size(scale);
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    width,
                    height,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                        | SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE,
                );
            }
            self.backdrop = capture_backdrop(self.hwnd, blur, position);
        }
        // 先把内容画进图层再显示：UpdateLayeredWindow 在隐藏状态下也会更新界面内容，
        // 反过来（先 ShowWindow 再画）会先看到上一次留下的画面（退出定位后闪一下示例）。
        self.preview = preview;
        self.paint();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
        if preview {
            // 定位预览是静态的，不需要动画定时器。
            unsafe {
                KillTimer(Some(self.hwnd), TIMER_ANIM).ok();
            }
        } else {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_ANIM, ANIM_TICK_MS, None);
            }
        }
    }

    /// 每次动画 tick 推进；返回 true 表示这一帧需要重绘。
    ///
    /// 进度全部由真实时间计算（不是按 tick 次数累加），所以窗口缩放、帧率波动都不会
    /// 改变动画时长——之前"缩放 >1 动画变慢"就是按步长累加导致的。
    fn tick(&mut self) -> bool {
        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.phase_started).as_secs_f32() * 1000.0;
        let pop_t = (now.duration_since(self.pop_started).as_secs_f32() * 1000.0 / POP_MS).clamp(0.0, 1.0);
        match self.phase {
            Phase::Hidden => false,
            Phase::FadeIn => {
                let t = (elapsed_ms / FADE_IN_MS as f32).clamp(0.0, 1.0);
                // 缓入缓出：起点和终点都慢，肉眼能看出"进入"，不会像缓出那样一瞬到位。
                let eased = t * t * (3.0 - 2.0 * t);
                self.alpha = self.fade_from + (1.0 - self.fade_from) * eased;
                // 淡入期间**不推进跳动**：内容保持不变，每帧只上传透明度，
                // 这样进入动画和退出一样"一镜到底"，不会因为中途重绘整张卡片而卡顿。
                if t >= 1.0 {
                    self.alpha = 1.0;
                    self.phase = Phase::Hold;
                    self.phase_started = now;
                    self.pop_started = now;
                }
                true
            }
            Phase::Hold => {
                // 定位预览是静态的：图标/徽标固定在最终形态，不跑跳动。
                if self.preview {
                    self.pop = 1.0;
                    return false;
                }
                self.pop = pop_t;
                let expired = self.hold_deadline.is_some_and(|deadline| now >= deadline);
                if expired {
                    self.phase = Phase::FadeOut;
                    self.phase_started = now;
                    return true;
                }
                self.pop < 1.0
            }
            Phase::FadeOut => {
                let t = (elapsed_ms / FADE_OUT_MS as f32).clamp(0.0, 1.0);
                self.alpha = (1.0 - t).powi(2);
                if t >= 1.0 {
                    self.alpha = 0.0;
                    self.phase = Phase::Hidden;
                    self.visible = false;
                    self.backdrop = None;
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                        KillTimer(Some(self.hwnd), TIMER_ANIM).ok();
                    }
                    return false;
                }
                true
            }
        }
    }

    /// 立即收起（隐藏意图执行时调用）。
    fn hide_now(&mut self) {
        self.preview = false;
        self.phase = Phase::Hidden;
        self.visible = false;
        self.alpha = 0.0;
        self.pop = 1.0;
        self.backdrop = None;
        self.hold_deadline = None;
        self.frame.key = None;
        self.frame.last_position = None;
        self.frame.last_alpha = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            KillTimer(Some(self.hwnd), TIMER_ANIM).ok();
        }
    }

    fn paint(&mut self) {
        let Some(payload) = self.payload.as_ref() else {
            return;
        };
        paint_tip(
            self.hwnd,
            payload,
            self.alpha,
            self.pop,
            self.backdrop.as_ref().map(|backdrop| backdrop.pixels.as_slice()),
            &mut self.frame,
        );
    }
}

fn run_message_loop(tx: mpsc::Sender<isize>) {
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            let _ = tx.send(0);
            return;
        };
        let class_name = wide(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // 窗口类在一个进程里只能注册一次；停用工具后再启用（线程销毁重建）时，
        // RegisterClassExW 会返回 0 并置 ERROR_CLASS_ALREADY_EXISTS。这里必须把它当成功，
        // 否则第二次 show() 直接放弃，表现为「停用后再启用就再也弹不出来」。
        if RegisterClassExW(&wc) == 0
            && windows::Win32::Foundation::GetLastError()
                != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
        {
            let _ = tx.send(0);
            return;
        }

        let scale = GetDpiForSystem() as f32 / 96.0;
        // 建窗时先用默认缩放，实际尺寸在每次显示时按 payload 重设。
        let (width, height) = window_size(scale);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(
                WS_EX_LAYERED.0
                    | WS_EX_TOOLWINDOW.0
                    | WS_EX_TOPMOST.0
                    | WS_EX_NOACTIVATE.0
                    | WS_EX_TRANSPARENT.0,
            ),
            PCWSTR(class_name.as_ptr()),
            w!("Native tip"),
            WINDOW_STYLE(WS_POPUP.0),
            0,
            0,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        );
        let Ok(hwnd) = hwnd else {
            let _ = tx.send(0);
            return;
        };
        // 实时背景每帧抓屏，必须把自己排除在捕获之外，否则会抓到自己形成拖影。
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        let state = Box::new(TipState::new(hwnd));
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            Box::into_raw(state) as isize,
        );
        let _ = tx.send(hwnd.0 as isize);

        let _ = timeBeginPeriod(1);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = timeEndPeriod(1);
    }
}

fn with_state(hwnd: HWND, f: impl FnOnce(&mut TipState)) {
    unsafe {
        let raw = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        );
        if raw == 0 {
            return;
        }
        let state = &mut *(raw as *mut TipState);
        f(state);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 窗口线程每次醒来只取"最新意图"：序号不比已执行的更新就丢弃。
    if msg == WM_APP + 1 {
        let request = REQUEST.lock().ok().and_then(|mut slot| slot.take());
        if let Some((seq, request)) = request {
            let applied = APPLIED_SEQ.load(std::sync::atomic::Ordering::SeqCst);
            if seq > applied {
                APPLIED_SEQ.store(seq, std::sync::atomic::Ordering::SeqCst);
                match request {
                    TiplRequest::Show(payload) => with_state(hwnd, |state| state.start(payload)),
                    TiplRequest::Hide => with_state(hwnd, |state| state.hide_now()),
                }
            }
        }
        return LRESULT(0);
    }
    if msg == WM_APP + 2 {
        let _ = DestroyWindow(hwnd);
        return LRESULT(0);
    }
    match msg {
        WM_TIMER => {
            if wparam.0 == TIMER_ANIM {
                let mut repaint = false;
                with_state(hwnd, |state| {
                    repaint = state.tick();
                    if state.phase == Phase::Hidden {
                        KillTimer(Some(hwnd), TIMER_ANIM).ok();
                    }
                });
                if repaint {
                    with_state(hwnd, |state| state.paint());
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 卡片高度对齐 Web 版的 56 CSS px；上下各留 9px 给投影扩散，否则阴影会被窗口边缘裁掉。
/// 内容（图标/文字/键帽/徽标）相对窗口的固定放大比例：内容跟着窗口缩放自适应，
/// 不再单独给用户配置，这个系数只保证内容比窗口略大一档。
const CONTENT_SCALE: f32 = 1.20;
const CARD_HEIGHT_CSS: f32 = 56.0;
const VERTICAL_MARGIN_CSS: f32 = 11.0;

fn window_size(scale: f32) -> (i32, i32) {
    let width = (560.0 * scale).round() as i32;
    let height = ((CARD_HEIGHT_CSS + VERTICAL_MARGIN_CSS * 2.0) * scale).round() as i32;
    (width, height)
}

struct Palette {
    card: (u8, u8, u8),
    tint_alpha: f32,
    border: (u8, u8, u8),
    border_alpha: f32,
    text: (u8, u8, u8),
    muted: (u8, u8, u8),
    chip: (u8, u8, u8),
    chip_alpha: f32,
    chip_border: (u8, u8, u8),
    chip_border_alpha: f32,
    accent: (u8, u8, u8),
    icon_bg_alpha: f32,
    badge: (u8, u8, u8),
}

/// 0xRRGGBB → (r, g, b)。
fn rgb_from_u32(value: u32) -> (u8, u8, u8) {
    (
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    )
}

fn palette(theme: &str) -> Palette {
    if theme == "light" {
        Palette {
            // 白卡 + 高不透明度覆盖磨砂底，观感接近 Web 版的 rgba(250,250,252,.95)
            card: (250, 250, 252),
            tint_alpha: 0.92,
            border: (174, 190, 216),
            border_alpha: 0.72,
            text: (23, 32, 51),
            muted: (48, 62, 88),
            // 键帽要单独够清楚：底色 16% / 描边 45% / 文字用比正文更深的蓝灰。
            chip: (108, 128, 168),
            chip_alpha: 0.16,
            chip_border: (96, 116, 156),
            chip_border_alpha: 0.45,
            accent: (47, 94, 232),
            icon_bg_alpha: 0.12,
            badge: (59, 109, 240),
        }
    } else {
        Palette {
            card: (33, 33, 36),
            tint_alpha: 0.78,
            border: (255, 255, 255),
            border_alpha: 0.12,
            text: (237, 242, 249),
            muted: (214, 224, 240),
            chip: (255, 255, 255),
            chip_alpha: 0.14,
            chip_border: (255, 255, 255),
            chip_border_alpha: 0.28,
            accent: (155, 182, 255),
            icon_bg_alpha: 0.16,
            badge: (59, 109, 240),
        }
    }
}

/// 图标 / 徽标跳动时长（与 Web 版 300ms 对齐）。
const POP_MS: f32 = 230.0;


/// 一帧：先把抓到的屏幕快照糊成磨砂底，再在像素层面合成胶囊/边框，最后用 GDI 上文字，
/// 收尾统一做圆角 + 投影的 alpha 遮罩。文字走 GDI 是为了保住 ClearType 的清晰度。
struct FrameCache {
    surface: Option<Surface>,
    /// 已渲染内容的指纹：内容不变时只更新整体透明度，不再重画。
    key: Option<u64>,
    /// 上次合成用的位置与透明度，用来跳过重复上传。
    last_position: Option<(i32, i32, i32, i32)>,
    last_alpha: Option<u8>,
}

struct Surface {
    mem_dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    width: i32,
    height: i32,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

fn ensure_surface(screen: HDC, width: i32, height: i32, cache: &mut FrameCache) -> bool {
    if cache
        .surface
        .as_ref()
        .is_some_and(|surface| surface.width == width && surface.height == height)
    {
        return true;
    }
    cache.surface = None;
    cache.key = None;
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let Ok(bitmap) = (unsafe { CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) })
    else {
        return false;
    };
    let mem_dc = unsafe { CreateCompatibleDC(Some(screen)) };
    unsafe {
        SelectObject(mem_dc, bitmap.into());
    }
    cache.surface = Some(Surface {
        mem_dc,
        bitmap,
        bits: bits as *mut u32,
        width,
        height,
    });
    true
}

/// 内容指纹：标题/快捷键/计数/主题/不透明度/尺寸/跳动进度/背景快照。
fn frame_key(
    payload: &NativeTipPayload,
    backdrop: Option<&[u32]>,
    width: i32,
    height: i32,
    pop: f32,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    payload.title.hash(&mut hasher);
    payload.keys.hash(&mut hasher);
    payload.count.hash(&mut hasher);
    payload.theme.hash(&mut hasher);
    payload.tint_alpha.map(f32::to_bits).hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    // 跳动大约 12 帧一档，足够顺滑又能省掉重复绘制。
    ((pop.clamp(0.0, 1.0) * 12.0).round() as u32).hash(&mut hasher);
    match backdrop {
        Some(pixels) => {
            pixels.len().hash(&mut hasher);
            pixels.as_ptr().hash(&mut hasher);
        }
        None => 0u8.hash(&mut hasher),
    }
    hasher.finish()
}

fn paint_tip(
    hwnd: HWND,
    payload: &NativeTipPayload,
    alpha: f32,
    pop: f32,
    backdrop: Option<&[u32]>,
    cache: &mut FrameCache,
) {
    // 只有内容（标题/计数/主题/尺寸/跳动/背景）变化时才重画整张卡片；
    // 淡入淡出阶段只换 SourceConstantAlpha，避免每帧重算遮罩和模糊导致大尺寸卡顿。
    let screen_dc = unsafe { windows::Win32::Graphics::Gdi::GetDC(None) };
    if screen_dc.is_invalid() {
        return;
    }
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let (width, height) = window_size(scale);
    if width <= 0 || height <= 0 {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
        }
        return;
    }

    let key = frame_key(payload, backdrop, width, height, pop);
    let needs_render = cache.key != Some(key);
    if needs_render && !ensure_surface(screen_dc, width, height, cache) {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
        }
        return;
    }
    let Some(surface) = cache.surface.as_mut() else {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
        }
        return;
    };
    let mem_dc = surface.mem_dc;
    if needs_render {
        let pixels = unsafe {
            std::slice::from_raw_parts_mut(surface.bits, (width * height) as usize)
        };
        pixels.fill(0);
        let pal = palette(&payload.theme);
        // 窗口尺寸固定为 1.0 缩放；内容按固定比例（1.20）比窗口略大一档。
        let ws = scale;
        let s = ws * CONTENT_SCALE;

        // 排版口径对齐 Web 版：卡片高 44、圆角 10、内边距 12、图标胶囊 24、元素间距 8。
        let card_h = CARD_HEIGHT_CSS * ws;
        let card_y = (height as f32 - card_h) / 2.0;
        let radius = 10.0 * ws;
        let pad = 12.0 * s;
        let icon_box = 24.0 * s;
        let gap = 8.0 * s;

        let title_font = make_font(14.5 * s, 600, "Microsoft YaHei UI");
        let keys_font = make_font(15.0 * s, 500, "Consolas");
        let badge_font = make_font(11.5 * s, 700, "Segoe UI");

        let title_wide = to_wide(&payload.title);
        let keys_wide = payload.keys.as_deref().map(to_wide);
        let badge = format!("\u{00d7}{}", payload.count.max(1));
        let badge_wide = to_wide(&badge);

        let title_size = measure(mem_dc, title_font, &title_wide);
        let keys_size = keys_wide.as_ref().map(|keys| measure(mem_dc, keys_font, keys));
        let badge_size = measure(mem_dc, badge_font, &badge_wide);

        let chip_pad_x = (7.0 * s).round() as i32;
        let chip_pad_y = (2.0 * s).round() as i32;
        let chip_w = keys_size
            .as_ref()
            .map(|size| size.cx + chip_pad_x * 2)
            .unwrap_or(0);
        let chip_h = keys_size
            .as_ref()
            .map(|size| size.cy + chip_pad_y * 2)
            .unwrap_or(0);
        let badge_w = ((badge_size.cx + (12.0 * s).round() as i32).max((24.0 * s).round() as i32)) as f32;
        let badge_h = 19.0 * s;

        let content_w = pad * 2.0
            + icon_box
            + gap
            + title_size.cx as f32
            + if chip_w > 0 { gap + chip_w as f32 } else { 0.0 }
            + gap
            + badge_w;
        let card_w = content_w.min(width as f32 - 8.0 * s).max(60.0 * s);
        let card_x = (width as f32 - card_w) / 2.0;
        let center_y = card_y + card_h / 2.0;

        let card = (card_x, card_y, card_w, card_h);

        // 底：屏幕快照 → 磨砂 → 主题色半透明覆盖（自绘亚克力，快照在显示那一刻抓取）。
        if let Some(backdrop) = backdrop {
            let x0 = card_x.max(0.0) as i32;
            let y0 = card_y.max(0.0) as i32;
            let x1 = ((card_x + card_w) as i32).min(width);
            let y1 = ((card_y + card_h) as i32).min(height);
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = (y * width + x) as usize;
                    if let Some(pixel) = backdrop.get(index) {
                        pixels[index] = *pixel;
                    }
                }
            }
            let tint = payload
                .tint_alpha
                .unwrap_or(pal.tint_alpha)
                .clamp(0.20, 1.0);
            fill_round_rect(pixels, width, height, card, radius, pal.card, tint);
        } else {
            fill_round_rect(pixels, width, height, card, radius, pal.card, 1.0);
        }
        stroke_round_rect(pixels, width, height, card, radius, pal.border, pal.border_alpha);

        // 图标：底色胶囊 + 图形，整体按跳动进度缩放。
        let icon_scale = pop_icon_scale(pop);
        let icon_cx = card_x + pad + icon_box / 2.0;
        draw_icon(
            pixels,
            width,
            height,
            icon_cx,
            center_y,
            icon_box,
            icon_scale,
            pal.accent,
            pal.icon_bg_alpha,
            payload,
        );

        let title_x = card_x + pad + icon_box + gap;
        draw_text_ink_centered(
            mem_dc,
            title_font,
            &title_wide,
            title_x.round() as i32,
            center_y.round() as i32,
            pal.text,
        );

        let mut cursor = title_x + title_size.cx as f32 + gap;
        if let (Some(keys), Some(_size)) = (keys_wide.as_ref(), keys_size.as_ref()) {
            let chip = (
                cursor,
                center_y - chip_h as f32 / 2.0,
                chip_w as f32,
                chip_h as f32,
            );
            // 键帽配色可配置：给了就用实色（用户自己选的色不该再被主题透明度稀释）。
            let (chip_bg, chip_bg_alpha) = match payload.chip_bg.map(rgb_from_u32) {
                Some(color) => (color, 1.0),
                None => (pal.chip, pal.chip_alpha),
            };
            let (chip_border, chip_border_alpha) = match payload.chip_border.map(rgb_from_u32) {
                Some(color) => (color, 1.0),
                None => (pal.chip_border, pal.chip_border_alpha),
            };
            let chip_text = payload.chip_text.map(rgb_from_u32).unwrap_or(pal.muted);
            fill_round_rect(pixels, width, height, chip, 7.0 * s, chip_bg, chip_bg_alpha);
            stroke_round_rect(pixels, width, height, chip, 7.0 * s, chip_border, chip_border_alpha);
            draw_text_ink_centered(
                mem_dc,
                keys_font,
                keys,
                (cursor + chip_pad_x as f32).round() as i32,
                center_y.round() as i32,
                chip_text,
            );
            cursor += chip_w as f32 + gap;
        }

        // 徽标：蓝底白字胶囊，按跳动进度缩放（布局位置仍按原尺寸，避免带动整行抖动）。
        let badge_scale = pop_badge_scale(pop);
        let badge_c = (cursor + badge_w / 2.0, center_y);
        let badge_w_scaled = badge_w * badge_scale;
        let badge_h_scaled = badge_h * badge_scale;
        let badge_rect = (
            badge_c.0 - badge_w_scaled / 2.0,
            badge_c.1 - badge_h_scaled / 2.0,
            badge_w_scaled,
            badge_h_scaled,
        );
        fill_round_rect(pixels, width, height, badge_rect, badge_h_scaled / 2.0, pal.badge, 1.0);
        draw_text_ink_centered(
            mem_dc,
            badge_font,
            &badge_wide,
            (badge_c.0 - badge_size.cx as f32 / 2.0).round() as i32,
            badge_c.1.round() as i32,
            (255, 255, 255),
        );

        #[cfg(test)]
        if let Ok(path) = std::env::var("NATIVE_TIP_DUMP") {
            let mut rgba = Vec::with_capacity(pixels.len() * 4);
            for px in pixels.iter() {
                // 遮罩之前 alpha 还是 0，这里导出的是 RGB（用于人工核对绘制结果）。
                let _ = (px >> 24) as u8;
                let a = 255u8;
                let r = ((px >> 16) & 0xFF) as u8;
                let g = ((px >> 8) & 0xFF) as u8;
                let b = (px & 0xFF) as u8;
                rgba.extend_from_slice(&[r, g, b, a]);
            }
            if let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, rgba) {
                let _ = image.save(path);
            }
        }

        // 圆角 + 投影 + 预乘 alpha。
        apply_alpha_mask(pixels, width, height, card, radius);
        unsafe {
            let _ = DeleteObject(HGDIOBJ(title_font.0));
            let _ = DeleteObject(HGDIOBJ(keys_font.0));
            let _ = DeleteObject(HGDIOBJ(badge_font.0));
        }
        cache.key = Some(key);
    }

        unsafe {
        let mut dst = POINT { x: 0, y: 0 };
        let len = SIZE { cx: width, cy: height };
        let origin = POINT { x: 0, y: 0 };
        let (base_x, base_y) = window_position(width, height, payload.position);
        let y = base_y + slide_offset(payload, alpha, scale);
        let x = base_x;
        dst.x = x;
        dst.y = y;
        let alpha255 = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        // 内容、位置、透明度都没变就不用再上传整张位图（大尺寸下这一项最省）。
        if !needs_render
            && cache.last_position == Some((x, y, width, height))
            && cache.last_alpha == Some(alpha255)
        {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
            return;
        }
        cache.last_alpha = Some(alpha255);
        let blend = windows::Win32::Graphics::Gdi::BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: alpha255,
            AlphaFormat: 1, // AC_SRC_ALPHA
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&dst),
            Some(&len),
            Some(mem_dc),
            Some(&origin),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        if cache.last_position != Some((x, y, width, height)) {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            cache.last_position = Some((x, y, width, height));
            cache.last_alpha = None;
        }
        }
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
        }
}

/// 抓窗口位置背后的屏幕内容并做模糊，作为磨砂底。必须在窗口可见时之前调用。
fn capture_backdrop(_hwnd: HWND, blur_radius: u32, position: Option<[i32; 2]>) -> Option<Backdrop> {
    unsafe {
        let screen = windows::Win32::Graphics::Gdi::GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let scale = GetDpiForSystem() as f32 / 96.0;
        let (width, height) = window_size(scale);
        if width <= 0 || height <= 0 {
            let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen);
            return None;
        }
        let (x, y) = window_position(width, height, position);
        let backdrop = capture_region(screen, x, y, width, height, blur_radius);
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen);
        backdrop
    }
}

fn capture_region(
    screen: HDC,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    blur_radius: u32,
) -> Option<Backdrop> {
    unsafe {
        use windows::Win32::Graphics::Gdi::{BitBlt, SRCCOPY};

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let mem = CreateCompatibleDC(Some(screen));
        let old = SelectObject(mem, bitmap.into());
        let _ = BitBlt(mem, 0, 0, width, height, Some(screen), x, y, SRCCOPY);
        let mut pixels = vec![0u32; (width * height) as usize];
        std::ptr::copy_nonoverlapping(bits as *const u32, pixels.as_mut_ptr(), pixels.len());
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem);
        blur_bgra(
            &mut pixels,
            width as usize,
            height as usize,
            (blur_radius as usize).min(32),
            3,
        );
        Some(Backdrop { pixels })
    }
}

/// 三遍盒式模糊近似高斯（560×56 的像素量，开销可以忽略）。
fn blur_bgra(pixels: &mut [u32], width: usize, height: usize, radius: usize, passes: usize) {
    if radius == 0 || width < 2 || height < 2 {
        return;
    }
    let mut buffer = vec![0u32; pixels.len()];
    for _ in 0..passes {
        // 横向
        for y in 0..height {
            let row = y * width;
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for x in 0..=(radius.min(width - 1)) {
                let px = pixels[row + x];
                sum[0] += px & 0xFF;
                sum[1] += (px >> 8) & 0xFF;
                sum[2] += (px >> 16) & 0xFF;
                count += 1;
            }
            for x in 0..width {
                let b = sum[0] / count;
                let g = sum[1] / count;
                let r = sum[2] / count;
                buffer[row + x] = (r << 16) | (g << 8) | b;
                let add = x + radius + 1;
                if add < width {
                    let px = pixels[row + add];
                    sum[0] += px & 0xFF;
                    sum[1] += (px >> 8) & 0xFF;
                    sum[2] += (px >> 16) & 0xFF;
                    count += 1;
                }
                let remove = x as isize - radius as isize;
                if remove >= 0 {
                    let px = pixels[row + remove as usize];
                    sum[0] -= px & 0xFF;
                    sum[1] -= (px >> 8) & 0xFF;
                    sum[2] -= (px >> 16) & 0xFF;
                    count -= 1;
                }
            }
        }
        // 纵向
        for x in 0..width {
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for y in 0..=(radius.min(height - 1)) {
                let px = buffer[y * width + x];
                sum[0] += px & 0xFF;
                sum[1] += (px >> 8) & 0xFF;
                sum[2] += (px >> 16) & 0xFF;
                count += 1;
            }
            for y in 0..height {
                let b = sum[0] / count;
                let g = sum[1] / count;
                let r = sum[2] / count;
                pixels[y * width + x] = (r << 16) | (g << 8) | b;
                let add = y + radius + 1;
                if add < height {
                    let px = buffer[add * width + x];
                    sum[0] += px & 0xFF;
                    sum[1] += (px >> 8) & 0xFF;
                    sum[2] += (px >> 16) & 0xFF;
                    count += 1;
                }
                let remove = y as isize - radius as isize;
                if remove >= 0 {
                    let px = buffer[remove as usize * width + x];
                    sum[0] -= px & 0xFF;
                    sum[1] -= (px >> 8) & 0xFF;
                    sum[2] -= (px >> 16) & 0xFF;
                    count -= 1;
                }
            }
        }
    }
}

fn pop_icon_scale(pop: f32) -> f32 {
    let t = pop.clamp(0.0, 1.0);
    if t < 0.55 {
        0.82 + (1.12 - 0.82) * (t / 0.55)
    } else {
        1.12 - (1.12 - 1.0) * ((t - 0.55) / 0.45)
    }
}

fn pop_badge_scale(pop: f32) -> f32 {
    1.28 - (1.28 - 1.0) * pop.clamp(0.0, 1.0)
}

/// 单像素 alpha 混合（预乘之前都是不透明 RGB 画布）。
fn blend_px(pixels: &mut [u32], width: i32, x: i32, y: i32, color: (u8, u8, u8), alpha: f32) {
    if x < 0 || y < 0 {
        return;
    }
    let index = (y * width + x) as usize;
    let Some(px) = pixels.get(index).copied() else {
        return;
    };
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let b = (px & 0xFF) as f32;
    let g = ((px >> 8) & 0xFF) as f32;
    let r = ((px >> 16) & 0xFF) as f32;
    let nb = b + (color.2 as f32 - b) * a;
    let ng = g + (color.1 as f32 - g) * a;
    let nr = r + (color.0 as f32 - r) * a;
    pixels[index] = ((nr.round() as u32) << 16) | ((ng.round() as u32) << 8) | nb.round() as u32;
}

/// 圆角矩形的覆盖率（0..1，1px 抗锯齿）。
fn round_coverage(x: f32, y: f32, rect: (f32, f32, f32, f32), radius: f32) -> f32 {
    let (rx, ry, rw, rh) = rect;
    if rw <= 0.0 || rh <= 0.0 {
        return 0.0;
    }
    let r = radius.min(rw / 2.0).min(rh / 2.0);
    let cx = rx + rw / 2.0;
    let cy = ry + rh / 2.0;
    let dx = ((x - cx).abs() - (rw / 2.0 - r)).max(0.0);
    let dy = ((y - cy).abs() - (rh / 2.0 - r)).max(0.0);
    let dist = (dx * dx + dy * dy).sqrt() - r;
    (0.5 - dist).clamp(0.0, 1.0)
}

fn fill_round_rect(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    rect: (f32, f32, f32, f32),
    radius: f32,
    color: (u8, u8, u8),
    alpha: f32,
) {
    let (rx, ry, rw, rh) = rect;
    let x0 = rx.floor().max(0.0) as i32;
    let y0 = ry.floor().max(0.0) as i32;
    let x1 = (rx + rw).ceil().min(width as f32) as i32;
    let y1 = (ry + rh).ceil().min(height as f32) as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let coverage = round_coverage(x as f32 + 0.5, y as f32 + 0.5, rect, radius);
            if coverage > 0.0 {
                blend_px(pixels, width, x, y, color, alpha * coverage);
            }
        }
    }
}

fn stroke_round_rect(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    rect: (f32, f32, f32, f32),
    radius: f32,
    color: (u8, u8, u8),
    alpha: f32,
) {
    let (rx, ry, rw, rh) = rect;
    let inner = (rx + 1.0, ry + 1.0, rw - 2.0, rh - 2.0);
    let x0 = rx.floor().max(0.0) as i32;
    let y0 = ry.floor().max(0.0) as i32;
    let x1 = (rx + rw).ceil().min(width as f32) as i32;
    let y1 = (ry + rh).ceil().min(height as f32) as i32;
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let outer = round_coverage(px, py, rect, radius);
            let covered = round_coverage(px, py, inner, (radius - 1.0).max(0.0));
            let ring = (outer - covered).clamp(0.0, 1.0);
            if ring > 0.0 {
                blend_px(pixels, width, x, y, color, alpha * ring);
            }
        }
    }
}

/// 图标：底色胶囊 + 图形；整体按 `scale` 缩放（跳动时用）。
///
/// 图形按 18×18 的基准盒子设计，先量出实际包围盒再居中——之前用固定左偏移，
/// 图形整体偏左了半个字宽。
#[allow(clippy::too_many_arguments)]
fn draw_icon(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    cx: f32,
    cy: f32,
    box_size: f32,
    scale: f32,
    accent: (u8, u8, u8),
    bg_alpha: f32,
    payload: &NativeTipPayload,
) {
    let size = box_size * scale;
    let rect = (cx - size / 2.0, cy - size / 2.0, size, size);
    fill_round_rect(pixels, width, height, rect, 7.0 * scale, accent, bg_alpha);

    let unit = size / 24.0;
    let line = (2.0 * unit).max(1.0);
    // 图标优先级：显式指定 > 定位预览固定键盘 > 按快捷键内容推断。
    let kind = match payload.icon.as_deref() {
        Some("keyboard") => IconKind::Keyboard,
        Some("copy") => IconKind::Copy,
        Some("paste") => IconKind::Paste,
        _ if payload.preview == Some(true) => IconKind::Keyboard,
        _ => payload.keys.as_deref().map(icon_kind).unwrap_or(IconKind::Keyboard),
    };

    // 各类图形的设计尺寸（单位 = unit），用于计算居中偏移。
    let (glyph_w, glyph_h) = match kind {
        IconKind::Keyboard => (13.0 * unit, 12.0 * unit),
        IconKind::Copy => (12.0 * unit, 12.0 * unit),
        IconKind::Paste => (11.0 * unit, 12.5 * unit),
    };
    let x0 = cx - glyph_w / 2.0;
    let y0 = cy - glyph_h / 2.0;

    match kind {
        IconKind::Keyboard => {
            for row in 0..2 {
                for col in 0..3 {
                    let x = x0 + col as f32 * 5.0 * unit;
                    let y = y0 + row as f32 * 4.5 * unit;
                    fill_round_rect(
                        pixels,
                        width,
                        height,
                        (x, y, 3.0 * unit, 3.0 * unit),
                        1.0 * unit,
                        accent,
                        1.0,
                    );
                }
            }
            fill_round_rect(
                pixels,
                width,
                height,
                (x0, y0 + 9.0 * unit, 13.0 * unit, line),
                line / 2.0,
                accent,
                1.0,
            );
        }
        IconKind::Copy => {
            fill_round_rect(
                pixels,
                width,
                height,
                (x0, y0, 9.0 * unit, 9.0 * unit),
                1.5 * unit,
                accent,
                1.0,
            );
            fill_round_rect(
                pixels,
                width,
                height,
                (x0 + 3.0 * unit, y0 + 3.0 * unit, 9.0 * unit, 9.0 * unit),
                1.5 * unit,
                accent,
                1.0,
            );
        }
        IconKind::Paste => {
            fill_round_rect(
                pixels,
                width,
                height,
                (x0, y0 + 0.5 * unit, 11.0 * unit, 12.0 * unit),
                1.5 * unit,
                accent,
                1.0,
            );
            fill_round_rect(
                pixels,
                width,
                height,
                (x0 + 3.5 * unit, y0, 4.0 * unit, 2.5 * unit),
                1.0 * unit,
                accent,
                1.0,
            );
        }
    }
}

/// 滑动动画的窗口位移：淡入阶段从偏移位置滑到最终位置，淡出阶段再滑走。
fn slide_offset(payload: &NativeTipPayload, alpha: f32, scale: f32) -> i32 {
    const DISTANCE_CSS: f32 = 20.0;
    let distance = (DISTANCE_CSS * scale).round() as i32;
    let remaining = ((1.0 - alpha.clamp(0.0, 1.0)) * distance as f32).round() as i32;
    match payload_animation(payload) {
        Animation::Fade => 0,
        Animation::SlideUp => remaining,
        Animation::SlideDown => -remaining,
    }
}

fn window_position(width: i32, height: i32, custom: Option<[i32; 2]>) -> (i32, i32) {
    if let Some([x, y]) = custom {
        return (x, y);
    }
    unsafe {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
        };
        let scale = GetDpiForSystem() as f32 / 96.0;
        let gap = (16.0 * scale).round() as i32;
        let margin = (VERTICAL_MARGIN_CSS * scale).round() as i32;
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let (left, bottom) = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            (info.rcWork.left, info.rcWork.bottom)
        } else {
            (0, GetSystemMetrics(SM_CYSCREEN))
        };
        // 卡片底边距工作区底部 16px（与 shortcut_tip 的 NOTIFY_GAP 一致）；
        // 窗口比卡片上下各多 margin，所以窗口底边要再让出 margin。
        (
            left + (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
            bottom - gap - height + margin,
        )
    }
}

/// `size_px` 是 CSS 意义上的字号（em），内部转成负的 `lfHeight`。
///
/// 传正值时 GDI 把参数当"字符格高"，渲染出来比同数值的 CSS `font-size` 小约 25%，
/// 「字号偏小」就是这个原因。
fn make_font(size_px: f32, weight: i32, face: &str) -> HFONT {
    let face = wide(face);
    let height = -(size_px.round().max(1.0) as i32);
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

fn measure(hdc: HDC, font: HFONT, text: &[u16]) -> SIZE {
    unsafe {
        let old = SelectObject(hdc, font.into());
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, text, &mut size);
        SelectObject(hdc, old);
        size
    }
}

/// 让文字的"墨迹"（而不是字符格）在给定中心线上居中。
///
/// GDI 的 `DT_VCENTER` 居中的是字符格：上升部通常比下降部高得多，数字这类只有
/// 大写字高的内容看起来会偏上。这里用 `GetTextMetricsW` 的 ascent/descent 估算
/// 大写字高，再把基线摆到正确位置。
fn ink_centered_top(hdc: HDC, font: HFONT, center_y: i32) -> i32 {
    unsafe {
        let old = SelectObject(hdc, font.into());
        let mut metrics = windows::Win32::Graphics::Gdi::TEXTMETRICW::default();
        let ok = windows::Win32::Graphics::Gdi::GetTextMetricsW(hdc, &mut metrics).as_bool();
        SelectObject(hdc, old);
        if !ok {
            return center_y;
        }
        let ascent = metrics.tmAscent as i32;
        let internal_leading = metrics.tmInternalLeading as i32;
        let cap_height = (ascent - internal_leading).max(1);
        center_y - ascent + cap_height / 2
    }
}

/// 按墨迹中心绘制单行文字（数字/中文都用这个，避免看起来偏上）。
fn draw_text_ink_centered(hdc: HDC, font: HFONT, text: &[u16], x: i32, center_y: i32, color: (u8, u8, u8)) {
    let top = ink_centered_top(hdc, font, center_y);
    unsafe {
        let old = SelectObject(hdc, font.into());
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(color));
        let _ = TextOutW(hdc, x, top, text);
        SelectObject(hdc, old);
    }
}

/// 圆角 + 投影 + 预乘 alpha。
///
/// 投影必须随距离衰减（`1 - dist/spread`）；写成 `1 + dist/spread` 会让卡片外恒等于
/// 最大不透明度，表现就是「卡片四周一整块黑底」。
fn apply_alpha_mask(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    card: (f32, f32, f32, f32),
    radius: f32,
) {
    // 窗口上下只留 9px 边距，扩散必须落在里面，否则投影被窗口边缘切掉。
    let spread = (radius * 0.75).max(5.0);
    for y in 0..height {
        for x in 0..width {
            let px = pixels[(y * width + x) as usize];
            // DIB 是小端 BGRA：低字节是蓝。
            let b8 = (px & 0xFF) as f32;
            let g8 = ((px >> 8) & 0xFF) as f32;
            let r8 = ((px >> 16) & 0xFF) as f32;

            let coverage = round_coverage(x as f32 + 0.5, y as f32 + 0.5, card, radius);
            let alpha = if coverage > 0.0 {
                coverage
            } else {
                let cx = card.0 + card.2 / 2.0;
                let cy = card.1 + card.3 / 2.0;
                let dx = ((x as f32 + 0.5 - cx).abs() - (card.2 / 2.0 - radius)).max(0.0);
                let dy = ((y as f32 + 0.5 - cy).abs() - (card.3 / 2.0 - radius)).max(0.0);
                let dist = (dx * dx + dy * dy).sqrt() - radius;
                let t = (1.0 - dist / spread).clamp(0.0, 1.0);
                (t * t * 0.30).clamp(0.0, 1.0)
            };
            if alpha <= 0.0005 {
                pixels[(y * width + x) as usize] = 0;
                continue;
            }
            let (r_out, g_out, b_out) = if coverage > 0.0 {
                (r8, g8, b8)
            } else {
                (0.0, 0.0, 0.0)
            };
            let a = (alpha * 255.0).round() as u32;
            let pr = ((r_out * alpha).round() as u32).min(255);
            let pg = ((g_out * alpha).round() as u32).min(255);
            let pb = ((b_out * alpha).round() as u32).min(255);
            pixels[(y * width + x) as usize] = (a << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }
}

enum IconKind {
    Keyboard,
    Copy,
    Paste,
}

fn icon_kind(keys: &str) -> IconKind {
    // 必须整串比较：早先用 contains("ctrl+a") 判断，`Win+Ctrl+Alt+…` 会误命中 `ctrl+a`，
    // 结果所有带 Alt 的组合都被画成复制图标。
    let chord: String = keys
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    match chord.as_str() {
        "ctrl+c" | "ctrl+x" | "ctrl+a" => IconKind::Copy,
        "ctrl+v" => IconKind::Paste,
        _ => IconKind::Keyboard,
    }
}

fn rgb(color: (u8, u8, u8)) -> COLORREF {
    COLORREF(color.0 as u32 | (color.1 as u32) << 8 | (color.2 as u32) << 16)
}

/// 带 NUL 结尾的宽字符串（给 Win32 API 用）。
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 不带 NUL 的宽字符串（给 GDI 文本测量/绘制用）。
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 手动跑的冒烟测试（默认 ignore）：确认原生浮窗真的建出来、尺寸对、可见，并截一张图到临时目录。
    /// 运行：`cargo test --lib native_tip -- --ignored --test-threads=1`
    struct FindCtx {
        pid: u32,
        found: Option<HWND>,
    }

    unsafe extern "system" fn find_cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut FindCtx);
        let mut pid = 0u32;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == ctx.pid {
            let mut buf = [0u16; 128];
            let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf);
            if len > 0 {
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                if name == CLASS_NAME {
                    ctx.found = Some(hwnd);
                    return windows::core::BOOL(0);
                }
            }
        }
        windows::core::BOOL(1)
    }

    /// 只看本进程拥有的窗口：窗口类名是每个进程各自注册的，`FindWindowW` 会连到
    /// 同样类名的其它进程窗口（开发版实例里就有一个），必须按 pid 过滤。
    fn find_own_window() -> Option<HWND> {
        let mut ctx = FindCtx {
            pid: std::process::id(),
            found: None,
        };
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(find_cb),
                LPARAM(&mut ctx as *mut FindCtx as isize),
            );
        }
        ctx.found
    }

    #[test]
    #[ignore]
    fn native_tip_renders_visible_window() {
        // 跑两轮「显示 → 停用」：窗口类在进程内只能注册一次，第二轮正好覆盖
        // 「停用工具后再启用就弹不出来」这个回归点。
        for round in 0..2 {
            show(NativeTipPayload {
                title: "关闭所有贴图窗口".to_owned(),
                keys: Some("Win+Ctrl+Alt+Shift+PageUp".to_owned()),
                count: 3,
                // 冒烟测试默认深色；NATIVE_TIP_THEME=light 时渲染浅色，方便两种主题都核对一遍。
                theme: std::env::var("NATIVE_TIP_THEME").unwrap_or_else(|_| "dark".to_owned()),
                blur_radius: None,
                tint_alpha: None,
                animation: None,
                position: None,
                persistent: None,
                duration_ms: None,
                preview: None,
                icon: None,
                chip_bg: None,
                chip_border: None,
                chip_text: None,
            })
            .expect("show 应成功");
            std::thread::sleep(Duration::from_millis(500));

            unsafe {
                let hwnd = find_own_window().unwrap_or_else(|| panic!("第 {} 轮：原生提示窗口应已创建", round + 1));
                assert!(
                    windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool(),
                    "第 {} 轮：原生提示窗口应可见",
                    round + 1
                );
                let mut rect = windows::Win32::Foundation::RECT::default();
                windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).unwrap();
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                assert!(width >= 400, "窗口宽度异常: {width}");
                assert!(height >= 40, "窗口高度异常: {height}");

                if round == 0 {
                    let path = std::env::temp_dir().join("native_tip_smoke.png");
                    let _ = capture_to_png(hwnd, &rect, &path);
                    println!("native tip screenshot: {}", path.display());
                }
            }

            stop();
            std::thread::sleep(Duration::from_millis(200));
            unsafe {
                assert!(
                    find_own_window().is_none(),
                    "第 {} 轮：停用后窗口应被销毁",
                    round + 1
                );
            }
        }
    }

    unsafe fn capture_to_png(
        hwnd: HWND,
        rect: &windows::Win32::Foundation::RECT,
        path: &std::path::Path,
    ) -> Result<(), String> {
        use windows::Win32::Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
            GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
            SRCCOPY,
        };

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bitmap.into());
        let _ = hwnd;
        BitBlt(mem, 0, 0, width, height, Some(screen), rect.left, rect.top, SRCCOPY)
            .map_err(|error| format!("BitBlt 失败: {error}"))?;

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buffer = vec![0u8; (width * height * 4) as usize];
        let scanned = GetDIBits(
            mem,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);
        if scanned == 0 {
            return Err("GetDIBits 失败".to_owned());
        }

        // BGRA -> RGBA
        for pixel in buffer.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let rgba = image::RgbaImage::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| "构造图像失败".to_owned())?;
        rgba.save(path).map_err(|error| format!("保存截图失败: {error}"))?;
        Ok(())
    }
}

