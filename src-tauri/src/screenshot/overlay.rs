use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
    BROWSEINFOW,
};
use windows_sys::Win32::UI::Input::Ime::{
    ImmAssociateContext, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
    ImmSetCandidateWindow, ImmSetCompositionWindow, CANDIDATEFORM, COMPOSITIONFORM,
    CFS_CANDIDATEPOS, CFS_POINT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_LSHIFT, VK_MENU, VK_RETURN,
    VK_RSHIFT, VK_SHIFT, VK_Y, VK_Z,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::annotation::{self, build_draft, simplify, text_extent, Document, Item, Shape};
use super::save;
use super::toolbar::{self, Action, Handle};
use super::capture;
use super::clipboard_image;
use super::dib::Surface;

static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
/// 贴图快捷键投递到覆盖层的自定义消息：钉住当前选区。
const WM_OVERLAY_PIN: u32 = WM_APP + 2;


const BORDER: i32 = 2;
const LABEL_HEIGHT: i32 = 24;

/// 放大镜：15×15 像素，每格 16 px（与 PixPin 的放大倍率一致）。
const MAG_CELLS: i32 = 15;
const MAG_CELL: i32 = 16;
const MAG_SIZE: i32 = MAG_CELLS * MAG_CELL;
const PANEL_WIDTH: i32 = MAG_SIZE + 2;
const PANEL_HEIGHT: i32 = 1 + MAG_SIZE + 1 + 8 + 22 + 2 + 22 + 2 + 40 + 6;
/// 面板相对光标的偏移。
const PANEL_OFFSET: i32 = 20;

/// 内联文字编辑状态（不建子窗口，直接画在覆盖层上）。
struct TextEdit {
    origin: (i32, i32),
    text: String,
    composing: String,
}

#[derive(Clone, Copy)]
enum AdjustKind {
    Move,
    Resize(Handle),
}

#[derive(Clone, Copy)]
struct AdjustState {
    kind: AdjustKind,
    start: (i32, i32),
    selection: Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Selecting,
    Annotating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 覆盖层启动参数（来自截图工具设置）。
pub struct OverlayOptions {
    pub manual_dir: PathBuf,
    pub quick_dir: PathBuf,
    pub file_template: String,
    pub dim_color: u32,
    pub dim_alpha: u8,
    /// 放大镜是否显示网格线。
    pub grid: bool,
    pub grid_color: u32,
    /// 辅助线显示模式：始终 / 按 Alt / 不显示。
    pub guides_mode: GuidesMode,
    /// 蓝色辅助线（屏幕中心十字 + 选区系）颜色。
    pub guides_color: u32,
    /// 灰色实线光标十字颜色。
    pub cursor_guide_color: u32,
    /// 蓝色辅助线线宽（1–5）。
    pub guides_width: i32,
    /// 光标十字线宽（1–5）。
    pub cursor_guide_width: i32,
}

impl OverlayOptions {
    /// 兜底参数：默认目录 + 内置外观，正常流程不会用到。
    pub fn defaults(dir: PathBuf) -> Self {
        Self {
            manual_dir: dir.clone(),
            quick_dir: dir,
            file_template: super::save::DEFAULT_TEMPLATE.to_owned(),
            dim_color: 0,
            dim_alpha: 115,
            grid: true,
            grid_color: super::annotation::rgb(0x66, 0x66, 0x66),
            guides_mode: GuidesMode::Alt,
            guides_color: super::annotation::rgb(0x30, 0xB4, 0xFF),
            cursor_guide_color: super::annotation::rgb(0x80, 0x80, 0x80),
            guides_width: 1,
            cursor_guide_width: 1,
        }
    }
}

/// 辅助线显示模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidesMode {
    Always,
    Alt,
    Off,
}

impl GuidesMode {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "off" => Self::Off,
            _ => Self::Alt,
        }
    }
}

pub struct OverlayOutcome {
    pub selection: Selection,
    /// 是否已通过“保存”按钮或 `Ctrl+S` 落盘（mod.rs 据此跳过默认保存）。
    pub saved: bool,
    /// 只复制到剪贴板、不落盘（复制按钮 / Ctrl+C）。
    pub copy_only: bool,
    /// 贴图：把手上的图钉到屏幕上（贴图按钮 / 贴图快捷键）。
    pub pin: bool,
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
    /// 蓝色虚线辅助线（屏幕中心十字 + 选区系）。
    guide_pen: HPEN,
    /// 灰色实线光标十字。
    cursor_guide_pen: HPEN,
    cross_pen: HPEN,
    white_pen: HPEN,
    panel_brush: HBRUSH,
    label_brush: HBRUSH,
    phase: Phase,
    selection: Option<Selection>,
    editing: Option<TextEdit>,
    caret_on: bool,
    adjust: Option<AdjustState>,
    picks: toolbar::Picks,
    style_open: bool,
    line_menu_open: bool,
    arrow_menu_open: bool,
    delete_mode: bool,
    saved: bool,
    copy_only: bool,
    /// 贴图按钮 / 贴图快捷键：结果交给 mod.rs 建贴图窗口。
    pin: bool,
    /// 「保存」按钮弹选择器时的初始目录。
    manual_dir: PathBuf,
    /// `Ctrl+S` 直接写入的目录。
    quick_dir: PathBuf,
    /// 文件名模板。
    file_template: String,
    /// 放大镜网格线开关。
    grid: bool,
    /// 是否启用对齐辅助线。
    guides_mode: GuidesMode,
    /// 上一帧画在窗口 DC 上的辅助线带（用于擦除，不进后备缓冲）。
    guide_bands: Vec<(i32, i32, i32, i32)>,
    /// 蓝色辅助线线宽。
    guide_width: i32,
    /// 光标十字线宽。
    cursor_guide_width: i32,
    /// 当前是否按住 Alt。
    alt_down: bool,
    ime_context: isize,
    doc: Document,
    draft_points: Vec<(i32, i32)>,
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
                self.guide_pen as HGDIOBJ,
                self.cursor_guide_pen as HGDIOBJ,
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
        self.phase == Phase::Selecting && self.cursor.is_some()
    }

    fn toolbar_visible(&self) -> bool {
        self.phase == Phase::Annotating && self.adjust.is_none()
    }

    /// 本帧可能绘制到的全部区域：选区（含边框/标签边距）∪ 放大镜面板。
    fn frame_rect(&self) -> Option<(i32, i32, i32, i32)> {
        if self.phase == Phase::Annotating {
            let sel = self.selection?;
            let mut frame = (
                (sel.x - BORDER - 1).clamp(0, self.width),
                (sel.y - LABEL_HEIGHT - BORDER).clamp(0, self.height),
                (sel.width + 2 * (BORDER + 1) + 1).clamp(0, self.width),
                (sel.height + LABEL_HEIGHT + 2 * BORDER + 1).clamp(0, self.height),
            );
            if self.toolbar_visible() {
                let layout = toolbar::layout(
                    sel,
                    (self.width, self.height),
                    self.doc.tool(),
                    self.style_open,
                    &self.picks,
                    self.line_menu_open,
                    self.arrow_menu_open,
                );
                let bounds = layout.bounds();
                if let Some(union) = union_rect(Some(frame), Some(bounds)) {
                    frame = clamp_rect(union, self.width, self.height).unwrap_or(frame);
                }
            }
            return Some(frame);
        }
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

fn rect_intersects(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// 把辅助线画进某一块区域（调用方已保证内容已恢复）。
unsafe fn draw_guides_in_rect(state: &OverlayState, hdc: HDC, rect: (i32, i32, i32, i32)) {
    let clip = CreateRectRgn(rect.0, rect.1, rect.0 + rect.2, rect.1 + rect.3);
    SelectClipRgn(hdc, clip);
    draw_guides_into(state, hdc);
    SelectClipRgn(hdc, std::ptr::null_mut());
    DeleteObject(clip);
}

/// 这条重绘带是否必须走完整绘制流程（选区内容、标注、控制点、工具栏、放大镜）。
///
/// 光标十字的线带通常远离这些元素，只需要「压暗图 + 辅助线」。
fn band_needs_full_paint(state: &OverlayState, band: (i32, i32, i32, i32)) -> bool {
    if state.editing.is_some() {
        return true;
    }
    if let Some(sel) = guide_rect(state).or(state.selection) {
        // 选区本身 + 上方的尺寸标签 + 两侧控制点余量。
        let expanded = (
            sel.x - 12,
            sel.y - 28,
            sel.width + 24,
            sel.height + 40,
        );
        if rect_intersects(band, expanded) {
            return true;
        }
    }
    if state.magnifier_visible() {
        if let Some(cursor) = state.cursor {
            let panel = state.panel_rect(cursor);
            if rect_intersects(band, (panel.0, panel.1, panel.2, panel.3)) {
                return true;
            }
        }
    }
    if state.toolbar_visible() {
        if let Some(sel) = state.selection {
            let layout = toolbar::layout(
                sel,
                (state.width, state.height),
                state.doc.tool(),
                state.style_open,
                &state.picks,
                state.line_menu_open,
                state.arrow_menu_open,
            );
            if rect_intersects(band, (layout.x, layout.y, layout.width, layout.height)) {
                return true;
            }
            if layout
                .style_bg
                .is_some_and(|bg| rect_intersects(band, bg))
            {
                return true;
            }
            if layout.menus.iter().any(|(_, rect)| {
                rect_intersects(band, (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
            }) {
                return true;
            }
        }
    }
    false
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

/// 弹出目录选择器（模态，必须在不持有 STATE 借用时调用）。
unsafe fn pick_folder(owner: HWND, initial: &std::path::Path) -> Option<PathBuf> {
    let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
    let title: Vec<u16> = "选择保存目录\0".encode_utf16().collect();
    let initial_text: Vec<u16> = format!("{}\0", initial.display()).encode_utf16().collect();
    let mut info: BROWSEINFOW = std::mem::zeroed();
    info.hwndOwner = owner;
    info.lpszTitle = title.as_ptr();
    info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
    info.lpfn = Some(browse_callback);
    info.lParam = initial_text.as_ptr() as isize;
    let pidl = SHBrowseForFolderW(&info);
    let mut result = None;
    if !pidl.is_null() {
        let mut buffer = [0u16; 260];
        if SHGetPathFromIDListW(pidl, buffer.as_mut_ptr()) != 0 {
            let len = buffer.iter().position(|c| *c == 0).unwrap_or(0);
            result = Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len])));
        }
        CoTaskMemFree(pidl as *mut std::ffi::c_void);
    }
    CoUninitialize();
    result
}

/// 目录选择器的初始化回调：对话框建好后把初始目录选上。
unsafe extern "system" fn browse_callback(hwnd: HWND, msg: u32, _lp: LPARAM, data: LPARAM) -> i32 {
    const BFFM_INITIALIZED: u32 = 1;
    const BFFM_SETSELECTIONW: u32 = 0x0467; // WM_USER + 103
    if msg == BFFM_INITIALIZED && data != 0 {
        SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, data);
    }
    0
}

