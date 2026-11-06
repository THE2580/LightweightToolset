use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::Ime::ImmAssociateContext;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, VK_ESCAPE, VK_LSHIFT, VK_RSHIFT, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::capture;
use super::clipboard_image;
use super::dib::Surface;

static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

const BORDER: i32 = 2;
const LABEL_HEIGHT: i32 = 24;
/// 与 `AlphaBlend` 配合：黑色源、45% 不透明度，用来压暗整屏。
const DIM_ALPHA: u8 = 115;

/// 放大镜：15×15 像素，每格 16 px（与 PixPin 的放大倍率一致）。
const MAG_CELLS: i32 = 15;
const MAG_CELL: i32 = 16;
const MAG_SIZE: i32 = MAG_CELLS * MAG_CELL;
const PANEL_WIDTH: i32 = MAG_SIZE + 2;
const PANEL_HEIGHT: i32 = 1 + MAG_SIZE + 1 + 8 + 22 + 2 + 22 + 2 + 40 + 6;
/// 面板相对光标的偏移。
const PANEL_OFFSET: i32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct OverlayOutcome {
    pub selection: Selection,
    /// 自上而下 BGRA（PNG 编码用）。
    pub top_down: Vec<u8>,
    /// 自下而上 BGRA（CF_DIB 剪贴板用）。
    pub bottom_up: Vec<u8>,
}

struct OverlayState {
    width: i32,
    height: i32,
    origin_x: i32,
    origin_y: i32,
    original: Surface,
    dimmed: Surface,
    back: Surface,
    font: HFONT,
    pen: HPEN,
    grid_pen: HPEN,
    cross_pen: HPEN,
    white_pen: HPEN,
    panel_brush: HBRUSH,
    label_brush: HBRUSH,
    dragging: bool,
    anchor: Option<(i32, i32)>,
    current: Option<(i32, i32)>,
    cursor: Option<(i32, i32)>,
    rgb_mode: bool,
    last_frame: Option<(i32, i32, i32, i32)>,
    painted_once: bool,
    result: Option<Selection>,
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        unsafe {
            for handle in [self.font as HGDIOBJ] {
                if !handle.is_null() {
                    DeleteObject(handle);
                }
            }
            for handle in [
                self.pen as HGDIOBJ,
                self.grid_pen as HGDIOBJ,
                self.cross_pen as HGDIOBJ,
                self.white_pen as HGDIOBJ,
            ] {
                if !handle.is_null() {
                    DeleteObject(handle);
                }
            }
            if !self.panel_brush.is_null() {
                DeleteObject(self.panel_brush);
            }
            if !self.label_brush.is_null() {
                DeleteObject(self.label_brush);
            }
        }
    }
}

impl OverlayState {
    /// 拖动中的矩形（1px 也算），用于实时绘制。
    fn current_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let (ax, ay) = self.anchor?;
        let (cx, cy) = self.current?;
        let left = ax.min(cx).clamp(0, self.width);
        let top = ay.min(cy).clamp(0, self.height);
        let right = ax.max(cx).clamp(0, self.width);
        let bottom = ay.max(cy).clamp(0, self.height);
        if right <= left || bottom <= top {
            return None;
        }
        Some((left, top, right - left, bottom - top))
    }

    /// 放大镜面板位置：默认在光标右下，越界时翻到另一侧。
    fn panel_rect(&self, cursor: (i32, i32)) -> (i32, i32, i32, i32) {
        let (cx, cy) = cursor;
        let mut x = cx + PANEL_OFFSET;
        let mut y = cy + PANEL_OFFSET;
        if x + PANEL_WIDTH > self.width {
            x = cx - PANEL_OFFSET - PANEL_WIDTH;
        }
        if y + PANEL_HEIGHT > self.height {
            y = cy - PANEL_OFFSET - PANEL_HEIGHT;
        }
        let x = x.clamp(0, (self.width - PANEL_WIDTH).max(0));
        let y = y.clamp(0, (self.height - PANEL_HEIGHT).max(0));
        (x, y, PANEL_WIDTH, PANEL_HEIGHT)
    }

    fn magnifier_visible(&self) -> bool {
        self.cursor.is_some()
    }

    /// 本帧可能绘制到的全部区域：选区（含边框/标签边距）∪ 放大镜面板。
    fn frame_rect(&self) -> Option<(i32, i32, i32, i32)> {
        let mut rect = self.current_rect().map(|(x, y, w, h)| {
            (
                (x - BORDER - 1).clamp(0, self.width),
                (y - LABEL_HEIGHT - BORDER).clamp(0, self.height),
                (w + 2 * (BORDER + 1) + 1).clamp(0, self.width),
                (h + LABEL_HEIGHT + 2 * BORDER + 1).clamp(0, self.height),
            )
        });
        if self.magnifier_visible() {
            if let Some(cursor) = self.cursor {
                rect = union_rect(rect, Some(self.panel_rect(cursor)));
            }
        }
        rect
    }

    /// 光标所在像素的颜色（B, G, R）；坐标按物理像素。
    fn pixel_at(&self, cursor: (i32, i32)) -> Option<(u8, u8, u8)> {
        let (x, y) = cursor;
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let stride = self.width as usize * 4;
        let index = y as usize * stride + x as usize * 4;
        let pixels = self.original.pixels();
        Some((pixels[index], pixels[index + 1], pixels[index + 2]))
    }
}

