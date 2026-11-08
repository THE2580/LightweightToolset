//! 贴图窗口：把一张图钉在屏幕上。
//!
//! 覆盖层的「贴图」按钮与贴图快捷键都走这里：置顶、可拖动、拖边/角缩放、
//! 滚轮缩放、锁定、鼠标穿透，关闭时可确认，并且可以重新贴出上一次关闭的贴图。
//!
//! 窗口全部由同一条常驻线程（`screenshot-pin`）创建与维护，指令通过 channel 投递，
//! 这样多个贴图窗口共享一个消息循环，覆盖层销毁后贴图仍然存在。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::ffi::c_void;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_EXPLORER, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_ESCAPE,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Ole::{OleInitialize, OleUninitialize, DROPEFFECT_COPY};
use windows_sys::Win32::UI::Shell::SHDoDragDrop;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::dib::Surface;

/// 一张贴图的像素（自顶向下 BGRA）。底部朝上的版本按需生成，不再常驻内存。
#[derive(Clone)]
pub(crate) struct PinImage {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) top_down: Vec<u8>,
}

/// 自顶向下 → 自下而上（CF_DIB 需要），只在复制剪贴板时临时生成。
pub(crate) fn flip_rows(top_down: &[u8], width: i32, height: i32) -> Vec<u8> {
    let stride = (width * 4) as usize;
    let mut bottom_up = vec![0u8; stride * height as usize];
    for row in 0..height as usize {
        let src = (height as usize - 1 - row) * stride;
        bottom_up[row * stride..(row + 1) * stride].copy_from_slice(&top_down[src..src + stride]);
    }
    bottom_up
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PinLock {
    /// 不锁定：可拖动、可缩放。
    None,
    /// 锁定位置：不能拖动，可以缩放。
    Position,
    /// 全部锁定：既不能拖动也不能缩放。
    All,
}

impl PinLock {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "position" => Self::Position,
            "all" => Self::All,
            _ => Self::None,
        }
    }

    fn allows_move(self) -> bool {
        self == Self::None
    }

    fn allows_resize(self) -> bool {
        self != Self::All
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DoubleClick {
    Close,
    Copy,
    None,
}

impl DoubleClick {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "close" => Self::Close,
            "copy" => Self::Copy,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PinOptions {
    pub(crate) shadow: bool,
    pub(crate) confirm_close: bool,
    pub(crate) double_click: DoubleClick,
    pub(crate) lock: PinLock,
}

impl Default for PinOptions {
    fn default() -> Self {
        Self {
            shadow: true,
            confirm_close: false,
            double_click: DoubleClick::None,
            lock: PinLock::None,
        }
    }
}

/// 贴图窗口类：阴影由独立的分层窗口绘制，不再用系统 CS_DROPSHADOW。
const CLASS_PIN: &str = "LightweightToolsetPin";
const WM_PIN_COMMAND: u32 = WM_APP + 1;
const TIMER_ZOOM: usize = 1;
const TIMER_GLOW: usize = 2;
/// 阴影外扩宽度（像素）。阴影由独立的分层窗口绘制，不占贴图窗口的客户区。
const SHADOW_MARGIN: i32 = 16;
/// 首次出现时的泛光时长（毫秒）。
const GLOW_MS: u32 = 520;
const SHADOW_BLUE: (u8, u8, u8) = (0x4F, 0xA8, 0xFF);
const SHADOW_GRAY: (u8, u8, u8) = (0x8A, 0x8A, 0x8A);
const SHADOW_CLASS: &str = "LightweightToolsetPinShadowFrame\0";

// 右键菜单命令 id。
const MENU_SELECT_ALL_TEXT: usize = 1;
const MENU_TEXT_SELECTABLE: usize = 2;
const MENU_ROTATE_LEFT: usize = 10;
const MENU_ROTATE_RIGHT: usize = 11;
const MENU_FLIP_H: usize = 12;
const MENU_FLIP_V: usize = 13;
const MENU_RESET_PROCESS: usize = 14;
const MENU_COPY_IMAGE: usize = 20;
const MENU_SAVE_AS: usize = 21;
const MENU_ANNOTATE: usize = 30;
const MENU_HIDE_ANNOTATION: usize = 31;
const MENU_SHADOW: usize = 40;
const MENU_LOCK: usize = 41;
const MENU_CLOSE: usize = 50;
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 8.0;
const EDGE: i32 = 6;
const MIN_SIDE: i32 = 16;

/// 所有贴图窗口句柄，供「关闭全部 / 隐藏显示 / 鼠标穿透」使用。
static PINS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// 最近关闭的贴图（重贴用），保存图片与关闭前的位置尺寸。
static LAST_CLOSED: Mutex<VecDeque<(Arc<PinImage>, RECT)>> = Mutex::new(VecDeque::new());
/// 全部贴图窗口是否处于隐藏状态。
static HIDDEN: AtomicUsize = AtomicUsize::new(0);
/// 鼠标穿透状态。
static PASSTHROUGH: AtomicUsize = AtomicUsize::new(0);
/// 贴图线程 id，用于唤醒消息循环。
static HOST_THREAD: AtomicUsize = AtomicUsize::new(0);
static HOST_SENDER: OnceLock<mpsc::Sender<PinCommand>> = OnceLock::new();
static PIN_HWND: AtomicIsize = AtomicIsize::new(0);

enum PinCommand {
    Create {
        image: Arc<PinImage>,
        rect: RECT,
        options: PinOptions,
    },
    CloseAll,
    ToggleVisible,
    TogglePassthrough,
}

struct Drag {
    /// 屏幕坐标下的初始窗口矩形。
    start_rect: RECT,
    /// 鼠标按下时的屏幕坐标。
    start_point: POINT,
    /// 缩放的边：1 左 2 右 4 上 8 下，0 表示拖动移动。
    edges: u32,
}

struct PinState {
    /// 贴图窗口的 DIB 段：既是绘制源，也是编码 PNG / 写剪贴板时的唯一像素源。
    surface: Surface,
    /// 未经旋转/翻转的原始图，「重置所有处理」用（与调用方共享，不额外拷一份）。
    original: Arc<PinImage>,
    zoom: f32,
    lock: PinLock,
    confirm: bool,
    double_click: DoubleClick,
    drag: Option<Drag>,
    zoom_until: u32,
    /// 阴影：PixPin 同款三态（出现泛光 / 焦点淡蓝 / 失焦灰）。
    shadow: Option<PinShadow>,
    shadow_enabled: bool,
    focused: bool,
    glow_until: u32,
    /// 最近一次绘制的阴影参数，避免拖动时无谓重建位图。
    shadow_key: (i32, i32, bool, i32),
    /// 超级拖放结束后的短暂时间窗：拖放用 Esc 取消时不要让窗口一起关掉。
    escape_guard: u32,
}

/// 贴在贴图窗口正下方的一层分层阴影窗口。
struct PinShadow {
    hwnd: HWND,
    surface: Option<Surface>,
    size: (i32, i32),
}

/// 距离四边小于 `EDGE` 时返回缩放位掩码，否则返回 0（可拖动区域）。
fn hit_edges(width: i32, height: i32, x: i32, y: i32) -> u32 {
    let mut edges = 0;
    if x < EDGE {
        edges |= 1;
    }
    if x >= width - EDGE {
        edges |= 2;
    }
    if y < EDGE {
        edges |= 4;
    }
    if y >= height - EDGE {
        edges |= 8;
    }
    edges
}

/// 按被拖的边重算矩形，允许拖过对边（翻转），并保证最小尺寸。
fn resize_rect(start: RECT, edges: u32, dx: i32, dy: i32) -> RECT {
    let mut rect = start;
    if edges & 1 != 0 {
        rect.left = start.left + dx;
    }
    if edges & 2 != 0 {
        rect.right = start.right + dx;
    }
    if edges & 4 != 0 {
        rect.top = start.top + dy;
    }
    if edges & 8 != 0 {
        rect.bottom = start.bottom + dy;
    }
    // 拖过对边后交换两端，矩形跟着翻转而不是塌成一条线。
    if rect.right < rect.left {
        std::mem::swap(&mut rect.left, &mut rect.right);
    }
    if rect.bottom < rect.top {
        std::mem::swap(&mut rect.top, &mut rect.bottom);
    }
    // 最小尺寸只在各自方向上夹住，避免翻转后又缩回去。
    if rect.right - rect.left < MIN_SIDE {
        if edges & 1 != 0 {
            rect.left = rect.right - MIN_SIDE;
        } else {
            rect.right = rect.left + MIN_SIDE;
        }
    }
    if rect.bottom - rect.top < MIN_SIDE {
        if edges & 4 != 0 {
            rect.top = rect.bottom - MIN_SIDE;
        } else {
            rect.bottom = rect.top + MIN_SIDE;
        }
    }
    rect
}

fn wheel_zoom(current: f32, delta: i32) -> f32 {
    let steps = delta as f32 / 120.0;
    let next = current * 1.1f32.powf(steps);
    next.clamp(ZOOM_MIN, ZOOM_MAX)
}

fn widen(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn host() -> Option<&'static mpsc::Sender<PinCommand>> {
    if let Some(sender) = HOST_SENDER.get() {
        return Some(sender);
    }
    let (tx, rx) = mpsc::channel::<PinCommand>();
    let (ready_tx, ready_rx) = mpsc::channel::<usize>();
    let spawned = std::thread::Builder::new()
        .name("screenshot-pin".to_owned())
        .spawn(move || unsafe {
            HOST_THREAD.store(GetCurrentThreadId() as usize, Ordering::SeqCst);
            // 超级拖放走 OLE 拖放，线程必须 OleInitialize（只 CoInitializeEx 会拿到 CO_E_NOTINITIALIZED）。
            let ole = OleInitialize(std::ptr::null());
            dbg_log(&format!("pin_thread.ole_init hr=0x{:08X}", ole as u32));
            let _ = ready_tx.send(GetCurrentThreadId() as usize);
            register_class(CLASS_PIN);
            register_shadow_class();

            let mut msg: MSG = std::mem::zeroed();
            loop {
                let got = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                if got <= 0 {
                    break;
                }
                if msg.hwnd.is_null() && msg.message == WM_PIN_COMMAND {
                    drain(&rx);
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            OleUninitialize();
        })
        .is_ok();
    if !spawned {
        return None;
    }
    // 线程 id 先拿到，之后投递指令时唤醒才一定有效。
    ready_rx.recv().ok()?;
    let _ = HOST_SENDER.set(tx);
    HOST_SENDER.get()
}

unsafe fn register_class(name: &str) {
    // 类名要活到 RegisterClassW 返回之后再释放，这里由 owned 持有。
    let owned = widen(name);
    let wc = WNDCLASSW {
        style: CS_DBLCLKS,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(std::ptr::null()),
        hIcon: std::ptr::null_mut(),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: owned.as_ptr(),
    };
    RegisterClassW(&wc);
}

fn send(command: PinCommand) -> bool {
    let Some(sender) = host() else {
        return false;
    };
    if sender.send(command).is_err() {
        return false;
    }
    let thread = HOST_THREAD.load(Ordering::SeqCst);
    unsafe {
        PostThreadMessageW(thread as u32, WM_PIN_COMMAND, 0, 0);
    }
    true
}

unsafe fn drain(rx: &mpsc::Receiver<PinCommand>) {
    while let Ok(command) = rx.try_recv() {
        match command {
            PinCommand::Create {
                image,
                rect,
                options,
            } => create_window(image, rect, options),
            PinCommand::CloseAll => each_pin(|hwnd| {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }),
            PinCommand::ToggleVisible => {
                let hide = HIDDEN.load(Ordering::SeqCst) == 0;
                HIDDEN.store(if hide { 1 } else { 0 }, Ordering::SeqCst);
                each_pin(|hwnd| {
                    ShowWindow(hwnd, if hide { SW_HIDE } else { SW_SHOW });
                    if let Some(state) = state_of(hwnd) {
                        if let Some(shadow) = (*state).shadow.as_ref() {
                            ShowWindow(
                                shadow.hwnd,
                                if hide { SW_HIDE } else { SW_SHOWNOACTIVATE },
                            );
                        }
                    }
                });
            }
            PinCommand::TogglePassthrough => {
                let on = PASSTHROUGH.load(Ordering::SeqCst) == 0;
                PASSTHROUGH.store(if on { 1 } else { 0 }, Ordering::SeqCst);
                each_pin(|hwnd| set_passthrough(hwnd, on));
            }
        }
    }
}

unsafe fn each_pin(mut action: impl FnMut(HWND)) {
    let handles = PINS.lock().map(|pins| pins.clone()).unwrap_or_default();
    for hwnd in handles {
        action(hwnd as HWND);
    }
}

unsafe fn set_passthrough(hwnd: HWND, on: bool) {
    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    let next = if on {
        style | WS_EX_TRANSPARENT | WS_EX_LAYERED
    } else {
        style & !(WS_EX_TRANSPARENT | WS_EX_LAYERED)
    };
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next as isize);
}

unsafe fn create_window(image: Arc<PinImage>, rect: RECT, options: PinOptions) {
    let class = widen(CLASS_PIN);
    let title = widen("LightweightToolset Pin");
    let hinst = GetModuleHandleW(std::ptr::null());
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class.as_ptr(),
        title.as_ptr(),
        // 不直接 WS_VISIBLE：贴图是「贴在屏幕上」而不是「切换过去」，
        // 创建时不能抢走当前前台窗口的焦点。
        WS_POPUP,
        rect.left,
        rect.top,
        (rect.right - rect.left).max(MIN_SIDE),
        (rect.bottom - rect.top).max(MIN_SIDE),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinst,
        std::ptr::null_mut(),
    );
    if hwnd.is_null() {
        return;
    }
    let mut surface = match Surface::new(image.width, image.height) {
        Some(surface) => surface,
        None => {
            DestroyWindow(hwnd);
            return;
        }
    };
    // Surface 是负高度的自顶向下 DIB，行序与 top_down 一致；拷 bottom_up 会上下颠倒。
    surface.pixels_mut().copy_from_slice(&image.top_down);
    let state = Box::new(PinState {
        surface,
        original: Arc::clone(&image),
        zoom: 1.0,
        lock: options.lock,
        confirm: options.confirm_close,
        double_click: options.double_click,
        drag: None,
        zoom_until: 0,
        shadow: None,
        shadow_enabled: options.shadow,
        focused: false,
        glow_until: GetTickCount() + GLOW_MS,
        shadow_key: (0, 0, false, -1),
        escape_guard: 0,
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
    if let Ok(mut pins) = PINS.lock() {
        pins.push(hwnd as isize);
    }
    if HIDDEN.load(Ordering::SeqCst) != 0 {
        ShowWindow(hwnd, SW_HIDE);
    } else {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    if PASSTHROUGH.load(Ordering::SeqCst) != 0 {
        set_passthrough(hwnd, true);
    }
    PIN_HWND.store(hwnd as isize, Ordering::SeqCst);
    UpdateWindow(hwnd);
    // 阴影在贴图窗口之后建，才能稳定压在它下方；同时启动出现泛光。
    if let Some(state) = state_of(hwnd) {
        shadow_sync(hwnd, &mut *state);
        if (*state).shadow.is_some() {
            SetTimer(hwnd, TIMER_GLOW, 40, None);
        }
    }
}

unsafe fn state_of(hwnd: HWND) -> Option<*mut PinState> {
    let raw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PinState;
    if raw.is_null() {
        None
    } else {
        Some(raw)
    }
}

unsafe fn window_rect(hwnd: HWND) -> RECT {
    let mut rect: RECT = std::mem::zeroed();
    GetWindowRect(hwnd, &mut rect);
    rect
}

unsafe extern "system" fn shadow_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn register_shadow_class() {
    let owned = widen(SHADOW_CLASS);
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(shadow_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(std::ptr::null()),
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: owned.as_ptr(),
    };
    RegisterClassW(&wc);
}

unsafe fn shadow_destroy(state: &mut PinState) {
    if let Some(shadow) = state.shadow.take() {
        if !shadow.hwnd.is_null() {
            DestroyWindow(shadow.hwnd);
        }
    }
    state.shadow_key = (0, 0, false, -1);
}

/// 首次出现时的泛光强度：1.0 起步逐渐收回到稳定值。
fn glow_strength(remaining_ms: u32) -> f32 {
    let t = (remaining_ms as f32 / GLOW_MS as f32).clamp(0.0, 1.0);
    // 起手有一记更亮的闪光，随后平滑落下。
    1.0 + 1.1 * t * t
}

/// 把阴影参数（尺寸/颜色/泛光）画进分层窗口；参数没变时只移动窗口，不重画。
unsafe fn shadow_sync(hwnd: HWND, state: &mut PinState) {
    if !state.shadow_enabled {
        shadow_destroy(state);
        return;
    }
    let rect = window_rect(hwnd);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let now = GetTickCount();
    let glowing = now < state.glow_until;
    let blue = state.focused || glowing;
    let strength = if glowing {
        glow_strength(state.glow_until - now)
    } else {
        1.0
    };
    if state.shadow.is_none() {
        let class = widen(SHADOW_CLASS);
        let hinst = GetModuleHandleW(std::ptr::null());
        let shadow_hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        if shadow_hwnd.is_null() {
            return;
        }
        state.shadow = Some(PinShadow {
            hwnd: shadow_hwnd,
            surface: None,
            size: (0, 0),
        });
        state.shadow_key = (0, 0, false, -1);
        ShowWindow(shadow_hwnd, SW_SHOWNOACTIVATE);
    }
    let Some(shadow) = state.shadow.as_mut() else {
        return;
    };
    let outer_x = rect.left - SHADOW_MARGIN;
    let outer_y = rect.top - SHADOW_MARGIN;
    let outer_w = width + SHADOW_MARGIN * 2;
    let outer_h = height + SHADOW_MARGIN * 2;
    // 阴影窗口压在贴图窗口正下方，贴图窗口本身保持最上层。
    SetWindowPos(
        shadow.hwnd,
        hwnd,
        outer_x,
        outer_y,
        outer_w,
        outer_h,
        SWP_NOACTIVATE,
    );
    let key = (width, height, blue, (strength * 100.0) as i32);
    if state.shadow_key == key {
        return;
    }
    state.shadow_key = key;
    render_shadow(shadow, width, height, outer_x, outer_y, blue, strength);
}

unsafe fn render_shadow(
    shadow: &mut PinShadow,
    image_w: i32,
    image_h: i32,
    dst_x: i32,
    dst_y: i32,
    blue: bool,
    strength: f32,
) {
    let w = image_w + SHADOW_MARGIN * 2;
    let h = image_h + SHADOW_MARGIN * 2;
    if shadow.surface.is_none() || shadow.size != (w, h) {
        shadow.surface = Surface::new(w, h);
        shadow.size = (w, h);
    }
    let Some(surface) = shadow.surface.as_mut() else {
        return;
    };
    let m = SHADOW_MARGIN;
    let color = if blue { SHADOW_BLUE } else { SHADOW_GRAY };
    {
        let bytes = surface.pixels_mut();
        bytes.fill(0);
        // 距离 0..=m 的透明度表，平方衰减更接近柔和投影。
        let mut alpha = [0u8; (SHADOW_MARGIN as usize) + 1];
        for (d, value) in alpha.iter_mut().enumerate() {
            let t = 1.0 - d as f32 / m as f32;
            *value = (t * t * 165.0 * strength).min(255.0) as u8;
        }
        for y in 0..h {
            let dy = if y < m {
                m - y
            } else if y >= m + image_h {
                y - (m + image_h) + 1
            } else {
                0
            };
            for x in 0..w {
                let dx = if x < m {
                    m - x
                } else if x >= m + image_w {
                    x - (m + image_w) + 1
                } else {
                    0
                };
                let d = dx.max(dy).min(m) as usize;
                let a = alpha[d];
                if d == 0 || a == 0 {
                    continue;
                }
                let idx = ((y * w + x) * 4) as usize;
                // UpdateLayeredWindow 要预乘 alpha 的 BGRA。
                bytes[idx] = (color.2 as u16 * a as u16 / 255) as u8;
                bytes[idx + 1] = (color.1 as u16 * a as u16 / 255) as u8;
                bytes[idx + 2] = (color.0 as u16 * a as u16 / 255) as u8;
                bytes[idx + 3] = a;
            }
        }
    }
    let screen = GetDC(std::ptr::null_mut());
    if screen.is_null() {
        return;
    }
    let dst = POINT { x: dst_x, y: dst_y };
    let src = POINT { x: 0, y: 0 };
    let size = SIZE { cx: w, cy: h };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    UpdateLayeredWindow(
        shadow.hwnd,
        screen,
        &dst,
        &size,
        surface.dc(),
        &src,
        0,
        &blend,
        ULW_ALPHA,
    );
    ReleaseDC(std::ptr::null_mut(), screen);
}

/// 关闭前确认（设置项），返回是否继续关闭。
unsafe fn confirm_close(hwnd: HWND, state: &PinState) -> bool {
    if !state.confirm {
        return true;
    }
    let text = widen("关闭这张贴图？");
    let caption = widen("轻量化工具集");
    MessageBoxW(
        hwnd,
        text.as_ptr(),
        caption.as_ptr(),
        MB_OKCANCEL | MB_ICONQUESTION,
    ) == IDOK
}

unsafe fn copy_to_clipboard(state: &PinState) {
    let width = state.surface.width;
    let height = state.surface.height;
    let png = super::save::encode_png(state.surface.pixels(), width as u32, height as u32).ok();
    // 底部朝上的副本只在写 CF_DIB 时临时生成。
    let bottom_up = flip_rows(state.surface.pixels(), width, height);
    let _ = super::clipboard_image::copy_image(width, height, &bottom_up, png.as_deref());
}

/// 超级拖放用的临时目录：拖出去的图以 PNG 文件形式交给目标程序。
fn drag_dir() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("LightweightToolset")
        .join("pin-drag")
}

/// 把当前贴图写成临时 PNG（保留最近 20 个），返回路径。
unsafe fn write_drag_file(state: &PinState) -> Option<std::path::PathBuf> {
    let dir = drag_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("Pin_{stamp}.png"));
    let png = super::save::encode_png(
        state.surface.pixels(),
        state.surface.width as u32,
        state.surface.height as u32,
    )
    .ok()?;
    std::fs::write(&path, png).ok()?;
    // 只保留最近的 20 个，避免临时目录无限增长。
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
            .collect();
        if files.len() > 20 {
            files.sort();
            let remove = files.len() - 20;
            for old in files.into_iter().take(remove) {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Some(path)
}

/// 只为开发版留的拖放诊断日志：写 `%TEMP%\lwt-supdrag.log`，正式构建不编译。
fn dbg_log(message: &str) {
    if !cfg!(debug_assertions) {
        return;
    }
    use std::io::Write;
    let path = std::env::temp_dir().join("lwt-supdrag.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

/// 超级拖放的数据对象：同时提供「图片位图」和「PNG 文件」两种格式。
///
/// 只给文件时，很多网页上传区（只吃位图）会拒收，表现就是「拖了没反应」；
/// PixPin 也是位图 + 文件双格式，这里对齐。
#[windows::core::implement(windows::Win32::System::Com::IDataObject)]
struct PinDataObject {
    /// 自顶向下 BGRA。
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    /// 临时 PNG 的宽字符路径（含结尾 0）。
    file: Vec<u16>,
}

/// IDragSourceHelper 通过 SetData 塞进来的拖动图像（cfFormat, tymed, HGLOBAL）。
/// 放在静态里，拖放结束后好统一释放，不然每次拖放都会漏一块位图内存。
static DRAG_IMAGE_MEDIUM: Mutex<Option<(u16, u32, isize)>> = Mutex::new(None);

const CF_DIB: u16 = 8;
const CF_HDROP: u16 = 15;

fn hglobal_from_bytes(bytes: &[u8]) -> Option<windows::Win32::Foundation::HGLOBAL> {
    unsafe {
        let handle = windows_sys::Win32::System::Memory::GlobalAlloc(
            windows_sys::Win32::System::Memory::GMEM_MOVEABLE,
            bytes.len(),
        );
        if handle.is_null() {
            return None;
        }
        let pointer = windows_sys::Win32::System::Memory::GlobalLock(handle);
        if pointer.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer as *mut u8, bytes.len());
        windows_sys::Win32::System::Memory::GlobalUnlock(handle);
        Some(windows::Win32::Foundation::HGLOBAL(handle))
    }
}

unsafe fn medium_from_bytes(
    bytes: &[u8],
) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
    use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
    let Some(handle) = hglobal_from_bytes(bytes) else {
        return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
    };
    Ok(STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: handle },
        pUnkForRelease: std::mem::ManuallyDrop::new(None),
    })
}

impl PinDataObject {
    /// CF_DIB：BITMAPINFOHEADER + 自下而上的 32 位像素。
    fn dib_bytes(&self) -> Vec<u8> {
        use std::mem::size_of;
        let header_size = size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>();
        let width = self.width.max(0) as u32;
        let height = self.height.max(0) as u32;
        let image_size = width * height * 4;
        let mut bytes = vec![0u8; header_size + image_size as usize];
        let header = windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER {
            biSize: header_size as u32,
            biWidth: width as i32,
            // 正高度 = 自下而上，CF_DIB 的约定。
            biHeight: height as i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: windows_sys::Win32::Graphics::Gdi::BI_RGB,
            biSizeImage: image_size,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                &header as *const _ as *const u8,
                bytes.as_mut_ptr(),
                header_size,
            );
        }
        let body = flip_rows(&self.pixels, self.width, self.height);
        bytes[header_size..].copy_from_slice(&body);
        bytes
    }

    /// CF_HDROP：DROPFILES 头 + 双 0 结尾的宽字符路径。
    fn dropfiles_bytes(&self) -> Vec<u8> {
        #[repr(C)]
        struct DropFiles {
            p_files: u32,
            pt_x: i32,
            pt_y: i32,
            f_nc: i32,
            f_wide: i32,
        }
        let header_size = std::mem::size_of::<DropFiles>();
        let mut bytes = vec![0u8; header_size + (self.file.len() + 1) * 2];
        let header = DropFiles {
            p_files: header_size as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                &header as *const _ as *const u8,
                bytes.as_mut_ptr(),
                header_size,
            );
        }
        for (index, unit) in self.file.iter().enumerate() {
            let offset = header_size + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn supports(format: u16) -> bool {
        format == CF_DIB || format == CF_HDROP
    }

    fn formats() -> Vec<windows::Win32::System::Com::FORMATETC> {
        use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
        [CF_DIB, CF_HDROP]
            .into_iter()
            .map(|format| FORMATETC {
                cfFormat: format,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT.0 as u32,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            })
            .collect()
    }
}

impl windows::Win32::System::Com::IDataObject_Impl for PinDataObject_Impl {
    fn GetData(
        &self,
        format: *const windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
        use windows::Win32::System::Com::DVASPECT_CONTENT;
        let Some(format) = (unsafe { format.as_ref() }) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        let extra = DRAG_IMAGE_MEDIUM
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .is_some_and(|(cf, _, _)| cf == format.cfFormat);
        if format.dwAspect != DVASPECT_CONTENT.0 as u32
            || (!PinDataObject::supports(format.cfFormat) && !extra)
        {
            // DV_E_FORMATETC
            return Err(windows::core::HRESULT(0x8004_0064u32 as i32).into());
        }
        if let Ok(mut slot) = DRAG_IMAGE_MEDIUM.lock() {
            if let Some((cf, tymed, handle)) = *slot {
                if cf == format.cfFormat {
                    *slot = None;
                    return Ok(windows::Win32::System::Com::STGMEDIUM {
                        tymed,
                        u: windows::Win32::System::Com::STGMEDIUM_0 {
                            hGlobal: windows::Win32::Foundation::HGLOBAL(handle as *mut c_void),
                        },
                        pUnkForRelease: std::mem::ManuallyDrop::new(None),
                    });
                }
            }
        }
        let bytes = if format.cfFormat == CF_DIB {
            self.dib_bytes()
        } else {
            self.dropfiles_bytes()
        };
        unsafe { medium_from_bytes(&bytes) }
    }

    fn GetDataHere(
        &self,
        _format: *const windows::Win32::System::Com::FORMATETC,
        _medium: *mut windows::Win32::System::Com::STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn QueryGetData(
        &self,
        format: *const windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::HRESULT {
        use windows::Win32::System::Com::DVASPECT_CONTENT;
        let Some(format) = (unsafe { format.as_ref() }) else {
            return windows::Win32::Foundation::E_INVALIDARG;
        };
        let extra = DRAG_IMAGE_MEDIUM
            .lock()
            .ok()
            .and_then(|slot| *slot)
            .is_some_and(|(cf, _, _)| cf == format.cfFormat);
        if format.dwAspect == DVASPECT_CONTENT.0 as u32
            && (PinDataObject::supports(format.cfFormat) || extra)
        {
            windows::Win32::Foundation::S_OK
        } else {
            windows::core::HRESULT(0x8004_0064u32 as i32)
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _in_format: *const windows::Win32::System::Com::FORMATETC,
        out_format: *mut windows::Win32::System::Com::FORMATETC,
    ) -> windows::core::HRESULT {
        if let Some(out) = unsafe { out_format.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        // DATA_S_SAMEFORMATETC
        windows::core::HRESULT(0x0004_0130)
    }

    fn SetData(
        &self,
        format: *const windows::Win32::System::Com::FORMATETC,
        medium: *const windows::Win32::System::Com::STGMEDIUM,
        release: windows::core::BOOL,
    ) -> windows::core::Result<()> {
        // IDragSourceHelper 会把拖动图像以私有格式写回数据对象；
        // 不接收的话 InitializeFromBitmap 直接返回 E_NOTIMPL。
        let Some(format) = (unsafe { format.as_ref() }) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        let Some(medium) = (unsafe { medium.as_ref() }) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        if release.as_bool() {
            let handle = unsafe { medium.u.hGlobal.0 } as isize;
            if let Ok(mut slot) = DRAG_IMAGE_MEDIUM.lock() {
                *slot = Some((format.cfFormat, medium.tymed, handle));
            }
            Ok(())
        } else {
            Err(windows::Win32::Foundation::E_NOTIMPL.into())
        }
    }

    fn EnumFormatEtc(
        &self,
        direction: u32,
    ) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        use windows::Win32::System::Com::DATADIR_GET;
        if direction != DATADIR_GET.0 as u32 {
            return Err(windows::Win32::Foundation::E_NOTIMPL.into());
        }
        let mut formats = PinDataObject::formats();
        if let Ok(slot) = DRAG_IMAGE_MEDIUM.lock() {
            if let Some((cf, _, _)) = *slot {
                formats.push(windows::Win32::System::Com::FORMATETC {
                    cfFormat: cf,
                    ptd: std::ptr::null_mut(),
                    dwAspect: windows::Win32::System::Com::DVASPECT_CONTENT.0 as u32,
                    lindex: -1,
                    tymed: windows::Win32::System::Com::TYMED_HGLOBAL.0 as u32,
                });
            }
        }
        unsafe { windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc(&formats) }
    }

    fn DAdvise(
        &self,
        _format: *const windows::Win32::System::Com::FORMATETC,
        _flags: u32,
        _sink: windows::core::Ref<'_, windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
}

/// 给数据对象设置拖动时跟随光标的图像（就是这张贴图本身）。
unsafe fn set_drag_image(
    data: &windows::Win32::System::Com::IDataObject,
    state: &PinState,
) -> windows::core::Result<()> {
    use windows::Win32::Foundation::{COLORREF, POINT, SIZE};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IDragSourceHelper, CLSID_DragDropHelper, SHDRAGIMAGE};
    let width = state.surface.width;
    let height = state.surface.height;
    let mut info: windows_sys::Win32::Graphics::Gdi::BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize =
        std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = windows_sys::Win32::Graphics::Gdi::BI_RGB;
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = windows_sys::Win32::Graphics::Gdi::CreateDIBSection(
        std::ptr::null_mut(),
        &info,
        windows_sys::Win32::Graphics::Gdi::DIB_RGB_COLORS,
        &mut bits,
        std::ptr::null_mut(),
        0,
    );
    if bitmap.is_null() || bits.is_null() {
        return Err(windows::Win32::Foundation::E_FAIL.into());
    }
    std::ptr::copy_nonoverlapping(
        state.surface.pixels().as_ptr(),
        bits as *mut u8,
        (width * height * 4) as usize,
    );
    // 截屏 DIB 的 alpha 字节是 0；拖动图像按 alpha 渲染，全 0 会导致缩略图完全透明、看不见。
    // 拖动图像不需要透明，统一置为不透明。
    let target = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
    for pixel in target.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let helper: IDragSourceHelper =
        CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)?;
    let image = SHDRAGIMAGE {
        sizeDragImage: SIZE { cx: width, cy: height },
        ptOffset: POINT {
            x: width / 2,
            y: height / 2,
        },
        hbmpDragImage: windows::Win32::Graphics::Gdi::HBITMAP(bitmap),
        crColorKey: COLORREF(0x00FF_00FF),
    };
    let result = helper.InitializeFromBitmap(&image, data);
    // Helper 会复制位图，调用后可以立刻释放。
    let _ = windows_sys::Win32::Graphics::Gdi::DeleteObject(bitmap as *mut c_void);
    result
}

/// 按住 Ctrl 拖动贴图：把这张图拖给其它程序（超级拖放）。
///
/// 数据对象自己实现，同时给 CF_DIB（位图）与 CF_HDROP（临时 PNG 文件），
/// 再用 `IDragSourceHelper` 把跟随光标的图像设成贴图本身。
unsafe fn super_drag(hwnd: HWND, state: &PinState) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::Interface;

    dbg_log("super_drag.begin");
    let Some(path) = write_drag_file(state) else {
        dbg_log("super_drag.write_file_failed");
        return;
    };
    dbg_log(&format!("super_drag.file={}", path.display()));
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let data: windows::Win32::System::Com::IDataObject = PinDataObject {
        pixels: state.surface.pixels().to_vec(),
        width: state.surface.width,
        height: state.surface.height,
        file,
    }
    .into();
    match set_drag_image(&data, state) {
        Ok(()) => dbg_log("super_drag.drag_image_ok"),
        Err(error) => dbg_log(&format!("super_drag.drag_image_failed {error:?}")),
    }
    let mut effect = 0u32;
    let drag_hr = SHDoDragDrop(
        hwnd,
        data.as_raw() as *mut c_void,
        std::ptr::null_mut(),
        DROPEFFECT_COPY,
        &mut effect,
    );
    dbg_log(&format!(
        "super_drag.drop hr=0x{:08X} effect=0x{:X}",
        drag_hr, effect
    ));
    // 拖动图像那块 HGLOBAL 由 helper 交给我们，拖放结束后释放。
    if let Ok(mut slot) = DRAG_IMAGE_MEDIUM.lock() {
        if let Some((_, _, handle)) = slot.take() {
            if handle != 0 {
                windows_sys::Win32::Foundation::GlobalFree(handle as *mut c_void);
            }
        }
    }
}

unsafe fn run_double_click(hwnd: HWND) {
    let Some(state) = state_of(hwnd) else { return };
    if (*state).drag.is_some() {
        return;
    }
    match (*state).double_click {
        DoubleClick::Close => {
            let state_ref = &*state;
            if confirm_close(hwnd, state_ref) {
                DestroyWindow(hwnd);
            }
        }
        DoubleClick::Copy => copy_to_clipboard(&*state),
        DoubleClick::None => {}
    }
}

unsafe fn begin_drag(hwnd: HWND, state: &mut PinState, lparam: LPARAM) {
    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    let client_x = (lparam & 0xFFFF) as i16 as i32;
    let client_y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let edges = hit_edges(rect.right, rect.bottom, client_x, client_y);
    if edges == 0 && !state.lock.allows_move() {
        return;
    }
    if edges != 0 && !state.lock.allows_resize() {
        return;
    }
    // 只有锁定位置时仍可缩放，此时四边之外不让拖动。
    if edges == 0 && state.lock == PinLock::Position {
        return;
    }
    SetCapture(hwnd);
    SetFocus(hwnd);
    state.drag = Some(Drag {
        start_rect: window_rect(hwnd),
        start_point: point,
        edges,
    });
}

unsafe fn update_drag(hwnd: HWND, state: &mut PinState) {
    let Some(drag) = state.drag.as_ref() else { return };
    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    let dx = point.x - drag.start_point.x;
    let dy = point.y - drag.start_point.y;
    if drag.edges == 0 {
        let width = drag.start_rect.right - drag.start_rect.left;
        let height = drag.start_rect.bottom - drag.start_rect.top;
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            drag.start_rect.left + dx,
            drag.start_rect.top + dy,
            width,
            height,
            SWP_NOACTIVATE,
        );
    } else {
        let rect = resize_rect(drag.start_rect, drag.edges, dx, dy);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE,
        );
    }
    shadow_sync(hwnd, state);
}

unsafe fn apply_zoom(hwnd: HWND, state: &mut PinState, delta: i32) {
    if !state.lock.allows_resize() {
        return;
    }
    let zoom = wheel_zoom(state.zoom, delta);
    if (zoom - state.zoom).abs() < f32::EPSILON {
        return;
    }
    let rect = window_rect(hwnd);
    let width = ((state.surface.width as f32 * zoom).round() as i32).max(MIN_SIDE);
    let height = ((state.surface.height as f32 * zoom).round() as i32).max(MIN_SIDE);
    state.zoom = zoom;
    // 缩放后短暂显示百分比，之后自动消失。
    state.zoom_until = GetTickCount() + 900;
    SetTimer(hwnd, TIMER_ZOOM, 80, None);
    // 左上角固定顶点：位置不动，只向右下方伸缩。
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        rect.left,
        rect.top,
        width,
        height,
        SWP_NOACTIVATE,
    );
    InvalidateRect(hwnd, std::ptr::null(), 0);
    shadow_sync(hwnd, state);
}

unsafe fn paint(hwnd: HWND, state: &PinState) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut client);
    SetStretchBltMode(dc, HALFTONE);
    StretchBlt(
        dc,
        0,
        0,
        client.right,
        client.bottom,
        state.surface.dc(),
        0,
        0,
        state.surface.width,
        state.surface.height,
        SRCCOPY,
    );
    if GetTickCount() < state.zoom_until {
        let text = format!("{:.0}%", state.zoom * 100.0);
        let mut tag = RECT {
            left: 6,
            top: 6,
            right: 66,
            bottom: 30,
        };
        let brush = CreateSolidBrush(0x00F0_F0F0);
        FillRect(dc, &tag, brush);
        DeleteObject(brush);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x0020_2020);
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let old = SelectObject(dc, font);
        let wide: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            dc,
            wide.as_ptr(),
            wide.len() as i32,
            &mut tag,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        SelectObject(dc, old);
    }
    EndPaint(hwnd, &ps);
}