/// 把当前选区 + 标注写成 PNG 到指定目录。
unsafe fn save_current(state: &OverlayState, dir: &std::path::Path) -> Result<PathBuf, String> {
    let selection = state.selection.ok_or_else(|| "没有选区".to_owned())?;
    let (top_down, _) = compose(&state.original, selection, &state.doc.items)
        .ok_or_else(|| "合成失败".to_owned())?;
    let png = save::encode_png(&top_down, selection.width as u32, selection.height as u32)?;
    save::write_png_to(dir, &png, &state.file_template)
}

/// 把选区与标注合成为两张 BGRA（自上而下 / 自下而上）。
unsafe fn compose(
    original: &Surface,
    selection: Selection,
    items: &[Item],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let width = selection.width;
    let height = selection.height;
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut surface = Surface::new(width, height)?;
    BitBlt(
        surface.dc(),
        0,
        0,
        width,
        height,
        original.dc(),
        selection.x,
        selection.y,
        SRCCOPY,
    );
    if !items.is_empty() {
        let region = CreateRectRgn(0, 0, width, height);
        SelectClipRgn(surface.dc(), region);
        annotation::draw(&mut surface, items, None, 0, 0, None);
        SelectClipRgn(surface.dc(), std::ptr::null_mut());
        DeleteObject(region);
    }

    let stride = width as usize * 4;
    let top_down = surface.pixels().to_vec();
    let mut bottom_up = vec![0u8; stride * height as usize];
    for row in 0..height as usize {
        let src = (height as usize - 1 - row) * stride;
        bottom_up[row * stride..(row + 1) * stride].copy_from_slice(&top_down[src..src + stride]);
    }
    Some((top_down, bottom_up))
}

unsafe fn draw_selection_chrome(hdc: HDC, pen: HPEN, x: i32, y: i32, w: i32, h: i32) {
    let old_pen = SelectObject(hdc, pen);
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    Rectangle(hdc, x, y, x + w, y + h);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
}

/// 辅助线是否应当显示（三态 × Alt 状态）。
fn guides_visible(state: &OverlayState) -> bool {
    match state.guides_mode {
        GuidesMode::Always => true,
        GuidesMode::Alt => state.alt_down,
        GuidesMode::Off => false,
    }
}

/// 当前选区（标注阶段用已确定的选区，选择阶段用拖动中的框）。
fn guide_rect(state: &OverlayState) -> Option<Selection> {
    match state.phase {
        Phase::Annotating => state.selection,
        Phase::Selecting => state
            .current_rect()
            .map(|(x, y, w, h)| Selection { x, y, width: w, height: h }),
    }
}

/// 一组辅助线：起点、终点、是否光标十字。
type GuideLine = (i32, i32, i32, i32, bool);

/// 当前应该出现的辅助线（不绘制，只算几何）。
fn guide_lines(state: &OverlayState) -> Vec<GuideLine> {
    if !guides_visible(state) {
        return Vec::new();
    }
    let width = state.width;
    let height = state.height;
    let mut lines: Vec<GuideLine> = Vec::new();
    if let Some(cursor) = state.cursor {
        lines.push((cursor.0, 0, cursor.0, height, true));
        lines.push((0, cursor.1, width, cursor.1, true));
    }
    match guide_rect(state) {
        Some(rect) => {
            let right = rect.x + rect.width;
            let bottom = rect.y + rect.height;
            let center_x = rect.x + rect.width / 2;
            let center_y = rect.y + rect.height / 2;
            lines.extend([
                (rect.x, 0, rect.x, height, false),
                (right, 0, right, height, false),
                (0, rect.y, width, rect.y, false),
                (0, bottom, width, bottom, false),
                (center_x, 0, center_x, height, false),
                (0, center_y, width, center_y, false),
            ]);
        }
        None => {
            let center_x = width / 2;
            let center_y = height / 2;
            lines.extend([
                (center_x, 0, center_x, height, false),
                (0, center_y, width, center_y, false),
            ]);
        }
    }
    lines
}

/// 辅助线的擦除/重绘带：按线宽留边。
fn guide_band_rects(state: &OverlayState) -> Vec<(i32, i32, i32, i32)> {
    let mut bands = Vec::new();
    for (x1, y1, x2, y2, cursor) in guide_lines(state) {
        let line_width = if cursor {
            state.cursor_guide_width
        } else {
            state.guide_width
        };
        let pad = (line_width / 2).max(1) + 1;
        let left = (x1.min(x2) - pad).max(0);
        let top = (y1.min(y2) - pad).max(0);
        let right = (x1.max(x2) + pad).min(state.width);
        let bottom = (y1.max(y2) + pad).min(state.height);
        if right > left && bottom > top {
            let rect = (left, top, right - left, bottom - top);
            if !bands.contains(&rect) {
                bands.push(rect);
            }
        }
    }
    bands
}