fn union_rect(
    a: Option<(i32, i32, i32, i32)>,
    b: Option<(i32, i32, i32, i32)>,
) -> Option<(i32, i32, i32, i32)> {
    match (a, b) {
        (None, None) => None,
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (Some(a), Some(b)) => {
            let left = a.0.min(b.0);
            let top = a.1.min(b.1);
            let right = (a.0 + a.2).max(b.0 + b.2);
            let bottom = (a.1 + a.3).max(b.1 + b.3);
            Some((left, top, right - left, bottom - top))
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<OverlayState>> = const { RefCell::new(None) };
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn crop_top_down(
    src: &[u8],
    stride: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    width: i32,
    height: i32,
) -> Vec<u8> {
    let x = x.clamp(0, width);
    let y = y.clamp(0, height);
    let w = w.clamp(0, width - x);
    let h = h.clamp(0, height - y);
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for row in 0..h as usize {
        let src_row = (y as usize + row) * stride + x as usize * 4;
        let dst_row = row * w as usize * 4;
        out[dst_row..dst_row + w as usize * 4]
            .copy_from_slice(&src[src_row..src_row + w as usize * 4]);
    }
    out
}

fn crop_bottom_up(
    src_top_down: &[u8],
    stride: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    width: i32,
    height: i32,
) -> Vec<u8> {
    let x = x.clamp(0, width);
    let y = y.clamp(0, height);
    let w = w.clamp(0, width - x);
    let h = h.clamp(0, height - y);
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for row in 0..h as usize {
        let src_row = (y as usize + (h as usize - 1 - row)) * stride + x as usize * 4;
        let dst_row = row * w as usize * 4;
        out[dst_row..dst_row + w as usize * 4]
            .copy_from_slice(&src_top_down[src_row..src_row + w as usize * 4]);
    }
    out
}

unsafe fn draw_selection_chrome(hdc: HDC, pen: HPEN, x: i32, y: i32, w: i32, h: i32) {
    let old_pen = SelectObject(hdc, pen);
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    Rectangle(hdc, x, y, x + w, y + h);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
}

/// 选区标签：起点屏幕坐标 + 尺寸（物理像素）。
///
/// 文字压在深色底条上：选区外是压暗图，选区内可能是任意亮色内容，
/// 只靠白色文字加阴影在浅色内容上对比度不够。
unsafe fn draw_label(
    hdc: HDC,
    font: HFONT,
    brush: HBRUSH,
    origin_x: i32,
    origin_y: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let label = format!("{}, {} · {} × {}", x + origin_x, y + origin_y, w, h);
    let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT as i32);

    let inside = y < LABEL_HEIGHT + BORDER + 2;
    let top = if inside { y + 3 } else { y - LABEL_HEIGHT + 2 };
    let max_right = (x + w - 2).max(x + 4);

    let mut size: SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32 - 1, &mut size);
    let pill = RECT {
        left: (x + 3).clamp(0, max_right),
        top: (top - 1).max(y.min(top - 1)),
        right: (x + 6 + size.cx + 6).min(max_right),
        bottom: top + 18,
    };
    FillRect(hdc, &pill, brush);

    let mut rect = RECT {
        left: x + 4,
        top,
        right: max_right,
        bottom: top + 19,
    };
    SetTextColor(hdc, rgb(0xff, 0xff, 0xff));
    DrawTextW(hdc, wide.as_ptr(), -1, &mut rect, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(hdc, old_font);
}

fn color_text(color: (u8, u8, u8), rgb_mode: bool) -> String {
    let (b, g, r) = color;
    if rgb_mode {
        format!("RGB({r}, {g}, {b})")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

/// 在矩形内居中画一行文字。
unsafe fn draw_centered(hdc: HDC, text: &str, rect: &mut RECT) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    DrawTextW(hdc, wide.as_ptr(), -1, rect, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX);
}

/// 放大镜：像素放大 + 网格 + 十字线 + 坐标 + 颜色值 + 快捷键提示。
///
/// 视觉对齐 PixPin：240px 放大区、1px 白框、蓝色十字线、黑底信息区三行居中。
unsafe fn draw_magnifier(state: &OverlayState, hdc: HDC, panel: (i32, i32, i32, i32)) {
    let (px, py, pw, ph) = panel;
    let panel_rect = RECT {
        left: px,
        top: py,
        right: px + pw,
        bottom: py + ph,
    };
    FillRect(hdc, &panel_rect, state.panel_brush);

    let Some(cursor) = state.cursor else {
        return;
    };
    let color = state.pixel_at(cursor).unwrap_or((0, 0, 0));

    let sx = (cursor.0 - MAG_CELLS / 2).clamp(0, (state.width - MAG_CELLS).max(0));
    let sy = (cursor.1 - MAG_CELLS / 2).clamp(0, (state.height - MAG_CELLS).max(0));
    let mx = px + 1;
    let my = py + 1;

    let old_mode = SetStretchBltMode(hdc, COLORONCOLOR);
    StretchBlt(
        hdc,
        mx,
        my,
        MAG_SIZE,
        MAG_SIZE,
        state.original.dc(),
        sx,
        sy,
        MAG_CELLS,
        MAG_CELLS,
        SRCCOPY,
    );
    SetStretchBltMode(hdc, old_mode);

    // 网格
    let old_pen = SelectObject(hdc, state.grid_pen);
    for i in 1..MAG_CELLS {
        let x = mx + i * MAG_CELL;
        MoveToEx(hdc, x, my, std::ptr::null_mut());
        LineTo(hdc, x, my + MAG_SIZE);
        let y = my + i * MAG_CELL;
        MoveToEx(hdc, mx, y, std::ptr::null_mut());
        LineTo(hdc, mx + MAG_SIZE, y);
    }
    SelectObject(hdc, old_pen);

    // 蓝色十字线
    let center_x = mx + MAG_SIZE / 2;
    let center_y = my + MAG_SIZE / 2;
    let old_pen = SelectObject(hdc, state.cross_pen);
    MoveToEx(hdc, mx, center_y, std::ptr::null_mut());
    LineTo(hdc, mx + MAG_SIZE - 1, center_y);
    MoveToEx(hdc, center_x, my, std::ptr::null_mut());
    LineTo(hdc, center_x, my + MAG_SIZE - 1);
    SelectObject(hdc, old_pen);

    // 白色外框 + 中心像素框
    let old_pen = SelectObject(hdc, state.white_pen);
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    Rectangle(hdc, px, py, px + pw - 1, py + MAG_SIZE + 1);
    let half = MAG_CELL / 2;
    Rectangle(
        hdc,
        center_x - half,
        center_y - half,
        center_x + half + 1,
        center_y + half + 1,
    );
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);

    // 信息区：坐标 / 色块 + 数值 / 两行提示，全部居中
    SetBkMode(hdc, TRANSPARENT as i32);
    let old_font = SelectObject(hdc, state.font);
    SetTextColor(hdc, rgb(0xff, 0xff, 0xff));

    let info_top = py + MAG_SIZE + 2 + 8;
    let mut coord_rect = RECT {
        left: px + 4,
        top: info_top,
        right: px + pw - 4,
        bottom: info_top + 22,
    };
    let coords = format!(
        "({}, {})",
        cursor.0 + state.origin_x,
        cursor.1 + state.origin_y
    );
    draw_centered(hdc, &coords, &mut coord_rect);

    let text = color_text(color, state.rgb_mode);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut measure: SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut measure);
    let group_width = 14 + 8 + measure.cx;
    let group_x = px + (pw - group_width) / 2;
    let color_top = coord_rect.bottom + 2;
    let swatch = RECT {
        left: group_x,
        top: color_top + 3,
        right: group_x + 14,
        bottom: color_top + 17,
    };
    let brush = CreateSolidBrush(rgb(color.2, color.1, color.0));
    FillRect(hdc, &swatch, brush);
    DeleteObject(brush);
    FrameRect(hdc, &swatch, GetStockObject(WHITE_BRUSH));

    let mut value_rect = RECT {
        left: group_x + 22,
        top: color_top,
        right: px + pw,
        bottom: color_top + 22,
    };
    let wide_z: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    DrawTextW(
        hdc,
        wide_z.as_ptr(),
        -1,
        &mut value_rect,
        DT_LEFT | DT_SINGLELINE | DT_NOPREFIX,
    );

    let hint_top = value_rect.bottom + 2;
    let mut hint1 = RECT {
        left: px + 4,
        top: hint_top,
        right: px + pw - 4,
        bottom: hint_top + 20,
    };
    draw_centered(hdc, "C: 复制颜色值", &mut hint1);
    let mut hint2 = RECT {
        left: px + 4,
        top: hint_top + 20,
        right: px + pw - 4,
        bottom: hint_top + 40,
    };
    draw_centered(hdc, "Shift: 切换 RGB/HEX", &mut hint2);

    SelectObject(hdc, old_font);
}

fn clamp_rect(
    rect: (i32, i32, i32, i32),
    width: i32,
    height: i32,
) -> Option<(i32, i32, i32, i32)> {
    let left = rect.0.clamp(0, width);
    let top = rect.1.clamp(0, height);
    let right = (rect.0 + rect.2).clamp(0, width);
    let bottom = (rect.1 + rect.3).clamp(0, height);
    if right <= left || bottom <= top {
        None
    } else {
        Some((left, top, right - left, bottom - top))
    }
}

fn paint(hdc: HDC) {
    STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return;
        };
        let frame = state.frame_rect();
        let dirty = if !state.painted_once {
            Some((0, 0, state.width, state.height))
        } else {
            union_rect(state.last_frame, frame).and_then(|r| clamp_rect(r, state.width, state.height))
        };
        let Some((dx, dy, dw, dh)) = dirty else {
            return;
        };
        unsafe {
            // 1. 用压暗图恢复变化区域（擦掉上一帧的边框/标签/十字线）。
            BitBlt(state.back.dc(), dx, dy, dw, dh, state.dimmed.dc(), dx, dy, SRCCOPY);

            // 2. 选区原图（只贴与本帧脏区相交的部分）。
            if let Some((x, y, sw, sh)) = state.current_rect() {
                let rx = x.max(dx);
                let ry = y.max(dy);
                let rw = (x + sw).min(dx + dw) - rx;
                let rh = (y + sh).min(dy + dh) - ry;
                if rw > 0 && rh > 0 {
                    BitBlt(state.back.dc(), rx, ry, rw, rh, state.original.dc(), rx, ry, SRCCOPY);
                }
                draw_selection_chrome(state.back.dc(), state.pen, x, y, sw, sh);
                draw_label(
                    state.back.dc(),
                    state.font,
                    state.label_brush,
                    state.origin_x,
                    state.origin_y,
                    x,
                    y,
                    sw,
                    sh,
                );
            }

            // 3. 放大镜（面板整体落在脏区内才画）。
            if state.magnifier_visible() {
                if let Some(cursor) = state.cursor {
                    let panel = state.panel_rect(cursor);
                    let visible = panel.0 < dx + dw
                        && panel.0 + panel.2 > dx
                        && panel.1 < dy + dh
                        && panel.1 + panel.3 > dy;
                    if visible {
                        draw_magnifier(state, state.back.dc(), panel);
                    }
                }
            }

            // 4. 只呈现变化区域；屏幕其余部分本来就是对的。
            BitBlt(hdc, dx, dy, dw, dh, state.back.dc(), dx, dy, SRCCOPY);
        }
        state.painted_once = true;
        state.last_frame = frame;
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let cx = (lp as u16) as i16 as i32;
    let cy = (((lp as u64) >> 16) as u16) as i16 as i32;
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(hdc);
            EndPaint(hwnd, &ps);
            0
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_CROSS));
            1
        }
        WM_MOUSEMOVE => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.cursor = Some((cx, cy));
                    if state.dragging {
                        state.current = Some((cx, cy));
                    }
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.dragging = true;
                    state.anchor = Some((cx, cy));
                    state.current = Some((cx, cy));
                    state.cursor = Some((cx, cy));
                }
            });
            // 立刻收起放大镜，不等下一次鼠标移动。
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_LBUTTONUP => {
            let mut finished = None;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.dragging {
                        state.current = Some((cx, cy));
                        if let Some((x, y, w, h)) = state.current_rect() {
                            if w >= 2 && h >= 2 {
                                finished = Some(Selection {
                                    x,
                                    y,
                                    width: w,
                                    height: h,
                                });
                            }
                        }
                        if finished.is_none() {
                            state.dragging = false;
                            state.anchor = None;
                            state.current = None;
                        } else {
                            state.result = finished;
                        }
                    }
                }
            });
            if finished.is_some() {
                DestroyWindow(hwnd);
            } else {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_RBUTTONUP | WM_MBUTTONUP => {
            DestroyWindow(hwnd);
            0
        }
        WM_KEYDOWN => {
            let key = wp as u16;
            if key == VK_ESCAPE {
                DestroyWindow(hwnd);
            } else if key == 'C' as u16 {
                let text = STATE.with(|cell| {
                    let guard = cell.borrow();
                    let state = guard.as_ref()?;
                    let cursor = state.cursor?;
                    let color = state.pixel_at(cursor)?;
                    Some(color_text(color, state.rgb_mode))
                });
                if let Some(text) = text {
                    let _ = clipboard_image::copy_text(&text);
                    // 与 PixPin 一致：取色并结束本次截图（不保存文件）。
                    DestroyWindow(hwnd);
                }
            }
            0
        }
        WM_KEYUP => {
            // Shift 用抬起事件切换，避免按住时的自动重复来回翻转。
            // 系统可能上报 VK_SHIFT，也可能上报 VK_LSHIFT / VK_RSHIFT。
            let key = wp as u16;
            if key == VK_SHIFT || key == VK_LSHIFT || key == VK_RSHIFT {
                STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.rgb_mode = !state.rgb_mode;
                    }
                });
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            OVERLAY_HWND.store(0, Ordering::SeqCst);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// 抓屏 → 建三张 DIB 段（原图/压暗/后备）→ 覆盖层窗口 → 选区。
pub fn select(width: i32, height: i32) -> Option<OverlayOutcome> {
    let class_name: Vec<u16> = "LightweightToolsetScreenshotOverlay\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "LightweightToolset Screenshot\0".encode_utf16().collect();

    let handle = std::thread::Builder::new()
        .name("screenshot-overlay".to_owned())
        .spawn(move || unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_CROSS),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&wc);

            let original = Surface::new(width, height)?;
            if capture::capture_into(&original).is_err() {
                return None;
            }
            let (origin_x, origin_y, virtual_width, virtual_height) = capture::virtual_rect().ok()?;
            if virtual_width != width || virtual_height != height {
                return None;
            }

            let dimmed = Surface::new(width, height)?;
            BitBlt(dimmed.dc(), 0, 0, width, height, original.dc(), 0, 0, SRCCOPY);
            let mut black = Surface::new(1, 1)?;
            black.set_pixel(0, 0, 0);
            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: DIM_ALPHA,
                AlphaFormat: 0,
            };
            AlphaBlend(
                dimmed.dc(),
                0,
                0,
                width,
                height,
                black.dc(),
                0,
                0,
                1,
                1,
                blend,
            );

            let back = Surface::new(width, height)?;
            BitBlt(back.dc(), 0, 0, width, height, dimmed.dc(), 0, 0, SRCCOPY);

            let font = CreateFontW(
                -17,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                windows_sys::w!("Segoe UI") as *const u16,
            );
            let pen = CreatePen(PS_SOLID, 1, rgb(0x30, 0x7d, 0xef));
            let grid_pen = CreatePen(PS_SOLID, 1, rgb(0x66, 0x66, 0x66));
            let cross_pen = CreatePen(PS_SOLID, 2, rgb(0x30, 0x7d, 0xef));
            let white_pen = CreatePen(PS_SOLID, 1, rgb(0xff, 0xff, 0xff));
            let panel_brush = CreateSolidBrush(rgb(0x00, 0x00, 0x00));
            let label_brush = CreateSolidBrush(rgb(0x00, 0x00, 0x00));

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                origin_x,
                origin_y,
                width,
                height,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                return None;
            }
            // 解绑输入法：否则 Shift 之后的中文输入法会把 C 吞成 VK_PROCESSKEY(229)，
            // 放大镜的 C/Shift 快捷键就失效了。
            ImmAssociateContext(hwnd, std::ptr::null_mut());
            OVERLAY_HWND.store(hwnd as isize, Ordering::SeqCst);
            STATE.with(|cell| {
                *cell.borrow_mut() = Some(OverlayState {
                    width,
                    height,
                    origin_x,
                    origin_y,
                    original,
                    dimmed,
                    back,
                    font,
                    pen,
                    grid_pen,
                    cross_pen,
                    white_pen,
                    panel_brush,
                    label_brush,
                    dragging: false,
                    anchor: None,
                    current: None,
                    cursor: None,
                    rgb_mode: false,
                    last_frame: None,
                    painted_once: false,
                    result: None,
                });
            });

            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
            UpdateWindow(hwnd);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let state = STATE.with(|cell| cell.borrow_mut().take())?;
            let selection = state.result?;
            let surface_stride = width as usize * 4;
            let top_down = crop_top_down(
                state.original.pixels(),
                surface_stride,
                selection.x,
                selection.y,
                selection.width,
                selection.height,
                width,
                height,
            );
            let bottom_up = crop_bottom_up(
                state.original.pixels(),
                surface_stride,
                selection.x,
                selection.y,
                selection.width,
                selection.height,
                width,
                height,
            );
            Some(OverlayOutcome {
                selection,
                top_down,
                bottom_up,
            })
        })
        .ok()?;

    handle.join().ok().flatten()
}