#[derive(Clone, Copy)]
enum Process {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

/// 旋转/翻转：按目标像素反查源像素，重新生成两种行序的像素。
fn processed(source: &[u8], width: i32, height: i32, kind: Process) -> PinImage {
    let (ow, oh) = (width, height);
    let (nw, nh) = match kind {
        Process::RotateLeft | Process::RotateRight => (oh, ow),
        _ => (ow, oh),
    };
    let mut top_down = vec![0u8; (nw * nh * 4) as usize];
    for ny in 0..nh {
        for nx in 0..nw {
            let (sx, sy) = match kind {
                Process::RotateRight => (ny, oh - 1 - nx),
                Process::RotateLeft => (ow - 1 - ny, nx),
                Process::FlipHorizontal => (ow - 1 - nx, ny),
                Process::FlipVertical => (nx, oh - 1 - ny),
            };
            let src = ((sy * ow + sx) * 4) as usize;
            let dst = ((ny * nw + nx) * 4) as usize;
            top_down[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
        }
    }
    PinImage {
        width: nw,
        height: nh,
        top_down,
    }
}

/// 换掉贴图内容（旋转/翻转/重置）：重建绘制用的 DIB，窗口左上角不动，宽高跟着换。
unsafe fn install_image(hwnd: HWND, state: &mut PinState, next: PinImage) {
    let rect = window_rect(hwnd);
    let zoom = state.zoom;
    let width = ((next.width as f32 * zoom).round() as i32).max(MIN_SIDE);
    let height = ((next.height as f32 * zoom).round() as i32).max(MIN_SIDE);
    if let Some(mut surface) = Surface::new(next.width, next.height) {
        surface.pixels_mut().copy_from_slice(&next.top_down);
        state.surface = surface;
    }
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        rect.left,
        rect.top,
        width,
        height,
        SWP_NOACTIVATE,
    );
    InvalidateRect(hwnd, std::ptr::null(), 0);
    shadow_sync(hwnd, state);
}

/// 右键菜单的一个条目。菜单项用 owner-draw，左右留白按这里的小尺寸走，
/// 系统原生菜单的勾选列与箭头列太宽（用户要求收紧）。
struct MenuEntry {
    text: Vec<u16>,
    checked: bool,
    grayed: bool,
    submenu: bool,
    separator: bool,
}

impl MenuEntry {
    fn item(text: &str, grayed: bool, checked: bool) -> Box<Self> {
        Box::new(Self {
            text: text.encode_utf16().collect(),
            checked,
            grayed,
            submenu: false,
            separator: false,
        })
    }