/// 把辅助线画进当前 DC（调用方负责裁剪到需要重绘的区域）。
unsafe fn draw_guides_into(state: &OverlayState, hdc: HDC) {
    let lines = guide_lines(state);
    if lines.is_empty() {
        return;
    }
    for (x1, y1, x2, y2, cursor) in lines {
        let pen = if cursor {
            state.cursor_guide_pen
        } else {
            state.guide_pen
        };
        let old_pen = SelectObject(hdc, pen);
        MoveToEx(hdc, x1, y1, std::ptr::null_mut());
        LineTo(hdc, x2, y2);
        SelectObject(hdc, old_pen);
    }
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

    // 网格（可按设置关闭）
    if state.grid {
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
    }

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
        unsafe {
            // 放开 BeginPaint 的更新区裁剪：辅助线区域可能落在更新区之外。
            SelectClipRgn(hdc, std::ptr::null_mut());

            let frame = state.frame_rect();
            let base = if !state.painted_once {
                Some((0, 0, state.width, state.height))
            } else {
                union_rect(state.last_frame, frame)
                    .and_then(|r| clamp_rect(r, state.width, state.height))
            };
            let old_bands = std::mem::take(&mut state.guide_bands);
            let new_bands = guide_band_rects(state);

            // 只重绘变化的辅助线带：没移动的线（例如拖选区时的光标竖线以外的边线）
            // 仍留在后备缓冲/屏幕里，不必每帧重画。
            let mut rects: Vec<(i32, i32, i32, i32)> = Vec::with_capacity(2 + old_bands.len() + new_bands.len());
            if let Some(rect) = base {
                rects.push(rect);
            }
            for rect in &old_bands {
                if !new_bands.contains(rect) && !rects.contains(rect) {
                    rects.push(*rect);
                }
            }
            for rect in &new_bands {
                if !old_bands.contains(rect) && !rects.contains(rect) {
                    rects.push(*rect);
                }
            }

            for rect in &rects {
                paint_rect(state, hdc, *rect);
            }

            state.guide_bands = new_bands;
            state.painted_once = true;
            state.last_frame = frame;
        }
    });
}

