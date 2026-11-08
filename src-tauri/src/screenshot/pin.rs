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
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::dib::Surface;

/// 一张贴图的原图：`top_down` 用于编码 PNG，`bottom_up` 用于建 DIB 段。
pub(crate) struct PinImage {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) top_down: Vec<u8>,
    pub(crate) bottom_up: Vec<u8>,
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

const CLASS_SHADOW: &str = "LightweightToolsetPinShadow\0";
const CLASS_FLAT: &str = "LightweightToolsetPinFlat\0";
const WM_PIN_COMMAND: u32 = WM_APP + 1;
const TIMER_ZOOM: usize = 1;
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
    surface: Surface,
    image: Arc<PinImage>,
    zoom: f32,
    lock: PinLock,
    confirm: bool,
    double_click: DoubleClick,
    drag: Option<Drag>,
    zoom_until: u32,
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
            let _ = ready_tx.send(GetCurrentThreadId() as usize);
            register_class(CLASS_SHADOW, CS_DROPSHADOW);
            register_class(CLASS_FLAT, 0);

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

unsafe fn register_class(name: &str, style: u32) {
    // 类名要活到 RegisterClassW 返回之后再释放，这里由 owned 持有。
    let owned = widen(name);
    let wc = WNDCLASSW {
        style: style | CS_DBLCLKS,
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
    let class = widen(if options.shadow { CLASS_SHADOW } else { CLASS_FLAT });
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
    surface.pixels_mut().copy_from_slice(&image.bottom_up);
    let state = Box::new(PinState {
        surface,
        image,
        zoom: 1.0,
        lock: options.lock,
        confirm: options.confirm_close,
        double_click: options.double_click,
        drag: None,
        zoom_until: 0,
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
    let png = super::save::encode_png(
        &state.image.top_down,
        state.image.width as u32,
        state.image.height as u32,
    )
    .ok();
    let _ = super::clipboard_image::copy_image(
        state.image.width,
        state.image.height,
        &state.image.bottom_up,
        png.as_deref(),
    );
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
    let center_x = (rect.left + rect.right) / 2;
    let center_y = (rect.top + rect.bottom) / 2;
    let width = ((state.image.width as f32 * zoom).round() as i32).max(MIN_SIDE);
    let height = ((state.image.height as f32 * zoom).round() as i32).max(MIN_SIDE);
    state.zoom = zoom;
    // 缩放后短暂显示百分比，之后自动消失。
    state.zoom_until = GetTickCount() + 900;
    SetTimer(hwnd, TIMER_ZOOM, 80, None);
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        center_x - width / 2,
        center_y - height / 2,
        width,
        height,
        SWP_NOACTIVATE,
    );
    InvalidateRect(hwnd, std::ptr::null(), 0);
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
        state.image.width,
        state.image.height,
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
                begin_drag(hwnd, &mut *state, lp);
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
                if confirm_close(hwnd, &*state) {
                    DestroyWindow(hwnd);
                }
            }
            0
        }
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
        WM_KEYDOWN => {
            if wp as u16 == VK_ESCAPE {
                if let Some(state) = state_of(hwnd) {
                    if confirm_close(hwnd, &*state) {
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
                // 记下关闭前的样子，供「恢复上次关闭的贴图」使用。
                let rect = window_rect(hwnd);
                if let Ok(mut last) = LAST_CLOSED.lock() {
                    last.push_back((Arc::clone(&(*state).image), rect));
                    while last.len() > 20 {
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
    let stride = width as usize * 4;
    let mut bottom_up = Vec::with_capacity(top_down.len());
    for row in (0..height as usize).rev() {
        bottom_up.extend_from_slice(&top_down[row * stride..(row + 1) * stride]);
    }
    Some(PinImage {
        width,
        height,
        top_down,
        bottom_up,
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