    fn popup(text: &str) -> Box<Self> {
        Box::new(Self {
            text: text.encode_utf16().collect(),
            checked: false,
            grayed: false,
            submenu: true,
            separator: false,
        })
    }

    fn separator() -> Box<Self> {
        Box::new(Self {
            text: Vec::new(),
            checked: false,
            grayed: false,
            submenu: false,
            separator: true,
        })
    }
}

// 紧凑版式参数（逻辑像素，随 DPI 缩放见 `menu_scale`）。
const MENU_CHECK_W: i32 = 14;
const MENU_LEFT_PAD: i32 = 3;
const MENU_RIGHT_PAD: i32 = 4;
const MENU_ARROW_W: i32 = 9;
const MENU_ROW_PAD: i32 = 4;

/// 菜单字体：优先用系统菜单字体，取不到就退回默认 GUI 字体。
fn menu_font() -> HFONT {
    static FONT: OnceLock<usize> = OnceLock::new();
    *FONT.get_or_init(|| unsafe {
        let scale = menu_scale();
        let height = -((12.0 * scale).round() as i32);
        let face = widen("Microsoft YaHei UI");
        let font = CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            face.as_ptr(),
        );
        if !font.is_null() {
            return font as usize;
        }
        // 字体建不出来时退回系统菜单字体，至少保证可读。
        let mut metrics: NONCLIENTMETRICSW = std::mem::zeroed();
        metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
        if SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            &mut metrics as *mut _ as *mut c_void,
            0,
        ) != 0
        {
            let fallback = CreateFontIndirectW(&metrics.lfMenuFont);
            if !fallback.is_null() {
                return fallback as usize;
            }
        }
        GetStockObject(DEFAULT_GUI_FONT) as usize
    }) as HFONT
}