/// 重绘一块区域（重建后备缓冲 + 呈现），最后把辅助线画进这一块。
unsafe fn paint_rect(state: &mut OverlayState, hdc: HDC, dirty: (i32, i32, i32, i32)) {
        let (dx, dy, dw, dh) = dirty;
            // 1. 用压暗图恢复变化区域（擦掉上一帧的边框/标签/放大镜/标注）。
            BitBlt(state.back.dc(), dx, dy, dw, dh, state.dimmed.dc(), dx, dy, SRCCOPY);

            // 远离选区和工具栏的线带只需要「压暗图 + 辅助线」，直接呈现，省掉整条绘制流程。
            if !band_needs_full_paint(state, dirty) {
                draw_guides_in_rect(state, state.back.dc(), dirty);
                BitBlt(hdc, dx, dy, dw, dh, state.back.dc(), dx, dy, SRCCOPY);
                return;
            }

            if state.phase == Phase::Annotating {
                let Some(sel) = state.selection else {
                    return;
                };
                // 选区原图
                let rx = sel.x.max(dx);
                let ry = sel.y.max(dy);
                let rw = (sel.x + sel.width).min(dx + dw) - rx;
                let rh = (sel.y + sel.height).min(dy + dh) - ry;
                if rw > 0 && rh > 0 {
                    BitBlt(state.back.dc(), rx, ry, rw, rh, state.original.dc(), rx, ry, SRCCOPY);
                }
                draw_selection_chrome(
                    state.back.dc(),
                    state.pen,
                    sel.x,
                    sel.y,
                    sel.width,
                    sel.height,
                );
                draw_label(
                    state.back.dc(),
                    state.font,
                    state.label_brush,
                    state.origin_x,
                    state.origin_y,
                    sel.x,
                    sel.y,
                    sel.width,
                    sel.height,
                );

                // 标注只画在选区内
                let rgn = CreateRectRgn(sel.x, sel.y, sel.x + sel.width, sel.y + sel.height);
                SelectClipRgn(state.back.dc(), rgn);
                let draft = state.doc.tool().and_then(|tool| {
                    let draft_cursor = if tool == Shape::Circle {
                        state.draft_points.first().map(|anchor| {
                            square_cursor(sel, *anchor, state.cursor.unwrap_or(*anchor))
                        })
                    } else {
                        state.cursor
                    };
                    build_draft(
                        tool,
                        &state.draft_points,
                        draft_cursor,
                        (sel.x, sel.y),
                        state.doc.color(),
                        state.doc.width(),
                        state.doc.fill(),
                        state.doc.line_style(matches!(
                            state.doc.tool(),
                            Some(Shape::Rect | Shape::Ellipse | Shape::Circle)
                        )),
                        if state.doc.tool() == Some(Shape::Arrow) {
                            state.doc.arrow_style()
                        } else {
                            0
                        },
                        state.doc.strength(state.doc.tool() != Some(Shape::Blur)),
                        state.doc.opacity(),
                    )
                });
                annotation::draw(
                    &mut state.back,
                    &state.doc.items,
                    state.doc.selected,
                    sel.x,
                    sel.y,
                    draft.as_ref(),
                );
                if let Some(editing) = state.editing.as_ref() {
                    let mut full = editing.text.clone();
                    full.push_str(&editing.composing);
                    let (width, height) = text_extent(&full, state.doc.font_size());
                    if !full.is_empty() {
                        let mut draft = Item::new(
                            Shape::Text,
                            vec![(
                                editing.origin.0 - sel.x,
                                editing.origin.1 - sel.y,
                            )],
                            state.doc.color(),
                            state.doc.width(),
                        );
                        draft.text = full.clone();
                        draft.width = state.doc.font_size();
                        annotation::draw(
                            &mut state.back,
                            &[],
                            None,
                            sel.x,
                            sel.y,
                            Some(&draft),
                        );
                    }

                    if !editing.composing.is_empty() {
                        let (committed_width, _) = text_extent(&editing.text, state.doc.font_size());
                        let underline = CreatePen(PS_SOLID, 1, state.doc.color());
                        let old_pen = SelectObject(state.back.dc(), underline);
                        MoveToEx(
                            state.back.dc(),
                            editing.origin.0 + committed_width,
                            editing.origin.1 + height + 1,
                            std::ptr::null_mut(),
                        );
                        LineTo(
                            state.back.dc(),
                            editing.origin.0 + width,
                            editing.origin.1 + height + 1,
                        );
                        SelectObject(state.back.dc(), old_pen);
                        DeleteObject(underline);
                    }

                    // 输入框：外深内浅双层 1px，明暗背景都能看清；内边距加大。
                    let pad_x = 10;
                    let pad_y = 7;
                    let (left, top) = (editing.origin.0 - pad_x, editing.origin.1 - pad_y);
                    let (right, bottom) = (
                        editing.origin.0 + width + pad_x,
                        editing.origin.1 + height + pad_y,
                    );
                    let old_brush = SelectObject(state.back.dc(), GetStockObject(NULL_BRUSH));
                    for (inset, color) in [(1, rgb(0x60, 0x60, 0x60)), (0, rgb(0xf0, 0xf0, 0xf0))] {
                        let pen = CreatePen(PS_SOLID, 1, color);
                        let old_pen = SelectObject(state.back.dc(), pen);
                        Rectangle(
                            state.back.dc(),
                            left - inset,
                            top - inset,
                            right + inset,
                            bottom + inset,
                        );
                        SelectObject(state.back.dc(), old_pen);
                        DeleteObject(pen);
                    }
                    SelectObject(state.back.dc(), old_brush);

                    if state.caret_on {
                        // 光标：贴最后一个字右侧，纵向对齐字形（下移 2px）；
                        // 深-浅-深三线保证白底、深底都清晰。
                        let caret_x = editing.origin.0 + width + 2;
                        let (top, bottom) =
                            (editing.origin.1 + 2, editing.origin.1 + height - 1);
                        let strokes = [
                            (caret_x - 1, 1, rgb(0x30, 0x30, 0x30)),
                            (caret_x + 2, 1, rgb(0x30, 0x30, 0x30)),
                            (caret_x, 2, rgb(0xfa, 0xfa, 0xfa)),
                        ];
                        for (x, thickness, color) in strokes {
                            let pen = CreatePen(PS_SOLID, thickness, color);
                            let old_pen = SelectObject(state.back.dc(), pen);
                            MoveToEx(state.back.dc(), x, top, std::ptr::null_mut());
                            LineTo(state.back.dc(), x, bottom);
                            SelectObject(state.back.dc(), old_pen);
                            DeleteObject(pen);
                        }
                    }
                }
                SelectClipRgn(state.back.dc(), std::ptr::null_mut());
                DeleteObject(rgn);

                // 辅助线画在选区内容之上、控制点/工具栏之下。
                draw_guides_in_rect(state, state.back.dc(), dirty);

                // 选区控制点（只画与当前重绘块相交的）
                let band = (dx, dy, dw, dh);
                if state.toolbar_visible() {
                    for (hx, hy) in toolbar::handle_points(sel) {
                        if !rect_intersects(band, (hx - 4, hy - 4, 10, 10)) {
                            continue;
                        }
                        let rect = RECT {
                            left: hx - 4,
                            top: hy - 4,
                            right: hx + 5,
                            bottom: hy + 5,
                        };
                        let brush = CreateSolidBrush(rgb(0xff, 0xff, 0xff));
                        FillRect(state.back.dc(), &rect, brush);
                        DeleteObject(brush);
                        let pen = CreatePen(PS_SOLID, 1, rgb(0x30, 0x30, 0x30));
                        let old_pen = SelectObject(state.back.dc(), pen);
                        let old_brush =
                            SelectObject(state.back.dc(), GetStockObject(NULL_BRUSH));
                        Rectangle(
                            state.back.dc(),
                            rect.left,
                            rect.top,
                            rect.right,
                            rect.bottom,
                        );
                        SelectObject(state.back.dc(), old_brush);
                        SelectObject(state.back.dc(), old_pen);
                        DeleteObject(pen);
                    }

                    // 工具栏只在与重绘块相交时才重画：否则每条辅助线带都要画一遍图标，明显拖慢。
                    let layout = toolbar::layout(
                        sel,
                        (state.width, state.height),
                        state.doc.tool(),
                        state.style_open,
                        &state.picks,
                        state.line_menu_open,
                        state.arrow_menu_open,
                    );
                    let bar_hit = rect_intersects(
                        band,
                        (layout.x, layout.y, layout.width, layout.height),
                    ) || layout
                        .style_bg
                        .is_some_and(|bg| rect_intersects(band, bg))
                        || layout.menus.iter().any(|(_, rect)| {
                            rect_intersects(band, (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
                        });
                    if bar_hit {
                        let toolbar_state = toolbar::ToolbarState {
                            selection: sel,
                            screen: (state.width, state.height),
                            cursor: state.cursor,
                            tool: state.doc.tool(),
                            picks: state.picks,
                            style_open: state.style_open,
                            color_index: state.doc.color_index(),
                            width: state.doc.width(),
                            fill: state.doc.fill(),
                            strength: state.doc.strength(state.doc.tool() != Some(Shape::Blur)),
                            opacity: state.doc.opacity(),
                            font_size: state.doc.font_size(),
                            serial_size: state.doc.serial_size(),
                            line_style: state.doc.line_style(matches!(
                                state.doc.tool(),
                                Some(Shape::Rect | Shape::Ellipse | Shape::Circle)
                            )),
                            arrow_style: state.doc.arrow_style(),
                            line_menu_open: state.line_menu_open,
                            arrow_menu_open: state.arrow_menu_open,
                            delete_mode: state.delete_mode,
                        };
                        toolbar::draw(state.back.dc(), &toolbar_state);
                    }
                }
            } else if let Some(w) = state.current_rect() {
                let (x, y, sw, sh) = w;
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

            // 1.5 选择阶段的辅助线（标注阶段已在控制点之前画过）。
            if state.phase == Phase::Selecting {
                draw_guides_in_rect(state, state.back.dc(), dirty);
            }

            // 2. 放大镜（仅选区阶段；面板整体落在脏区内才画）。
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

            // 3. 只呈现变化区域；屏幕其余部分本来就是对的。
            BitBlt(hdc, dx, dy, dw, dh, state.back.dc(), dx, dy, SRCCOPY);
}

/// 正圆：把鼠标终点约束成"以锚点为角、边长取较大拖距"的正方形对角，并夹在选区内。
fn square_cursor(sel: Selection, anchor: (i32, i32), cursor: (i32, i32)) -> (i32, i32) {
    let ax = anchor.0.clamp(sel.x, sel.x + sel.width);
    let ay = anchor.1.clamp(sel.y, sel.y + sel.height);
    let dx = cursor.0 - ax;
    let dy = cursor.1 - ay;
    let sx = if dx >= 0 { 1 } else { -1 };
    let sy = if dy >= 0 { 1 } else { -1 };
    let limit_x = if sx > 0 {
        sel.x + sel.width - ax
    } else {
        ax - sel.x
    };
    let limit_y = if sy > 0 {
        sel.y + sel.height - ay
    } else {
        ay - sel.y
    };
    let side = dx
        .abs()
        .max(dy.abs())
        .min(limit_x.max(0))
        .min(limit_y.max(0));
    (ax + sx * side, ay + sy * side)
}

/// 开始内联文字编辑：接管 IME，键盘输入直接进缓冲区。
unsafe fn start_text_edit(hwnd: HWND, state: &mut OverlayState, cx: i32, cy: i32) {
    state.editing = Some(TextEdit {
        origin: (cx, cy),
        text: String::new(),
        composing: String::new(),
    });
    state.caret_on = true;
    CreateCaret(hwnd, std::ptr::null_mut(), 1, 1);
    SetTimer(hwnd, 1, 530, None);
    update_ime_position(hwnd, state);
    if state.ime_context != 0 {
        ImmAssociateContext(hwnd, state.ime_context as *mut std::ffi::c_void);
    }
}

/// 结束编辑；有文字则返回待加入文档的文字项。
unsafe fn finish_text_edit(hwnd: HWND, state: &mut OverlayState) -> Option<Item> {
    let editing = state.editing.take()?;
    KillTimer(hwnd, 1);
    DestroyCaret();
    state.caret_on = false;
    ImmAssociateContext(hwnd, std::ptr::null_mut());
    let text = editing.text.trim().to_owned();
    if text.is_empty() {
        return None;
    }
    let selection = state.selection?;
    let mut item = Item::new(
        Shape::Text,
        vec![(
            editing.origin.0 - selection.x,
            editing.origin.1 - selection.y,
        )],
        state.doc.color(),
        state.doc.width(),
    );
    item.text = text;
    Some(item)
}

unsafe fn cancel_text_edit(hwnd: HWND, state: &mut OverlayState) {
    if state.editing.take().is_some() {
        KillTimer(hwnd, 1);
        DestroyCaret();
        state.caret_on = false;
        ImmAssociateContext(hwnd, std::ptr::null_mut());
    }
}

/// 读取 IME 组合串的辅助：`flag` 为 GCS_COMPSTR 或 GCS_RESULTSTR。
unsafe fn ime_string(hwnd: HWND, flag: u32) -> Option<String> {
    let himc = ImmGetContext(hwnd);
    if himc.is_null() {
        return None;
    }
    let length = ImmGetCompositionStringW(himc, flag, std::ptr::null_mut(), 0);
    if length <= 0 {
        ImmReleaseContext(hwnd, himc);
        return None;
    }
    let mut buffer = vec![0u16; (length as usize + 1) / 2];
    let copied = ImmGetCompositionStringW(
        himc,
        flag,
        buffer.as_mut_ptr() as *mut std::ffi::c_void,
        length as u32,
    );
    ImmReleaseContext(hwnd, himc);
    if copied <= 0 {
        return None;
    }
    let units = (copied as usize) / 2;
    Some(String::from_utf16_lossy(&buffer[..units]))
}

/// 当前光标（客户区）位置，用于 IME 组合窗/候选窗定位。
unsafe fn ime_caret(state: &OverlayState) -> Option<POINT> {
    let editing = state.editing.as_ref()?;
    let mut full = editing.text.clone();
    full.push_str(&editing.composing);
    let (width, _) = text_extent(&full, state.doc.font_size());
    Some(POINT {
        x: editing.origin.0 + width + 1,
        y: editing.origin.1 + annotation::TEXT_HEIGHT,
    })
}

/// 把输入法候选窗/组合窗贴到光标下方，并同步隐藏系统光标位置。
unsafe fn update_ime_position(hwnd: HWND, state: &OverlayState) {
    let Some(point) = ime_caret(state) else {
        return;
    };
    SetCaretPos(point.x, point.y);
    let himc = ImmGetContext(hwnd);
    if himc.is_null() {
        return;
    }
    let mut composition: COMPOSITIONFORM = std::mem::zeroed();
    composition.dwStyle = CFS_POINT;
    composition.ptCurrentPos = point;
    ImmSetCompositionWindow(himc, &composition);

    let mut candidate: CANDIDATEFORM = std::mem::zeroed();
    candidate.dwIndex = 0;
    candidate.dwStyle = CFS_CANDIDATEPOS;
    candidate.ptCurrentPos = point;
    ImmSetCandidateWindow(himc, &candidate);
    ImmReleaseContext(hwnd, himc);
}

/// 组合态（拼音未上屏）时按键应交给输入法，而不是我们自己处理。
fn composing_active(state: &OverlayState) -> bool {
    state
        .editing
        .as_ref()
        .map(|editing| !editing.composing.is_empty())
        .unwrap_or(false)
}

/// 结束当前草稿：拖拽型用 `end` 作为终点，铅笔/折线用已收集的点。
fn finalize_draft(state: &mut OverlayState, end: Option<(i32, i32)>) {
    let Some(sel) = state.selection else {
        return;
    };
    if state.draft_points.is_empty() {
        return;
    }
    let Some(tool) = state.doc.tool() else {
        state.draft_points.clear();
        return;
    };
    let clamp = |value: i32, lo: i32, hi: i32| value.clamp(lo, hi);
    let to_image = |p: (i32, i32)| {
        (
            clamp(p.0, sel.x, sel.x + sel.width) - sel.x,
            clamp(p.1, sel.y, sel.y + sel.height) - sel.y,
        )
    };

    match tool {
        Shape::Polyline => {
            let mut points: Vec<(i32, i32)> = Vec::new();
            for p in state.draft_points.drain(..) {
                let image = to_image(p);
                if points.last().copied() != Some(image) {
                    points.push(image);
                }
            }
            if points.len() >= 2 {
                state.doc.push(Item::new(
                    Shape::Polyline,
                    points,
                    state.doc.color(),
                    state.doc.width(),
                ));
            }
        }
        Shape::Pencil | Shape::Highlighter => {
            let raw: Vec<(i32, i32)> = state.draft_points.drain(..).map(to_image).collect();
            if raw.len() >= 2 {
                let points = simplify(&raw, 1.5);
                state.doc.push(Item::new(
                    tool,
                    points,
                    state.doc.color(),
                    state.doc.width(),
                ));
            }
        }
        _ => {
            let anchor = state.draft_points[0];
            let raw_end = end.unwrap_or(anchor);
            let stop_screen = if tool == Shape::Circle {
                square_cursor(sel, anchor, raw_end)
            } else {
                raw_end
            };
            let start = to_image(anchor);
            let stop = to_image(stop_screen);
            state.draft_points.clear();
            let width = (start.0 - stop.0).abs();
            let height = (start.1 - stop.1).abs();
            let ok = if matches!(tool, Shape::Line | Shape::Arrow) {
                width + height >= 4
            } else {
                width >= 2 && height >= 2
            };
            if ok {
                state.doc.push(Item::new(
                    tool,
                    vec![start, stop],
                    state.doc.color(),
                    state.doc.width(),
                ));
            }
        }
    }
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
                    if let Some(adjust) = state.adjust {
                        if let Some(current) = state.selection {
                            let screen = (state.width, state.height);
                            let updated = match adjust.kind {
                                AdjustKind::Move => toolbar::translate(
                                    adjust.selection,
                                    cx - adjust.start.0,
                                    cy - adjust.start.1,
                                    screen,
                                ),
                                AdjustKind::Resize(handle) => {
                                    toolbar::resize(adjust.selection, handle, cx, cy, screen)
                                }
                            };
                            if updated != current {
                                let dx = updated.x - current.x;
                                let dy = updated.y - current.y;
                                if dx != 0 || dy != 0 {
                                    for item in &mut state.doc.items {
                                        for point in &mut item.points {
                                            point.0 -= dx;
                                            point.1 -= dy;
                                        }
                                    }
                                }
                                state.selection = Some(updated);
                            }
                        }
                        return;
                    }
                    if state.phase == Phase::Selecting && state.dragging {
                        state.current = Some((cx, cy));
                    }
                    if state.phase == Phase::Annotating
                        && matches!(state.doc.tool(), Some(Shape::Pencil | Shape::Highlighter))
                    {
                        if let Some(last) = state.draft_points.last().copied() {
                            if (cx - last.0).abs() + (cy - last.1).abs() >= 2 {
                                state.draft_points.push((cx, cy));
                            }
                        }
                    }
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            let mut destroy_now = false;
            let mut save_requested = false;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    match state.phase {
                        Phase::Selecting => {
                            state.dragging = true;
                            state.anchor = Some((cx, cy));
                            state.current = Some((cx, cy));
                            state.cursor = Some((cx, cy));
                        }
                        Phase::Annotating => {
                            if state.editing.is_some() {
                                if let Some(item) = finish_text_edit(hwnd, state) {
                                    state.doc.push(item);
                                }
                            }
                            if let Some(sel) = state.selection {
                                if state.toolbar_visible() {
                                    let layout = toolbar::layout(
                                        sel,
                                        (state.width, state.height),
                                        state.doc.tool(),
                                        state.style_open,
                                        &state.picks,
                                        state.line_menu_open,
                                        state.arrow_menu_open,
                                    );
                                    let hit = toolbar::hit(&layout, cx, cy);
                                    if state.line_menu_open || state.arrow_menu_open {
                                        let keep = matches!(
                                            hit,
                                            Some(Action::LineStyle)
                                                | Some(Action::ArrowType)
                                                | Some(Action::LineStylePick(_))
                                                | Some(Action::ArrowPick(_))
                                        );
                                        if !keep {
                                            state.line_menu_open = false;
                                            state.arrow_menu_open = false;
                                        }
                                    }
                                    if let Some(action) = hit {
                                        if !matches!(
                                            action,
                                            Action::LineStyle
                                                | Action::ArrowType
                                                | Action::LineStylePick(_)
                                                | Action::ArrowPick(_)
                                        ) {
                                            state.line_menu_open = false;
                                            state.arrow_menu_open = false;
                                        }
                                        match action {
                                            Action::Group(group) => {
                                                state.delete_mode = false;
                                                let same = state
                                                    .doc
                                                    .tool()
                                                    .and_then(toolbar::group_of)
                                                    == Some(group);
                                                if same && state.style_open {
                                                    state.doc.clear_tool();
                                                    state.style_open = false;
                                                    state.line_menu_open = false;
                                                    state.arrow_menu_open = false;
                                                } else {
                                                    let shape = state.picks.current(group);
                                                    state.doc.set_tool(shape);
                                                    state.style_open = true;
                                                }
                                                state.draft_points.clear();
                                            }
                                            Action::Pick(group, shape) => {
                                                state.delete_mode = false;
                                                state.picks.set(group, shape);
                                                state.doc.set_tool(shape);
                                                state.style_open = true;
                                                state.line_menu_open = false;
                                                state.arrow_menu_open = false;
                                                state.draft_points.clear();
                                            }
                                            Action::Tool(shape) => {
                                                state.delete_mode = false;
                                                state.doc.toggle_tool(shape);
                                                state.style_open =
                                                    state.doc.tool() == Some(shape);
                                                state.line_menu_open = false;
                                                state.arrow_menu_open = false;
                                                state.draft_points.clear();
                                            }
                                            Action::Color(index) => state.doc.set_color(index),
                                            Action::Width => {}
                                            Action::Fill => state.doc.toggle_fill(),
                                            Action::LineStyle => {
                                                state.line_menu_open = !state.line_menu_open;
                                                state.arrow_menu_open = false;
                                            }
                                            Action::ArrowType => {
                                                state.arrow_menu_open = !state.arrow_menu_open;
                                                state.line_menu_open = false;
                                            }
                                            Action::LineStylePick(style) => {
                                                let shapes = matches!(
                                                    state.doc.tool(),
                                                    Some(Shape::Rect | Shape::Ellipse | Shape::Circle)
                                                );
                                                state.doc.set_line_style(shapes, style);
                                                state.line_menu_open = false;
                                            }
                                            Action::ArrowPick(style) => {
                                                state.doc.set_arrow_style(style);
                                                state.arrow_menu_open = false;
                                            }
                                            Action::Strength => {}
                                            Action::Opacity => {}
                                            Action::FontSize => {}
                                            Action::SerialSize => {}
                                            Action::PixelMode(shape) => {
                                                state.picks.set(toolbar::Group::Pixels, shape);
                                                state.doc.set_tool(shape);
                                                state.style_open = true;
                                            }
                                            Action::Undo => {
                                                state.doc.undo();
                                            }
                                            Action::Redo => {
                                                state.doc.redo();
                                            }
                                            Action::Delete => {
                                                state.delete_mode = !state.delete_mode;
                                            }
                                            Action::Save => {
                                                // 目录选择器是模态对话框，必须释放借用后再弹。
                                                save_requested = true;
                                            }
                                            Action::Copy => {
                                                state.result = Some(sel);
                                                state.copy_only = true;
                                                destroy_now = true;
                                            }
                                            Action::Pin => {
                                                state.result = Some(sel);
                                                state.pin = true;
                                                destroy_now = true;
                                            }
                                            Action::Cancel => {
                                                destroy_now = true;
                                            }
                                        }
                                        return;
                                    }
                                }
                                if state.delete_mode {
                                    let lx = cx - sel.x;
                                    let ly = cy - sel.y;
                                    if lx >= 0 && ly >= 0 && lx <= sel.width && ly <= sel.height {
                                        if let Some(index) = state.doc.hit_test(lx, ly) {
                                            state.doc.delete_at(index);
                                        }
                                    }
                                    return;
                                }
                                if toolbar::handle_at(sel, cx, cy).is_some()
                                    || toolbar::border_at(sel, cx, cy)
                                {
                                    let kind = match toolbar::handle_at(sel, cx, cy) {
                                        Some(handle) => AdjustKind::Resize(handle),
                                        None => AdjustKind::Move,
                                    };
                                    state.adjust = Some(AdjustState {
                                        kind,
                                        start: (cx, cy),
                                        selection: sel,
                                    });
                                    state.draft_points.clear();
                                    return;
                                }
                                let lx = cx - sel.x;
                                let ly = cy - sel.y;
                                if lx >= 0 && ly >= 0 && lx <= sel.width && ly <= sel.height {
                                    let tool = state.doc.tool();
                                    if tool.is_none() {
                                        state.adjust = Some(AdjustState {
                                            kind: AdjustKind::Move,
                                            start: (cx, cy),
                                            selection: sel,
                                        });
                                        return;
                                    }
                                    if tool == Some(Shape::Text) {
                                        start_text_edit(hwnd, state, cx, cy);
                                        return;
                                    }
                                    if tool == Some(Shape::Serial) {
                                        let number = state.doc.next_serial();
                                        let mut item = Item::new(
                                            Shape::Serial,
                                            vec![(lx, ly)],
                                            state.doc.color(),
                                            state.doc.width(),
                                        );
                                        item.text = number.to_string();
                                        state.doc.push(item);
                                        return;
                                    }
                                    if let Some(index) = state.doc.hit_test(lx, ly) {
                                        state.doc.select(Some(index));
                                        state.draft_points.clear();
                                    } else {
                                        state.doc.select(None);
                                        let tool = state.doc.tool().unwrap_or(Shape::Rect);
                                        if tool.is_path_shape() {
                                            state.draft_points.push((cx, cy));
                                        } else {
                                            state.draft_points.clear();
                                            state.draft_points.push((cx, cy));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });
            if save_requested {
                // 选择器是模态对话框，必须先把初始目录取出来、释放 STATE 借用再弹。
                let initial = STATE
                    .with(|cell| cell.borrow().as_ref().map(|state| state.manual_dir.clone()))
                    .unwrap_or_default();
                if let Some(dir) = pick_folder(hwnd, &initial) {
                    let finished = STATE.with(|cell| {
                        let mut guard = cell.borrow_mut();
                        if let Some(state) = guard.as_mut() {
                            if let Some(selection) = state.selection {
                                if save_current(state, &dir).is_ok() {
                                    state.result = Some(selection);
                                    state.saved = true;
                                    return true;
                                }
                            }
                        }
                        false
                    });
                    if finished {
                        DestroyWindow(hwnd);
                        return 0;
                    }
                }
                return 0;
            }
            if destroy_now {
                DestroyWindow(hwnd);
                return 0;
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_LBUTTONUP => {
            let mut repaint = false;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    match state.phase {
                        Phase::Selecting => {
                            if state.dragging {
                                state.current = Some((cx, cy));
                                if let Some((x, y, w, h)) = state.current_rect() {
                                    if w >= 2 && h >= 2 {
                                        state.selection = Some(Selection {
                                            x,
                                            y,
                                            width: w,
                                            height: h,
                                        });
                                        state.phase = Phase::Annotating;
                                        repaint = true;
                                    }
                                }
                                state.dragging = false;
                                state.anchor = None;
                                state.current = None;
                            }
                        }
                        Phase::Annotating => {
                            if state.adjust.is_some() {
                                state.adjust = None;
                                repaint = true;
                                return;
                            }
                            if state.editing.is_some() {
                                return;
                            }
                            // 折线点击加点，不在这里收尾（回车/双击收尾）。
                            if state.doc.tool() != Some(Shape::Polyline) {
                                finalize_draft(state, Some((cx, cy)));
                                repaint = true;
                            }
                        }
                    }
                }
            });
            if repaint {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.phase == Phase::Annotating
                        && state.doc.tool() == Some(Shape::Polyline)
                    {
                        finalize_draft(state, None);
                    }
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_RBUTTONUP | WM_MBUTTONUP => {
            let finish_polyline = STATE.with(|cell| {
                let mut guard = cell.borrow_mut();
                if let Some(state) = guard.as_mut() {
                    if state.phase == Phase::Annotating
                        && state.doc.tool() == Some(Shape::Polyline)
                        && !state.draft_points.is_empty()
                    {
                        finalize_draft(state, None);
                        return true;
                    }
                }
                false
            });
            if finish_polyline {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                return 0;
            }
            DestroyWindow(hwnd);
            0
        }
        // Alt 走 SYSKEYDOWN/UP：用来切换对齐辅助线（其余 Alt 组合不处理）。
        WM_SYSKEYDOWN => {
            if wp as u16 == VK_MENU {
                let changed = STATE.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    if let Some(state) = guard.as_mut() {
                        if !state.alt_down {
                            state.alt_down = true;
                            return true;
                        }
                    }
                    false
                });
                if changed {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_SYSKEYUP => {
            if wp as u16 == VK_MENU {
                let changed = STATE.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    if let Some(state) = guard.as_mut() {
                        if state.alt_down {
                            state.alt_down = false;
                            return true;
                        }
                    }
                    false
                });
                if changed {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_KEYDOWN => {
            let key = wp as u16;
            if GetKeyState(VK_CONTROL as i32) < 0 && key == 0x53 {
                let finished = STATE.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    if let Some(state) = guard.as_mut() {
                        if state.phase == Phase::Annotating {
                            if let Some(selection) = state.selection {
                                let dir = state.quick_dir.clone();
                                if save_current(state, &dir).is_ok() {
                                    state.result = Some(selection);
                                    state.saved = true;
                                    return true;
                                }
                            }
                        }
                    }
                    false
                });
                if finished {
                    DestroyWindow(hwnd);
                    return 0;
                }
                return 0;
            }
            if GetKeyState(VK_CONTROL as i32) < 0 && key == 0x43 {
                let finished = STATE.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    if let Some(state) = guard.as_mut() {
                        if state.phase == Phase::Annotating {
                            if let Some(selection) = state.selection {
                                state.result = Some(selection);
                                state.copy_only = true;
                                return true;
                            }
                        }
                    }
                    false
                });
                if finished {
                    DestroyWindow(hwnd);
                    return 0;
                }
                return 0;
            }
            let editing = STATE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .map(|state| state.editing.is_some())
                    .unwrap_or(false)
            });
            if editing {
                let composing = STATE.with(|cell| {
                    cell.borrow().as_ref().map(composing_active).unwrap_or(false)
                });
                if composing {
                    // 拼音未上屏：回车选词、退格删拼音都交给输入法。
                    return 0;
                }
                match key {
                    VK_ESCAPE => {
                        STATE.with(|cell| {
                            if let Some(state) = cell.borrow_mut().as_mut() {
                                cancel_text_edit(hwnd, state);
                            }
                        });
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return 0;
                    }
                    VK_RETURN => {
                        STATE.with(|cell| {
                            if let Some(state) = cell.borrow_mut().as_mut() {
                                if let Some(item) = finish_text_edit(hwnd, state) {
                                    state.doc.push(item);
                                }
                            }
                        });
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return 0;
                    }
                    0x08 => {
                        STATE.with(|cell| {
                            if let Some(state) = cell.borrow_mut().as_mut() {
                                state.caret_on = true;
                                SetTimer(hwnd, 1, 530, None);
                                if let Some(editing) = state.editing.as_mut() {
                                    editing.text.pop();
                                }
                                update_ime_position(hwnd, state);
                            }
                        });
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return 0;
                    }
                    _ => return 0,
                }
            }
            if key == VK_ESCAPE {
                DestroyWindow(hwnd);
                return 0;
            }
            let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
            let mut copy_text_value: Option<String> = None;
            let mut finish = false;
            let mut repaint = false;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    match state.phase {
                        Phase::Selecting => {
                            if key == 'C' as u16 {
                                if let Some(cursor) = state.cursor {
                                    if let Some(color) = state.pixel_at(cursor) {
                                        copy_text_value = Some(color_text(color, state.rgb_mode));
                                    }
                                }
                            }
                        }
                        // 工具切换、换色、线宽只走工具栏与二级栏点选，不再保留字母/数字单键。
                        Phase::Annotating => match key {
                            VK_DELETE => {
                                state.doc.delete_selected();
                                repaint = true;
                            }
                            VK_RETURN => {
                                if !state.draft_points.is_empty() {
                                    finalize_draft(state, state.cursor);
                                }
                                finish = true;
                            }
                            VK_Z if ctrl => {
                                state.doc.undo();
                                repaint = true;
                            }
                            VK_Y if ctrl => {
                                state.doc.redo();
                                repaint = true;
                            }
                            _ => {}
                        },
                    }
                }
            });
            if let Some(text) = copy_text_value {
                let _ = clipboard_image::copy_text(&text);
                DestroyWindow(hwnd);
                return 0;
            }
            if finish {
                let selection = STATE.with(|cell| cell.borrow().as_ref().and_then(|s| s.selection));
                if let Some(selection) = selection {
                    STATE.with(|cell| {
                        if let Some(state) = cell.borrow_mut().as_mut() {
                            state.result = Some(selection);
                        }
                    });
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            if repaint {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_CHAR => {
            let composing = STATE.with(|cell| {
                cell.borrow().as_ref().map(composing_active).unwrap_or(false)
            });
            if composing {
                return 0;
            }
            let ch = wp as u32;
            if let Some(c) = char::from_u32(ch) {
                if !c.is_control() {
                    STATE.with(|cell| {
                        if let Some(state) = cell.borrow_mut().as_mut() {
                            state.caret_on = true;
                            SetTimer(hwnd, 1, 530, None);
                            if let Some(editing) = state.editing.as_mut() {
                                editing.text.push(c);
                            }
                            update_ime_position(hwnd, state);
                        }
                    });
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_IME_CHAR => {
            // 组合结果统一走 WM_IME_COMPOSITION，避免重复。
            0
        }
        WM_IME_STARTCOMPOSITION => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(editing) = state.editing.as_mut() {
                        editing.composing.clear();
                    }
                    update_ime_position(hwnd, state);
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_IME_ENDCOMPOSITION => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(editing) = state.editing.as_mut() {
                        editing.composing.clear();
                    }
                    update_ime_position(hwnd, state);
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_IME_COMPOSITION => {
            let flags = lp as u32;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.editing.is_none() {
                        return;
                    }
                    if flags & 0x800 != 0 {
                        if let Some(result) = ime_string(hwnd, 0x800) {
                            state.caret_on = true;
                            SetTimer(hwnd, 1, 530, None);
                            if let Some(editing) = state.editing.as_mut() {
                                editing.text.push_str(&result);
                                // 上屏后必须清空组合串，否则会显示成“你好nihao”。
                                editing.composing.clear();
                            }
                        }
                    }
                    if flags & 0x8 != 0 {
                        let composing = ime_string(hwnd, 0x8).unwrap_or_default();
                        if let Some(editing) = state.editing.as_mut() {
                            editing.composing = composing;
                        }
                    }
                    update_ime_position(hwnd, state);
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_MOUSEWHEEL => {
            let screen_x = (lp as u16) as i16 as i32;
            let screen_y = (((lp as u64) >> 16) as u16) as i16 as i32;
            let delta = ((wp >> 16) as u16) as i16;
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.phase == Phase::Annotating && state.style_open {
                        if let Some(selection) = state.selection {
                            let x = screen_x - state.origin_x;
                            let y = screen_y - state.origin_y;
                            let layout = toolbar::layout(
                                selection,
                                (state.width, state.height),
                                state.doc.tool(),
                                state.style_open,
                                &state.picks,
                                state.line_menu_open,
                                state.arrow_menu_open,
                            );
                            if let Some(styles) = &layout.styles {
                                for (action, rect) in styles {
                                    if *action == Action::Width
                                        && x >= rect.left
                                        && x <= rect.right
                                        && y >= rect.top
                                        && y <= rect.bottom
                                    {
                                        state.doc.adjust_width(if delta > 0 { 1 } else { -1 });
                                    }
                                    if *action == Action::SerialSize
                                        && x >= rect.left && x <= rect.right
                                        && y >= rect.top && y <= rect.bottom
                                    {
                                        state.doc.adjust_serial_size(if delta > 0 { 1 } else { -1 });
                                    }
                                    if *action == Action::FontSize
                                        && x >= rect.left && x <= rect.right
                                        && y >= rect.top && y <= rect.bottom
                                    {
                                        state.doc.adjust_font_size(if delta > 0 { 1 } else { -1 });
                                    }
                                    if *action == Action::Opacity
                                        && x >= rect.left
                                        && x <= rect.right
                                        && y >= rect.top
                                        && y <= rect.bottom
                                    {
                                        state.doc.adjust_opacity(if delta > 0 { 1 } else { -1 });
                                    }
                                    if *action == Action::Strength
                                        && x >= rect.left
                                        && x <= rect.right
                                        && y >= rect.top
                                        && y <= rect.bottom
                                    {
                                        let mosaic = state.doc.tool() != Some(Shape::Blur);
                                        state.doc.adjust_strength(mosaic, if delta > 0 { 1 } else { -1 });
                                    }
                                }
                            }
                        }
                    }
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_TIMER => {
            let editing = STATE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .map(|state| state.editing.is_some())
                    .unwrap_or(false)
            });
            if editing {
                STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.caret_on = !state.caret_on;
                    }
                });
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_KEYUP => {
            // Shift 用抬起事件切换，避免按住时的自动重复来回翻转。
            // 系统可能上报 VK_SHIFT，也可能上报 VK_LSHIFT / VK_RSHIFT。
            let key = wp as u16;
            if (key == VK_SHIFT || key == VK_LSHIFT || key == VK_RSHIFT)
                && STATE.with(|cell| cell.borrow().as_ref().map(|s| s.phase) == Some(Phase::Selecting))
            {
                STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.rgb_mode = !state.rgb_mode;
                    }
                });
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_ACTIVATE => {
            let inactive = (wp & 0xFFFF) == 0;
            if inactive {
                STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.line_menu_open = false;
                        state.arrow_menu_open = false;
                    }
                });
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_OVERLAY_PIN => {
            // 贴图快捷键：覆盖层已经画出选区才有效，否则忽略、不打断当前操作。
            let ready = STATE.with(|cell| {
                let mut borrow = cell.borrow_mut();
                let Some(state) = borrow.as_mut() else {
                    return false;
                };
                if state.phase != Phase::Annotating || state.selection.is_none() {
                    return false;
                }
                state.result = state.selection;
                state.pin = true;
                true
            });
            if ready {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing = None;
                }
            });
            OVERLAY_HWND.store(0, Ordering::SeqCst);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}


/// 抓屏 → 建三张 DIB 段（原图/压暗/后备）→ 覆盖层窗口 → 选区。
pub fn select(width: i32, height: i32, options: OverlayOptions) -> Option<OverlayOutcome> {
    let class_name: Vec<u16> = "LightweightToolsetScreenshotOverlay\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "LightweightToolset Screenshot\0".encode_utf16().collect();

    let handle = std::thread::Builder::new()
        .name("screenshot-overlay".to_owned())
        .spawn(move || unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                // CS_DBLCLKS：折线靠双击收尾，必须有它才会收到 WM_LBUTTONDBLCLK。
                style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
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
            let mut dim_base = Surface::new(1, 1)?;
            dim_base.set_pixel(
                (options.dim_color & 0xFF) as u8,
                ((options.dim_color >> 8) & 0xFF) as u8,
                ((options.dim_color >> 16) & 0xFF) as u8,
            );
            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: options.dim_alpha,
                AlphaFormat: 0,
            };
            AlphaBlend(
                dimmed.dc(),
                0,
                0,
                width,
                height,
                dim_base.dc(),
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
            let pen = CreatePen(PS_SOLID, 3, rgb(0x30, 0x7d, 0xef));
            let grid_pen = CreatePen(PS_SOLID, 1, options.grid_color);
            // 自定义虚线段长（随线宽缩放），避免系统 PS_DASH 的粗段看起来像“色块+白条”。
            let guide_width = options.guides_width.clamp(1, 5);
            let guide_dash: [u32; 2] = [
                (guide_width as u32) * 4,
                (guide_width as u32) * 4,
            ];
            let guide_brush = LOGBRUSH {
                lbStyle: BS_SOLID,
                lbColor: options.guides_color,
                lbHatch: 0,
            };
            let guide_pen = ExtCreatePen(
                (PS_GEOMETRIC | PS_USERSTYLE | PS_ENDCAP_FLAT | PS_JOIN_MITER) as u32,
                guide_width as u32,
                &guide_brush,
                2,
                guide_dash.as_ptr(),
            );
            let cursor_guide_width = options.cursor_guide_width.clamp(1, 5);
            let cursor_guide_pen =
                CreatePen(PS_SOLID, cursor_guide_width, options.cursor_guide_color);
            let cross_pen = CreatePen(PS_SOLID, 2, rgb(0x30, 0x7d, 0xef));
            let white_pen = CreatePen(PS_SOLID, 1, rgb(0xff, 0xff, 0xff));
            let panel_brush = CreateSolidBrush(rgb(0x00, 0x00, 0x00));
            let label_brush = CreateSolidBrush(rgb(0x00, 0x00, 0x00));

            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN,
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
            let ime_context = ImmAssociateContext(hwnd, std::ptr::null_mut());
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
                    guide_pen,
                    cursor_guide_pen,
                    cross_pen,
                    white_pen,
                    panel_brush,
                    label_brush,
                    dragging: false,
                    anchor: None,
                    current: None,
                    cursor: None,
                    phase: Phase::Selecting,
                    selection: None,
                    editing: None,
                    caret_on: false,
                    adjust: None,
                    picks: toolbar::Picks::default(),
                    style_open: false,
                    line_menu_open: false,
                    arrow_menu_open: false,
                    delete_mode: false,
                    saved: false,
                    copy_only: false,
                    pin: false,
                    manual_dir: options.manual_dir,
                    quick_dir: options.quick_dir,
                    file_template: options.file_template,
                    grid: options.grid,
                    guides_mode: options.guides_mode,
                    guide_width,
                    cursor_guide_width,
                    guide_bands: Vec::new(),
                    alt_down: false,
                    ime_context: ime_context as isize,
                    doc: Document::new(),
                    draft_points: Vec::new(),
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
            let (top_down, bottom_up) =
                compose(&state.original, selection, &state.doc.items)?;
            Some(OverlayOutcome {
                selection,
                top_down,
                bottom_up,
                saved: state.saved,
                copy_only: state.copy_only,
                pin: state.pin,
            })
        })
        .ok()?;

    handle.join().ok().flatten()
}

/// 贴图快捷键：请覆盖层把当前选区钉出去；没有覆盖层时返回 false。
pub fn request_pin() -> bool {
    let hwnd = OVERLAY_HWND.load(Ordering::SeqCst);
    if hwnd == 0 {
        return false;
    }
    unsafe { PostMessageW(hwnd as HWND, WM_OVERLAY_PIN, 0, 0) != 0 }
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
    use super::{color_text, square_cursor, union_rect, Selection};

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
    fn circle_cursor_makes_square() {
        let sel = Selection {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        };
        // 横向拖得多：边长取 120，纵向也只走 120。
        let end = square_cursor(sel, (10, 10), (130, 60));
        assert_eq!(end, (130, 130));
        // 向左上拖：方向保持。
        let end = square_cursor(sel, (300, 300), (240, 250));
        assert_eq!(end, (240, 240));
        // 贴着选区边界时被夹住，仍是正方形。
        let end = square_cursor(sel, (480, 380), (600, 600));
        assert_eq!(end, (500, 400));
    }

    #[test]
    fn color_text_switches_format() {
        // 输入是 (B, G, R)。
        assert_eq!(color_text((0xAB, 0xCD, 0xEF), false), "#efcdab");
        assert_eq!(color_text((0x72, 0x63, 0x69), true), "RGB(105, 99, 114)");
    }
}