/// 立刻关闭覆盖层（工具被停用时调用）。
pub fn abort() {
    let hwnd = OVERLAY_HWND.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{color_text, crop_bottom_up, crop_top_down, union_rect};

    /// 自顶向下样本：像素值 = y * 10 + x。
    fn sample(width: i32, height: i32) -> Vec<u8> {
        let stride = width as usize * 4;
        let mut buf = vec![0u8; stride * height as usize];
        for y in 0..height {
            for x in 0..width {
                let index = y as usize * stride + x as usize * 4;
                buf[index] = (y * 10 + x) as u8;
                buf[index + 3] = 255;
            }
        }
        buf
    }

    #[test]
    fn crop_top_down_keeps_reading_order() {
        let buf = sample(4, 4);
        let out = crop_top_down(&buf, 16, 1, 1, 2, 2, 4, 4);
        let values: Vec<u8> = out.chunks_exact(4).map(|px| px[0]).collect();
        assert_eq!(values, vec![11, 12, 21, 22]);
    }

    #[test]
    fn crop_bottom_up_flips_rows() {
        let buf = sample(4, 4);
        let out = crop_bottom_up(&buf, 16, 1, 1, 2, 2, 4, 4);
        let values: Vec<u8> = out.chunks_exact(4).map(|px| px[0]).collect();
        assert_eq!(values, vec![21, 22, 11, 12]);
    }

    #[test]
    fn crop_clamps_out_of_bounds() {
        let buf = sample(4, 4);
        let out = crop_top_down(&buf, 16, 3, 3, 10, 10, 4, 4);
        let values: Vec<u8> = out.chunks_exact(4).map(|px| px[0]).collect();
        assert_eq!(values, vec![33]);
    }

    #[test]
    fn union_rect_covers_both() {
        assert_eq!(union_rect(None, None), None);
        assert_eq!(union_rect(Some((1, 2, 3, 4)), None), Some((1, 2, 3, 4)));
        assert_eq!(
            union_rect(Some((10, 10, 10, 10)), Some((5, 5, 10, 10))),
            Some((5, 5, 15, 15))
        );
    }

    #[test]
    fn color_text_switches_format() {
        // 输入是 (B, G, R)。
        assert_eq!(color_text((0xAB, 0xCD, 0xEF), false), "#efcdab");
        assert_eq!(color_text((0x72, 0x63, 0x69), true), "RGB(105, 99, 114)");
    }
}