/// 每个菜单项的数据指针在菜单销毁前必须有效，这里持有 Box。
struct MenuStore(Vec<*mut MenuEntry>);

impl MenuStore {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn keep(&mut self, entry: Box<MenuEntry>) -> *const u16 {
        let raw = Box::into_raw(entry);
        self.0.push(raw);
        raw as *const u16
    }
}

impl Drop for MenuStore {
    fn drop(&mut self) {
        for raw in self.0.drain(..) {
            // 菜单已销毁，条目数据可以释放。
            drop(unsafe { Box::from_raw(raw) });
        }
    }
}

unsafe fn append_entry(
    store: &mut MenuStore,
    menu: HMENU,
    id: usize,
    entry: Box<MenuEntry>,
) {
    let grayed = entry.grayed;
    let submenu = entry.submenu;
    let separator = entry.separator;
    let data = store.keep(entry);
    let mut flags = MF_OWNERDRAW;
    if grayed {
        // owner-draw 下 MF_GRAYED 仍会被高亮选中，用 MF_DISABLED 才真正不可点。
        flags |= MF_DISABLED;
    }
    if submenu {
        flags |= MF_POPUP;
    }
    if separator {
        flags = MF_OWNERDRAW | MF_DISABLED;
    }
    AppendMenuW(menu, flags, id, data);
}

/// 菜单项尺寸：文字宽 + 固定的小内边距，勾选列与箭头列都按窄口径。
unsafe fn measure_menu_item(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let measure = &mut *(lparam as *mut MEASUREITEMSTRUCT);
    let entry = &*(measure.itemData as *const MenuEntry);
    if entry.separator {
        measure.itemWidth = 1;
        measure.itemHeight = (9.0 * menu_scale()) as u32;
        return 1;
    }
    let scale = menu_scale();
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return 1;
    }
    let font = menu_font();
    let old = SelectObject(dc, font);
    let mut size: SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(dc, entry.text.as_ptr(), entry.text.len() as i32, &mut size);
    SelectObject(dc, old);
    ReleaseDC(hwnd, dc);
    let arrow = if entry.submenu { MENU_ARROW_W } else { 0 };
    measure.itemWidth = ((((MENU_LEFT_PAD + MENU_CHECK_W + MENU_RIGHT_PAD + arrow) as f32
        * scale) as i32)
        + size.cx) as u32;
    measure.itemHeight = (size.cy as f32 + (MENU_ROW_PAD * 2) as f32 * scale) as u32;
    1
}

unsafe fn draw_menu_item(_hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let draw = &mut *(lparam as *mut DRAWITEMSTRUCT);
    let entry = &*(draw.itemData as *const MenuEntry);
    let selected = draw.itemState & ODS_SELECTED != 0;
    let scale = menu_scale();
    if entry.separator {
        let brush = GetSysColorBrush(COLOR_3DSHADOW);
        let rect = RECT {
            left: draw.rcItem.left + (4.0 * scale) as i32,
            top: (draw.rcItem.top + draw.rcItem.bottom) / 2,
            right: draw.rcItem.right - (4.0 * scale) as i32,
            bottom: (draw.rcItem.top + draw.rcItem.bottom) / 2 + 1,
        };
        FillRect(draw.hDC, &rect, brush);
        return 1;
    }
    let background = if selected {
        GetSysColorBrush(COLOR_HIGHLIGHT)
    } else {
        GetSysColorBrush(COLOR_MENU)
    };
    FillRect(draw.hDC, &draw.rcItem, background);
    let font = menu_font();
    let old_font = SelectObject(draw.hDC, font);
    SetBkMode(draw.hDC, TRANSPARENT as i32);
    let color = if entry.grayed {
        GetSysColor(COLOR_GRAYTEXT)
    } else if selected {
        GetSysColor(COLOR_HIGHLIGHTTEXT)
    } else {
        GetSysColor(COLOR_MENUTEXT)
    };
    SetTextColor(draw.hDC, color);
    if entry.checked {
        let tick = widen("✓");
        let mut rect = RECT {
            left: draw.rcItem.left + (MENU_LEFT_PAD as f32 * scale) as i32,
            top: draw.rcItem.top,
            right: draw.rcItem.left
                + ((MENU_LEFT_PAD + MENU_CHECK_W) as f32 * scale) as i32,
            bottom: draw.rcItem.bottom,
        };
        DrawTextW(
            draw.hDC,
            tick.as_ptr(),
            tick.len() as i32 - 1,
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
    }
    let mut text_rect = RECT {
        left: draw.rcItem.left
            + ((MENU_LEFT_PAD + MENU_CHECK_W) as f32 * scale) as i32,
        top: draw.rcItem.top,
        right: draw.rcItem.right - (MENU_RIGHT_PAD as f32 * scale) as i32,
        bottom: draw.rcItem.bottom,
    };
    DrawTextW(
        draw.hDC,
        entry.text.as_ptr(),
        entry.text.len() as i32,
        &mut text_rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    // 子菜单箭头由系统绘制（owner-draw 的弹出项系统仍会画），这里不能再自己画，
    // 否则会出现两个箭头；宽度只在 measure 里预留。
    SelectObject(draw.hDC, old_font);
    1
}

/// 菜单按 96 DPI 设计，实际按当前 DPI 放大。
fn menu_scale() -> f32 {
    unsafe {
        let dpi = GetDpiForWindow(GetForegroundWindow());
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }
}

/// 贴图的右键菜单：文本选择与标注先占位（置灰），其余项可用。
unsafe fn show_menu(hwnd: HWND, state: &PinState) {
    let menu = CreatePopupMenu();
    let process = CreatePopupMenu();
    if menu.is_null() || process.is_null() {
        return;
    }
    let mut store = MenuStore::new();
    append_entry(
        &mut store,
        process,
        MENU_ROTATE_LEFT,
        MenuEntry::item("向左旋转", false, false),
    );
    append_entry(
        &mut store,
        process,
        MENU_ROTATE_RIGHT,
        MenuEntry::item("向右旋转", false, false),
    );
    append_entry(
        &mut store,
        process,
        MENU_FLIP_H,
        MenuEntry::item("水平翻转", false, false),
    );
    append_entry(
        &mut store,
        process,
        MENU_FLIP_V,
        MenuEntry::item("垂直翻转", false, false),
    );
    append_entry(&mut store, process, 0, MenuEntry::separator());
    append_entry(
        &mut store,
        process,
        MENU_RESET_PROCESS,
        MenuEntry::item("重置所有处理", false, false),
    );

    append_entry(
        &mut store,
        menu,
        MENU_SELECT_ALL_TEXT,
        MenuEntry::item("选择所有文本", true, false),
    );
    append_entry(
        &mut store,
        menu,
        MENU_TEXT_SELECTABLE,
        MenuEntry::item("文本可选择", true, false),
    );
    append_entry(&mut store, menu, 0, MenuEntry::separator());
    append_entry(
        &mut store,
        menu,
        process as usize,
        MenuEntry::popup("图像处理"),
    );
    append_entry(
        &mut store,
        menu,
        MENU_COPY_IMAGE,
        MenuEntry::item("复制图像", false, false),
    );
    append_entry(
        &mut store,
        menu,
        MENU_SAVE_AS,
        MenuEntry::item("图像另存为…", false, false),
    );
    append_entry(&mut store, menu, 0, MenuEntry::separator());
    append_entry(
        &mut store,
        menu,
        MENU_ANNOTATE,
        MenuEntry::item("标注", true, false),
    );
    append_entry(
        &mut store,
        menu,
        MENU_HIDE_ANNOTATION,
        MenuEntry::item("隐藏标注", true, false),
    );
    append_entry(&mut store, menu, 0, MenuEntry::separator());
    append_entry(
        &mut store,
        menu,
        MENU_SHADOW,
        MenuEntry::item("窗口阴影", false, state.shadow_enabled),
    );
    append_entry(
        &mut store,
        menu,
        MENU_LOCK,
        MenuEntry::item("锁定", false, state.lock != PinLock::None),
    );
    append_entry(&mut store, menu, 0, MenuEntry::separator());
    append_entry(
        &mut store,
        menu,
        MENU_CLOSE,
        MenuEntry::item("关闭", false, false),
    );

    // owner-draw 的条目要报告宽度，菜单宽取各项最大宽度。
    let mut point: POINT = std::mem::zeroed();
    GetCursorPos(&mut point);
    // 不用 TPM_RETURNCMD：命令统一走 WM_COMMAND，右键菜单与外部投递的行为一致。
    TrackPopupMenuEx(
        menu,
        TPM_RIGHTBUTTON,
        point.x,
        point.y,
        hwnd,
        std::ptr::null_mut(),
    );
    // DestroyMenu 会连带回收用 MF_POPUP 挂上的子菜单。
    DestroyMenu(menu);
}

/// 「图像另存为…」：系统保存对话框，默认按贴图名字模板生成文件名。
unsafe fn save_as(hwnd: HWND, state: &PinState) {
    let default_name = super::save::render_file_name("Pin_$yyyy$-$MM$-$dd$_$HH$-$mm$-$ss$");
    let mut buffer = [0u16; 520];
    for (index, unit) in default_name.encode_utf16().take(510).enumerate() {
        buffer[index] = unit;
    }
    let filter: Vec<u16> = "PNG 图片 (*.png)\0*.png\0所有文件 (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let extension: Vec<u16> = "png\0".encode_utf16().collect();
    let title: Vec<u16> = "贴图另存为".encode_utf16().collect();
    let mut dialog: OPENFILENAMEW = std::mem::zeroed();
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.nFilterIndex = 1;
    dialog.lpstrFile = buffer.as_mut_ptr();
    dialog.nMaxFile = buffer.len() as u32;
    dialog.lpstrDefExt = extension.as_ptr();
    dialog.lpstrTitle = title.as_ptr();
    dialog.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER;
    if GetSaveFileNameW(&mut dialog) == 0 {
        return;
    }
    let length = buffer.iter().position(|value| *value == 0).unwrap_or(0);
    let path = String::from_utf16_lossy(&buffer[..length]);
    if let Ok(png) = super::save::encode_png(
        state.surface.pixels(),
        state.surface.width as u32,
        state.surface.height as u32,
    ) {
        let _ = std::fs::write(path, png);
    }
}

unsafe fn handle_menu(hwnd: HWND, state: &mut PinState, command: usize) {
    match command {
        MENU_ROTATE_LEFT => {
            let next = processed(state.surface.pixels(), state.surface.width, state.surface.height, Process::RotateLeft);
            install_image(hwnd, state, next);
        }
        MENU_ROTATE_RIGHT => {
            let next = processed(state.surface.pixels(), state.surface.width, state.surface.height, Process::RotateRight);
            install_image(hwnd, state, next);
        }
        MENU_FLIP_H => {
            let next = processed(state.surface.pixels(), state.surface.width, state.surface.height, Process::FlipHorizontal);
            install_image(hwnd, state, next);
        }
        MENU_FLIP_V => {
            let next = processed(state.surface.pixels(), state.surface.width, state.surface.height, Process::FlipVertical);
            install_image(hwnd, state, next);
        }
        MENU_RESET_PROCESS => {
            let next = (*state.original).clone();
            install_image(hwnd, state, next);
        }
        MENU_COPY_IMAGE => copy_to_clipboard(state),
        MENU_SAVE_AS => save_as(hwnd, state),
        MENU_SHADOW => {
            state.shadow_enabled = !state.shadow_enabled;
            state.shadow_key = (0, 0, false, -1);
            if state.shadow_enabled {
                state.glow_until = GetTickCount() + GLOW_MS;
                SetTimer(hwnd, TIMER_GLOW, 40, None);
            }
            shadow_sync(hwnd, state);
        }
        MENU_LOCK => {
            state.lock = if state.lock == PinLock::None {
                PinLock::All
            } else {
                PinLock::None
            };
        }
        MENU_CLOSE => {
            if confirm_close(hwnd, state) {
                DestroyWindow(hwnd);
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            if let Some(state) = state_of(hwnd) {
                paint(hwnd, &*state);
            }
            0
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = state_of(hwnd) {
                if GetKeyState(VK_CONTROL as i32) < 0 {
                    // Ctrl + 拖动 = 超级拖放：把这张图当文件拖给其它程序，窗口本身不动。
                    super_drag(hwnd, &*state);
                    // 拖放期间按 Esc 是「取消拖放」，结束后可能还会落到窗口上，先挡一下。
                    (*state).escape_guard = GetTickCount() + 400;
                } else {
                    begin_drag(hwnd, &mut *state, lp);
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            if let Some(state) = state_of(hwnd) {
                if (*state).drag.is_some() {
                    update_drag(hwnd, &mut *state);
                }
            }
            0
        }
        WM_LBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                (*state).drag = None;
            }
            ReleaseCapture();
            0
        }
        WM_LBUTTONDBLCLK => {
            run_double_click(hwnd);
            0
        }
        WM_RBUTTONUP => {
            if let Some(state) = state_of(hwnd) {
                (*state).drag = None;
                ReleaseCapture();
                show_menu(hwnd, &*state);
            }
            0
        }
        WM_COMMAND => {
            // 右键菜单项：低 16 位是命令 id。
            if let Some(state) = state_of(hwnd) {
                handle_menu(hwnd, &mut *state, (wp & 0xFFFF) as usize);
            }
            0
        }
        WM_MEASUREITEM => measure_menu_item(hwnd, lp),
        WM_DRAWITEM => draw_menu_item(hwnd, lp),
        WM_MOUSEWHEEL => {
            if let Some(state) = state_of(hwnd) {
                let delta = ((wp >> 16) & 0xFFFF) as u16 as i16 as i32;
                apply_zoom(hwnd, &mut *state, delta);
            }
            0
        }
        WM_SETCURSOR => {
            if let Some(state) = state_of(hwnd) {
                let mut point: POINT = std::mem::zeroed();
                GetCursorPos(&mut point);
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(hwnd, &mut rect);
                let edges = if (*state).lock.allows_resize() {
                    hit_edges(
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        point.x - rect.left,
                        point.y - rect.top,
                    )
                } else {
                    0
                };
                let cursor = match edges {
                    1 | 2 => IDC_SIZEWE,
                    4 | 8 => IDC_SIZENS,
                    5 | 10 => IDC_SIZENWSE,
                    6 | 9 => IDC_SIZENESW,
                    _ => {
                        if (*state).lock.allows_move() {
                            IDC_SIZEALL
                        } else {
                            IDC_ARROW
                        }
                    }
                };
                SetCursor(LoadCursorW(std::ptr::null_mut(), cursor));
                return 1;
            }
            0
        }
        WM_TIMER if wp == TIMER_ZOOM => {
            if let Some(state) = state_of(hwnd) {
                if GetTickCount() >= (*state).zoom_until {
                    KillTimer(hwnd, TIMER_ZOOM);
                }
                let tag = RECT {
                    left: 0,
                    top: 0,
                    right: 72,
                    bottom: 36,
                };
                InvalidateRect(hwnd, &tag, 0);
            }
            0
        }
        WM_TIMER if wp == TIMER_GLOW => {
            if let Some(state) = state_of(hwnd) {
                if GetTickCount() >= (*state).glow_until {
                    KillTimer(hwnd, TIMER_GLOW);
                }
                shadow_sync(hwnd, &mut *state);
            }
            0
        }
        WM_ACTIVATE => {
            if let Some(state) = state_of(hwnd) {
                let active = (wp & 0xFFFF) as u32 != WA_INACTIVE;
                if (*state).focused != active {
                    (*state).focused = active;
                    // 焦点态决定阴影颜色：焦点淡蓝、失焦灰。
                    shadow_sync(hwnd, &mut *state);
                }
            }
            0
        }
        WM_KEYDOWN => {
            if wp as u16 == VK_ESCAPE {
                if let Some(state) = state_of(hwnd) {
                    if GetTickCount() >= (*state).escape_guard && confirm_close(hwnd, &*state) {
                        DestroyWindow(hwnd);
                    }
                }
            }
            0
        }
        WM_CLOSE => {
            if let Some(state) = state_of(hwnd) {
                if confirm_close(hwnd, &*state) {
                    DestroyWindow(hwnd);
                }
            } else {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_NCDESTROY => {
            let state = state_of(hwnd);
            if let Some(state) = state {
                shadow_destroy(&mut *state);
                // 记下关闭前的样子，供「恢复上次关闭的贴图」使用。
                // 只共享 original 的 Arc（不复制像素），队列上限压到 3 份。
                let rect = window_rect(hwnd);
                if let Ok(mut last) = LAST_CLOSED.lock() {
                    last.push_back((Arc::clone(&(*state).original), rect));
                    while last.len() > 3 {
                        last.pop_front();
                    }
                }
                drop(Box::from_raw(state));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            if let Ok(mut pins) = PINS.lock() {
                pins.retain(|item| *item != hwnd as isize);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 一次贴图窗口创建指令是否成功投递。
pub(crate) fn warm_up() {
    // 预热：提前起贴图线程并注册窗口类，按下贴图时不用再等线程启动。
    let _ = host();
}

/// 建一个贴图窗口；`rect` 是屏幕坐标下的初始位置与大小。
pub(crate) fn create(image: Arc<PinImage>, rect: RECT, options: PinOptions) -> bool {
    send(PinCommand::Create {
        image,
        rect,
        options,
    })
}

/// 关闭全部贴图窗口。
pub(crate) fn close_all() {
    let _ = send(PinCommand::CloseAll);
}

/// 隐藏 / 显示全部贴图窗口。
pub(crate) fn toggle_visible() {
    let _ = send(PinCommand::ToggleVisible);
}

/// 切换全部贴图窗口的鼠标穿透。
pub(crate) fn toggle_passthrough() {
    let _ = send(PinCommand::TogglePassthrough);
}

/// 重新贴出上一次关闭的贴图（没有则退回历史里最新的一张）。
pub(crate) fn restore_last(history_dir: &Path, options: PinOptions) -> bool {
    let last = LAST_CLOSED.lock().ok().and_then(|mut last| last.pop_back());
    if let Some((image, rect)) = last {
        return create(image, rect, options);
    }
    let Some(path) = newest_history(history_dir) else {
        return false;
    };
    match decode_png(&path) {
        Some(image) => {
            let rect = RECT {
                left: 120,
                top: 120,
                right: 120 + image.width,
                bottom: 120 + image.height,
            };
            create(Arc::new(image), rect, options)
        }
        None => false,
    }
}

/// 工具被停用时调用：关掉所有贴图窗口。
pub(crate) fn stop_all() {
    close_all();
}

/// 贴图历史目录：`<存储目录>/screenshot/pins`。
pub(crate) fn history_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("pins")
}

/// 往历史里追加一张贴图，并按保留数量清理最旧的。
pub(crate) fn record_history(data_dir: &Path, image: &PinImage, limit: usize) -> Option<PathBuf> {
    let dir = history_dir(data_dir);
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let name = format!("pin_{stamp}.png");
    let path = dir.join(&name);
    let png = super::save::encode_png(&image.top_down, image.width as u32, image.height as u32).ok()?;
    std::fs::write(&path, png).ok()?;
    let meta = format!(
        "{{\"file\":\"{name}\",\"width\":{},\"height\":{},\"created_at\":{stamp}}}",
        image.width, image.height
    );
    let _ = std::fs::write(dir.join(format!("pin_{stamp}.meta")), meta);
    prune(&dir, limit.max(1));
    Some(path)
}

/// 只保留最新的 `limit` 张，其余图片与 .meta 一起删掉。
fn prune(dir: &Path, limit: usize) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut shots: Vec<PathBuf> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect();
    if shots.len() <= limit {
        return;
    }
    shots.sort();
    let remove = shots.len() - limit;
    for path in shots.into_iter().take(remove) {
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            let _ = std::fs::remove_file(dir.join(format!("{stem}.meta")));
        }
        let _ = std::fs::remove_file(path);
    }
}

fn newest_history(dir: &Path) -> Option<PathBuf> {
    let mut shots: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect();
    shots.sort();
    shots.pop()
}

/// 读历史 PNG 并转成贴图用的两种像素排布。
fn decode_png(path: &Path) -> Option<PinImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(file).read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as i32, info.height as i32);
    let mut top_down = Vec::with_capacity((width * height * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => top_down.extend_from_slice(&buffer),
        png::ColorType::Rgb => {
            for chunk in buffer.chunks(3) {
                top_down.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
        }
        _ => return None,
    }
    Some(PinImage {
        width,
        height,
        top_down,
    })
}

#[cfg(test)]
mod tests {
    use super::{hit_edges, resize_rect, wheel_zoom, DoubleClick, PinLock};
    use windows_sys::Win32::Foundation::RECT;

    fn rect() -> RECT {
        RECT {
            left: 100,
            top: 100,
            right: 300,
            bottom: 200,
        }
    }

    #[test]
    fn edges_hit_only_near_borders() {
        assert_eq!(hit_edges(200, 100, 100, 50), 0);
        assert_eq!(hit_edges(200, 100, 2, 50), 1);
        assert_eq!(hit_edges(200, 100, 198, 50), 2);
        assert_eq!(hit_edges(200, 100, 100, 1), 4);
        assert_eq!(hit_edges(200, 100, 198, 98), 10);
    }

    #[test]
    fn resize_keeps_fixed_opposite_edge() {
        let next = resize_rect(rect(), 2, 40, 0);
        assert_eq!((next.left, next.right), (100, 340));
    }

    #[test]
    fn resize_flips_when_crossing() {
        // 拖右边往左推过左边界：被拖的边成为新左边界，对边固定。
        let next = resize_rect(rect(), 2, -260, 0);
        assert_eq!((next.left, next.right), (40, 100));
        // 继续往左推时跟随光标，对边仍然固定在 100。
        let flipped = resize_rect(rect(), 2, -350, 0);
        assert_eq!((flipped.left, flipped.right), (-50, 100));
    }

    #[test]
    fn wheel_zoom_is_bounded() {
        assert!((wheel_zoom(1.0, 120) - 1.1).abs() < 0.001);
        assert!(wheel_zoom(1.0, -120) < 1.0);
        assert!((wheel_zoom(1000.0, 120) - 8.0).abs() < 0.001);
        assert!(wheel_zoom(0.0001, -120) >= 0.1);
    }

    #[test]
    fn locks_gate_operations() {
        assert!(PinLock::None.allows_move() && PinLock::None.allows_resize());
        assert!(!PinLock::Position.allows_move() && PinLock::Position.allows_resize());
        assert!(!PinLock::All.allows_move() && !PinLock::All.allows_resize());
    }

    #[test]
    fn double_click_parses() {
        assert_eq!(DoubleClick::parse("close"), DoubleClick::Close);
        assert_eq!(DoubleClick::parse("copy"), DoubleClick::Copy);
        assert_eq!(DoubleClick::parse(""), DoubleClick::None);
    }
}
