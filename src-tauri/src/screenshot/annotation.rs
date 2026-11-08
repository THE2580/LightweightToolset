use windows_sys::Win32::Foundation::{POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;

use super::dib::Surface;

/// COLORREF 的 BGR 打包。
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

/// 调色板固定 8 格，可在工具箱设置里改色。
pub const PALETTE_LEN: usize = 8;

pub const DEFAULT_PALETTE: [u32; PALETTE_LEN] = [
    rgb(0xE5, 0x3E, 0x3E), // 红
    rgb(0xED, 0x89, 0x36), // 橙
    rgb(0xF2, 0xC9, 0x4C), // 黄
    rgb(0x2E, 0xC4, 0x6B), // 绿
    rgb(0x38, 0xB2, 0xAC), // 青
    rgb(0x30, 0x7D, 0xEF), // 蓝
    rgb(0x80, 0x5A, 0xD5), // 紫
    rgb(0x1A, 0x20, 0x2C), // 黑
];

/// 当前覆盖层使用的调色板；每次截图前由设置覆盖。
static ACTIVE_PALETTE: std::sync::Mutex<[u32; PALETTE_LEN]> = std::sync::Mutex::new(DEFAULT_PALETTE);

pub fn palette() -> [u32; PALETTE_LEN] {
    ACTIVE_PALETTE.lock().map(|guard| *guard).unwrap_or(DEFAULT_PALETTE)
}

pub fn set_palette(colors: [u32; PALETTE_LEN]) {
    if let Ok(mut guard) = ACTIVE_PALETTE.lock() {
        *guard = colors;
    }
}

/// 解析 `#RRGGBB` / `RRGGBB`，失败返回 `None`。
pub fn parse_hex_color(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(rgb(r, g, b))
}

/// 反方向：`u32` → `#RRGGBB`（大写）。
pub fn hex_color(color: u32) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        color & 0xFF,
        (color >> 8) & 0xFF,
        (color >> 16) & 0xFF
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// 到矩形边框的距离：外部取切比雪夫距离，内部取到最近边的距离。
    pub fn border_distance(&self, x: i32, y: i32) -> i32 {
        let right = self.x + self.width;
        let bottom = self.y + self.height;
        let out_x = (self.x - x).max(x - right).max(0);
        let out_y = (self.y - y).max(y - bottom).max(0);
        if out_x > 0 || out_y > 0 {
            return out_x.max(out_y);
        }
        (x - self.x)
            .min(right - x)
            .min(y - self.y)
            .min(bottom - y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Ellipse,
    /// 正圆：包围盒被强制为正方形，圆内切其中。
    Circle,
    Line,
    Arrow,
    Polyline,
    Pencil,
    Highlighter,
    Mosaic,
    Blur,
    Text,
    Serial,
}

impl Shape {
    pub fn label(self) -> &'static str {
        match self {
            Shape::Rect => "矩形",
            Shape::Ellipse => "椭圆",
            Shape::Circle => "正圆",
            Shape::Line => "直线",
            Shape::Arrow => "箭头",
            Shape::Polyline => "折线",
            Shape::Pencil => "铅笔",
            Shape::Highlighter => "荧光笔",
            Shape::Mosaic => "马赛克",
            Shape::Blur => "模糊",
            Shape::Text => "文字",
            Shape::Serial => "序号",
        }
    }

    pub fn is_path_shape(self) -> bool {
        matches!(self, Shape::Polyline | Shape::Pencil | Shape::Highlighter)
    }

}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub shape: Shape,
    pub points: Vec<(i32, i32)>,
    pub color: u32,
    pub width: i32,
    pub text: String,
    pub fill: bool,
    pub arrow: u8,
    pub closed: bool,
    pub line_style: u8,
    pub strength: i32,
    pub opacity: u8,
}

impl Item {
    pub fn new(shape: Shape, points: Vec<(i32, i32)>, color: u32, width: i32) -> Self {
        Self {
            shape,
            points,
            color,
            width,
            text: String::new(),
            fill: false,
            arrow: 0,
            closed: false,
            line_style: 0,
            strength: 14,
            opacity: 45,
        }
    }

    pub fn bounds(&self) -> Option<Rect> {
        if self.shape == Shape::Text {
            let origin = *self.points.first()?;
            let (width, height) = text_extent(&self.text, self.width.max(8));
            return Some(Rect {
                x: origin.0,
                y: origin.1,
                width,
                height,
            });
        }
        if self.shape == Shape::Serial {
            let origin = *self.points.first()?;
            let radius = serial_radius(self.width);
            return Some(Rect {
                x: origin.0 - radius,
                y: origin.1 - radius,
                width: radius * 2,
                height: radius * 2,
            });
        }
        let first = *self.points.first()?;
        let mut left = first.0;
        let mut top = first.1;
        let mut right = first.0;
        let mut bottom = first.1;
        for point in &self.points[1..] {
            left = left.min(point.0);
            top = top.min(point.1);
            right = right.max(point.0);
            bottom = bottom.max(point.1);
        }
        Some(Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// 点到线段的距离。
pub fn distance_to_segment(p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> f64 {
    let (px, py) = (p.0 as f64, p.1 as f64);
    let (ax, ay) = (a.0 as f64, a.1 as f64);
    let (bx, by) = (b.0 as f64, b.1 as f64);
    let dx = bx - ax;
    let dy = by - ay;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f64::EPSILON {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / length_squared).clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Ramer–Douglas–Peucker 简化手绘点列。
pub fn simplify(points: &[(i32, i32)], epsilon: f64) -> Vec<(i32, i32)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        if end <= start + 1 {
            continue;
        }
        let mut farthest = 0.0;
        let mut index = start;
        for i in start + 1..end {
            let distance = distance_to_segment(points[i], points[start], points[end]);
            if distance > farthest {
                farthest = distance;
                index = i;
            }
        }
        if farthest > epsilon {
            keep[index] = true;
            stack.push((start, index));
            stack.push((index, end));
        }
    }
    points
        .iter()
        .enumerate()
        .filter(|(i, _)| keep[*i])
        .map(|(_, p)| *p)
        .collect()
}

/// 标注文档：item 列表 + 选中项 + 当前工具 + 撤销/重做栈。
pub struct Document {
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    tool: Option<Shape>,
    color_index: usize,
    width: i32,
    fill: bool,
    arrow_style: u8,
    closed: bool,
    line_style_shapes: u8,
    line_style_lines: u8,
    mosaic_strength: i32,
    blur_strength: i32,
    opacity: u8,
    font_size: i32,
    serial_size: i32,
    undo: Vec<Vec<Item>>,
    redo: Vec<Vec<Item>>,
    serial_next: u32,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            tool: None,
            color_index: 0,
            width: 4,
            fill: false,
            arrow_style: 1,
            closed: false,
            line_style_shapes: 0,
            line_style_lines: 0,
            mosaic_strength: 14,
            blur_strength: 6,
            opacity: 45,
            font_size: TEXT_HEIGHT,
            serial_size: 14,
            undo: Vec::new(),
            redo: Vec::new(),
            serial_next: 1,
        }
    }

    pub fn next_serial(&mut self) -> u32 {
        let value = self.serial_next;
        self.serial_next += 1;
        value
    }

    pub fn tool(&self) -> Option<Shape> {
        self.tool
    }

    pub fn set_tool(&mut self, tool: Shape) {
        self.tool = Some(tool);
    }

    /// 工具栏再次点击同一工具时取消选中。
    pub fn toggle_tool(&mut self, tool: Shape) {
        self.tool = if self.tool == Some(tool) {
            None
        } else {
            Some(tool)
        };
    }

    pub fn color(&self) -> u32 {
        let colors = palette();
        colors[self.color_index.min(PALETTE_LEN - 1)]
    }

    pub fn color_index(&self) -> usize {
        self.color_index
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn set_width(&mut self, width: i32) {
        self.width = width.clamp(1, 50);
    }

    pub fn adjust_width(&mut self, delta: i32) {
        self.set_width(self.width + delta);
    }

    pub fn fill(&self) -> bool {
        self.fill
    }

    pub fn toggle_fill(&mut self) {
        self.fill = !self.fill;
    }

    pub fn line_style(&self, shapes: bool) -> u8 {
        if shapes {
            self.line_style_shapes
        } else {
            self.line_style_lines
        }
    }

    pub fn set_line_style(&mut self, shapes: bool, style: u8) {
        if shapes {
            self.line_style_shapes = style.min(1);
        } else {
            self.line_style_lines = style.min(1);
        }
    }

    pub fn font_size(&self) -> i32 {
        self.font_size
    }

    pub fn serial_size(&self) -> i32 {
        self.serial_size
    }

    pub fn adjust_serial_size(&mut self, delta: i32) {
        self.serial_size = (self.serial_size + delta).clamp(6, 40);
    }

    pub fn adjust_font_size(&mut self, delta: i32) {
        self.font_size = (self.font_size + delta).clamp(8, 96);
    }

    pub fn opacity(&self) -> u8 {
        self.opacity
    }

    pub fn adjust_opacity(&mut self, delta: i32) {
        self.opacity = ((self.opacity as i32 + delta).clamp(10, 50)) as u8;
    }

    pub fn strength(&self, mosaic: bool) -> i32 {
        if mosaic { self.mosaic_strength } else { self.blur_strength }
    }

    pub fn adjust_strength(&mut self, mosaic: bool, delta: i32) {
        let (min, max) = if mosaic { (4, 32) } else { (1, 20) };
        let value = (self.strength(mosaic) + delta).clamp(min, max);
        if mosaic { self.mosaic_strength = value } else { self.blur_strength = value }
    }

    pub fn clear_tool(&mut self) {
        self.tool = None;
    }

    pub fn arrow_style(&self) -> u8 {
        self.arrow_style
    }

    pub fn set_arrow_style(&mut self, style: u8) {
        self.arrow_style = style.min(2);
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn toggle_closed(&mut self) {
        self.closed = !self.closed;
    }

    pub fn set_color(&mut self, index: usize) {
        self.color_index = index.min(PALETTE_LEN - 1);
    }

    fn begin_change(&mut self) {
        self.undo.push(self.items.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn push(&mut self, mut item: Item) {
        if matches!(item.shape, Shape::Rect | Shape::Ellipse | Shape::Circle) {
            item.fill = self.fill;
        }
        if item.shape == Shape::Arrow {
            item.arrow = self.arrow_style.clamp(1, 2);
        } else if item.shape == Shape::Line {
            item.arrow = 0;
        }
        if item.shape == Shape::Polyline {
            item.closed = self.closed;
        }
        if item.shape == Shape::Text {
            item.width = self.font_size;
        }
        if item.shape == Shape::Serial {
            item.width = self.serial_size;
        }
        if item.shape == Shape::Highlighter {
            item.opacity = self.opacity;
        }
        if item.shape == Shape::Mosaic {
            item.strength = self.mosaic_strength;
        } else if item.shape == Shape::Blur {
            item.strength = self.blur_strength;
        }
        item.line_style = match item.shape {
            Shape::Rect | Shape::Ellipse | Shape::Circle => self.line_style_shapes,
            Shape::Line | Shape::Arrow | Shape::Polyline => self.line_style_lines,
            _ => 0,
        };
        self.begin_change();
        self.items.push(item);
        self.selected = Some(self.items.len() - 1);
    }

    pub fn delete_selected(&mut self) -> bool {
        let Some(index) = self.selected else {
            return false;
        };
        self.delete_at(index)
    }

    pub fn delete_at(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.begin_change();
        self.items.remove(index);
        self.selected = None;
        true
    }

    /// 命中测试：返回距离最近且在容差内的 item。
    pub fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (index, item) in self.items.iter().enumerate() {
            let tolerance = (item.width as f64 / 2.0 + 4.0).max(6.0);
            let distance = match item.shape {
                Shape::Rect | Shape::Ellipse | Shape::Circle | Shape::Mosaic | Shape::Blur => item
                    .bounds()
                    .map(|bounds| bounds.border_distance(x, y) as f64)
                    .unwrap_or(f64::MAX),
                Shape::Text => {
                    let (width, height) = text_extent(&item.text, item.width.max(8));
                    let origin = item.points.first().copied().unwrap_or((0, 0));
                    let dx = (origin.0 - x).max(x - (origin.0 + width)).max(0);
                    let dy = (origin.1 - y).max(y - (origin.1 + height)).max(0);
                    (dx * dx + dy * dy) as f64
                }
                Shape::Serial => {
                    let origin = item.points.first().copied().unwrap_or((0, 0));
                    let radius = serial_radius(item.width) + 4;
                    let dx = (origin.0 - x) as f64;
                    let dy = (origin.1 - y) as f64;
                    if (dx * dx + dy * dy).sqrt() <= radius as f64 {
                        0.0
                    } else {
                        (dx * dx + dy * dy).sqrt()
                    }
                }
                _ => {
                    let mut min = f64::MAX;
                    for pair in item.points.windows(2) {
                        min = min.min(distance_to_segment((x, y), pair[0], pair[1]));
                    }
                    min
                }
            };
            if distance <= tolerance && best.map(|(_, d)| distance < d).unwrap_or(true) {
                best = Some((index, distance));
            }
        }
        best.map(|(index, _)| index)
    }

    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index.filter(|i| *i < self.items.len());
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.items.clone());
        self.items = previous;
        self.selected = self.items.len().checked_sub(1);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.items.clone());
        self.items = next;
        self.selected = self.items.len().checked_sub(1);
        true
    }
}

unsafe fn item_pen(item: &Item) -> HPEN {
    if item.line_style == 1 {
        let brush = LOGBRUSH {
            lbStyle: BS_SOLID,
            lbColor: item.color,
            lbHatch: 0,
        };
        let width = item.width.max(1);
        let dash = (width as u32 * 2).max(6);
        let gap = (width as u32 * 3 / 2).max(4);
        let pattern: [u32; 2] = [dash, gap];
        ExtCreatePen(
            (PS_GEOMETRIC | PS_USERSTYLE | PS_ENDCAP_FLAT | PS_JOIN_MITER) as u32,
            width as u32,
            &brush,
            2,
            pattern.as_ptr(),
        )
    } else {
        CreatePen(PS_SOLID, item.width.max(1), item.color)
    }
}

unsafe fn draw_arrow_head(hdc: HDC, item: &Item, from: (i32, i32), tip: (i32, i32), ox: i32, oy: i32) {
    let dx = (tip.0 - from.0) as f64;
    let dy = (tip.1 - from.1) as f64;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return;
    }
    let ux = dx / length;
    let uy = dy / length;
    let head = item.width as f64 * 3.0 + 8.0;
    let base_x = tip.0 as f64 - ux * head;
    let base_y = tip.1 as f64 - uy * head;
    let half = (head * 0.55).max(item.width as f64 * 0.9 + 4.0);
    let points = [
        POINT {
            x: tip.0 + ox,
            y: tip.1 + oy,
        },
        POINT {
            x: (base_x - uy * half) as i32 + ox,
            y: (base_y + ux * half) as i32 + oy,
        },
        POINT {
            x: (base_x + uy * half) as i32 + ox,
            y: (base_y - ux * half) as i32 + oy,
        },
    ];
    let brush = CreateSolidBrush(item.color);
    let old_brush = SelectObject(hdc, brush);
    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    Polygon(hdc, points.as_ptr(), points.len() as i32);
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    DeleteObject(brush);
}

/// 文字标注的字号（像素高度）。
pub const TEXT_HEIGHT: i32 = 22;

fn text_font(size: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            500,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            (DEFAULT_PITCH | FF_DONTCARE) as u32,
            windows_sys::w!("Microsoft YaHei") as *const u16,
        )
    }
}

/// 文字标注的像素尺寸（用于命中测试与选中框）。
pub fn text_extent(text: &str, size: i32) -> (i32, i32) {
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let font = text_font(size.max(8));
        let old = SelectObject(dc, font);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size: SIZE = std::mem::zeroed();
        GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut size);
        SelectObject(dc, old);
        DeleteObject(font);
        ReleaseDC(std::ptr::null_mut(), dc);
        (size.cx.max(1), size.cy.max(TEXT_HEIGHT))
    }
}

/// 序号圆圈的半径。
pub fn serial_radius(width: i32) -> i32 {
    width.max(6)
}

/// 在 `hdc` 上绘制标注；`ox/oy` 是图像坐标系的屏幕原点偏移。
/// 计算 item 在 surface 像素坐标系里的矩形（裁剪到画布内）。
fn region(surface: &Surface, item: &Item, ox: i32, oy: i32) -> Option<(usize, usize, usize, usize)> {
    let bounds = item.bounds()?;
    let x0 = (bounds.x + ox).max(0) as usize;
    let y0 = (bounds.y + oy).max(0) as usize;
    let x1 = ((bounds.x + bounds.width + ox).min(surface.width)) as usize;
    let y1 = ((bounds.y + bounds.height + oy).min(surface.height)) as usize;
    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some((x0, y0, x1 - x0, y1 - y0))
    }
}

/// 马赛克：按线宽决定块大小，块内取平均色后整块填充。
unsafe fn apply_mosaic(surface: &mut Surface, item: &Item, ox: i32, oy: i32) {
    let Some((x0, y0, width, height)) = region(surface, item, ox, oy) else {
        return;
    };
    let block = item.strength.clamp(4, 32) as usize;
    let stride = surface.width as usize * 4;
    let pixels = surface.pixels_mut();
    let mut y = 0;
    while y < height {
        let bh = block.min(height - y);
        let mut x = 0;
        while x < width {
            let bw = block.min(width - x);
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for yy in 0..bh {
                for xx in 0..bw {
                    let index = (y0 + y + yy) * stride + (x0 + x + xx) * 4;
                    sum[0] += pixels[index] as u32;
                    sum[1] += pixels[index + 1] as u32;
                    sum[2] += pixels[index + 2] as u32;
                    count += 1;
                }
            }
            let avg = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ];
            for yy in 0..bh {
                for xx in 0..bw {
                    let index = (y0 + y + yy) * stride + (x0 + x + xx) * 4;
                    pixels[index] = avg[0];
                    pixels[index + 1] = avg[1];
                    pixels[index + 2] = avg[2];
                }
            }
            x += bw;
        }
        y += bh;
    }
}

/// 模糊：滑窗盒式模糊（可分离，O(w*h)），半径随线宽。
unsafe fn apply_blur(surface: &mut Surface, item: &Item, ox: i32, oy: i32) {
    let Some((x0, y0, width, height)) = region(surface, item, ox, oy) else {
        return;
    };
    let radius = item.strength.clamp(1, 20) as isize;
    let stride = surface.width as usize * 4;

    // 先把区域拷出来，避免读写互相污染。
    let mut source = vec![0u8; width * height * 4];
    {
        let pixels = surface.pixels();
        for row in 0..height {
            let src = (y0 + row) * stride + x0 * 4;
            source[row * width * 4..(row + 1) * width * 4]
                .copy_from_slice(&pixels[src..src + width * 4]);
        }
    }

    // 水平方向
    let mut horizontal = vec![0u8; width * height * 4];
    for row in 0..height {
        let base = row * width * 4;
        let mut sum = [0u32; 3];
        let mut count = 0u32;
        let last = (radius.min(width as isize - 1)).max(0) as usize;
        for i in 0..=last {
            let index = base + i * 4;
            sum[0] += source[index] as u32;
            sum[1] += source[index + 1] as u32;
            sum[2] += source[index + 2] as u32;
            count += 1;
        }
        for x in 0..width {
            let index = base + x * 4;
            horizontal[index] = (sum[0] / count) as u8;
            horizontal[index + 1] = (sum[1] / count) as u8;
            horizontal[index + 2] = (sum[2] / count) as u8;
            let remove = x as isize - radius;
            if remove >= 0 {
                let i = base + remove as usize * 4;
                sum[0] -= source[i] as u32;
                sum[1] -= source[i + 1] as u32;
                sum[2] -= source[i + 2] as u32;
                count -= 1;
            }
            let add = x as isize + radius + 1;
            if add < width as isize {
                let i = base + add as usize * 4;
                sum[0] += source[i] as u32;
                sum[1] += source[i + 1] as u32;
                sum[2] += source[i + 2] as u32;
                count += 1;
            }
        }
    }

    // 垂直方向，直接写回 surface
    let pixels = surface.pixels_mut();
    for x in 0..width {
        let mut sum = [0u32; 3];
        let mut count = 0u32;
        let last = (radius.min(height as isize - 1)).max(0) as usize;
        for y in 0..=last {
            let index = y * width * 4 + x * 4;
            sum[0] += horizontal[index] as u32;
            sum[1] += horizontal[index + 1] as u32;
            sum[2] += horizontal[index + 2] as u32;
            count += 1;
        }
        for y in 0..height {
            let index = (y0 + y) * stride + (x0 + x) * 4;
            pixels[index] = (sum[0] / count) as u8;
            pixels[index + 1] = (sum[1] / count) as u8;
            pixels[index + 2] = (sum[2] / count) as u8;
            let remove = y as isize - radius;
            if remove >= 0 {
                let i = remove as usize * width * 4 + x * 4;
                sum[0] -= horizontal[i] as u32;
                sum[1] -= horizontal[i + 1] as u32;
                sum[2] -= horizontal[i + 2] as u32;
                count -= 1;
            }
            let add = y as isize + radius + 1;
            if add < height as isize {
                let i = add as usize * width * 4 + x * 4;
                sum[0] += horizontal[i] as u32;
                sum[1] += horizontal[i + 1] as u32;
                sum[2] += horizontal[i + 2] as u32;
                count += 1;
            }
        }
    }
}

/// 荧光笔：先在副本上画实心笔迹，再整体常量 alpha 叠加，避免背景被黑色污染。
unsafe fn draw_highlighter(hdc: HDC, item: &Item, ox: i32, oy: i32) {
    let Some(bounds) = item.bounds() else {
        return;
    };
    let pad = item.width * 3 / 2 + 2;
    let width = bounds.width + pad * 2;
    let height = bounds.height + pad * 2;
    if width <= 0 || height <= 0 {
        return;
    }
    let Some(temp) = Surface::new(width, height) else {
        return;
    };
    let bx = bounds.x + ox - pad;
    let by = bounds.y + oy - pad;
    BitBlt(temp.dc(), 0, 0, width, height, hdc, bx, by, SRCCOPY);

    let pen = CreatePen(PS_SOLID, (item.width * 3).max(4), item.color);
    let old_pen = SelectObject(temp.dc(), pen);
    let old_brush = SelectObject(temp.dc(), GetStockObject(NULL_BRUSH));
    if item.points.len() >= 2 {
        let points: Vec<POINT> = item
            .points
            .iter()
            .map(|p| POINT {
                x: p.0 + ox - bx,
                y: p.1 + oy - by,
            })
            .collect();
        Polyline(temp.dc(), points.as_ptr(), points.len() as i32);
    }
    SelectObject(temp.dc(), old_brush);
    SelectObject(temp.dc(), old_pen);
    DeleteObject(pen);

    let blend = BLENDFUNCTION {
        BlendOp: 0,
        BlendFlags: 0,
        SourceConstantAlpha: (item.opacity as u32 * 255 / 100).clamp(1, 255) as u8,
        AlphaFormat: 0,
    };
    AlphaBlend(hdc, bx, by, width, height, temp.dc(), 0, 0, width, height, blend);
}

pub unsafe fn draw(
    surface: &mut Surface,
    items: &[Item],
    selected: Option<usize>,
    ox: i32,
    oy: i32,
    draft: Option<&Item>,
) {
    // 先铺像素效果（直接改像素），再叠加其余标注。
    for item in items {
        match item.shape {
            Shape::Mosaic => apply_mosaic(surface, item, ox, oy),
            Shape::Blur => apply_blur(surface, item, ox, oy),
            _ => {}
        }
    }
    if let Some(draft) = draft {
        match draft.shape {
            Shape::Mosaic => apply_mosaic(surface, draft, ox, oy),
            Shape::Blur => apply_blur(surface, draft, ox, oy),
            _ => {}
        }
    }

    let hdc = surface.dc();
    let paint_one = |item: &Item, highlighted: bool| {
        let pen = item_pen(item);
        let old_pen = SelectObject(hdc, pen);
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));

        match item.shape {
            Shape::Mosaic | Shape::Blur => {
                // 已在效果阶段处理，这里只画选中框。
            }
            Shape::Highlighter => {
                draw_highlighter(hdc, item, ox, oy);
            }
            Shape::Rect | Shape::Ellipse | Shape::Circle => {
                if let Some(bounds) = item.bounds() {
                    let left = bounds.x + ox;
                    let top = bounds.y + oy;
                    let right = left + bounds.width;
                    let bottom = top + bounds.height;
                    let brush = if item.fill {
                        CreateSolidBrush(item.color)
                    } else {
                        GetStockObject(NULL_BRUSH) as HBRUSH
                    };
                    let old_brush = SelectObject(hdc, brush);
                    if item.shape == Shape::Rect {
                        Rectangle(hdc, left, top, right, bottom);
                    } else {
                        Ellipse(hdc, left, top, right, bottom);
                    }
                    SelectObject(hdc, old_brush);
                    if item.fill {
                        DeleteObject(brush);
                    }
                }
            }
            Shape::Line | Shape::Arrow => {
                if item.points.len() >= 2 {
                    let a = item.points[0];
                    let b = item.points[1];
                    let dx = (b.0 - a.0) as f64;
                    let dy = (b.1 - a.1) as f64;
                    let length = (dx * dx + dy * dy).sqrt().max(0.001);
                    let ux = dx / length;
                    let uy = dy / length;
                    let head = item.width as f64 * 3.0 + 8.0;
                    let shrink = if item.arrow >= 1 { (head * 0.9) as i32 } else { 0 };
                    let end = if item.arrow >= 1 {
                        (b.0 - (ux * shrink as f64) as i32, b.1 - (uy * shrink as f64) as i32)
                    } else {
                        b
                    };
                    let start = if item.arrow == 2 {
                        (a.0 + (ux * shrink as f64) as i32, a.1 + (uy * shrink as f64) as i32)
                    } else {
                        a
                    };
                    MoveToEx(hdc, start.0 + ox, start.1 + oy, std::ptr::null_mut());
                    LineTo(hdc, end.0 + ox, end.1 + oy);
                    match item.arrow {
                        0 => {}
                        1 => draw_arrow_head(hdc, item, a, b, ox, oy),
                        _ => {
                            draw_arrow_head(hdc, item, a, b, ox, oy);
                            draw_arrow_head(hdc, item, b, a, ox, oy);
                        }
                    }
                }
            }
            Shape::Text => {
                let font = text_font(item.width.max(8));
                let old_font = SelectObject(hdc, font);
                SetBkMode(hdc, TRANSPARENT as i32);
                let wide: Vec<u16> = item.text.encode_utf16().collect();
                if let Some(p) = item.points.first() {
                    let (x, y) = (p.0 + ox, p.1 + oy);
                    // 1px 白色描边：四个方向先铺白，再用本色覆盖中心。
                    SetTextColor(hdc, rgb(0xff, 0xff, 0xff));
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        TextOutW(hdc, x + dx, y + dy, wide.as_ptr(), wide.len() as i32);
                    }
                    SetTextColor(hdc, item.color);
                    TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32);
                }
                SelectObject(hdc, old_font);
                DeleteObject(font);
            }
            Shape::Serial => {
                let (cx, cy) = item.points.first().copied().unwrap_or((0, 0));
                let radius = serial_radius(item.width);
                let brush = CreateSolidBrush(item.color);
                let old_brush = SelectObject(hdc, brush);
                let old_pen = SelectObject(hdc, pen);
                Ellipse(
                    hdc,
                    cx + ox - radius,
                    cy + oy - radius,
                    cx + ox + radius,
                    cy + oy + radius,
                );
                SelectObject(hdc, old_pen);
                SelectObject(hdc, old_brush);
                DeleteObject(brush);

                let font = text_font(item.width.max(8));
                let old_font = SelectObject(hdc, font);
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, rgb(0xff, 0xff, 0xff));
                let wide: Vec<u16> = item.text.encode_utf16().collect();
                let mut size: SIZE = std::mem::zeroed();
                GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut size);
                TextOutW(
                    hdc,
                    cx + ox - size.cx / 2,
                    cy + oy - size.cy / 2,
                    wide.as_ptr(),
                    wide.len() as i32,
                );
                SelectObject(hdc, old_font);
                DeleteObject(font);
            }
            Shape::Polyline | Shape::Pencil => {
                if item.points.len() >= 2 {
                    let mut closed_points: Vec<POINT> = Vec::new();
                    if item.shape == Shape::Polyline && item.closed {
                        closed_points.extend(item.points.iter().map(|p| POINT {
                            x: p.0 + ox,
                            y: p.1 + oy,
                        }));
                        if let Some(first) = closed_points.first().copied() {
                            closed_points.push(first);
                        }
                    }
                    let points: Vec<POINT> = if item.shape == Shape::Polyline && item.closed {
                        closed_points
                    } else {
                        item.points
                            .iter()
                            .map(|p| POINT {
                                x: p.0 + ox,
                                y: p.1 + oy,
                            })
                            .collect()
                    };
                    Polyline(hdc, points.as_ptr(), points.len() as i32);
                } else if let Some(p) = item.points.first() {
                    SetPixelV(hdc, p.0 + ox, p.1 + oy, item.color);
                }
            }
        }

        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(pen);

        if highlighted {
            if let Some(bounds) = item.bounds() {
                let highlight = CreatePen(PS_DOT, 1, rgb(0xff, 0xff, 0xff));
                let old_pen = SelectObject(hdc, highlight);
                let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                Rectangle(
                    hdc,
                    bounds.x + ox - 3,
                    bounds.y + oy - 3,
                    bounds.x + ox + bounds.width + 3,
                    bounds.y + oy + bounds.height + 3,
                );
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                DeleteObject(highlight);
            }
        }
    };

    for (index, item) in items.iter().enumerate() {
        paint_one(item, selected == Some(index));
    }
    if let Some(draft) = draft {
        paint_one(draft, false);
    }
}

/// 当前工具下的草稿 item（坐标换算到图像坐标系）。
pub fn build_draft(
    tool: Shape,
    points: &[(i32, i32)],
    cursor: Option<(i32, i32)>,
    origin: (i32, i32),
    color: u32,
    width: i32,
    fill: bool,
    line_style: u8,
    arrow: u8,
    strength: i32,
    opacity: u8,
) -> Option<Item> {
    let first = *points.first()?;
    let to_image = |p: (i32, i32)| (p.0 - origin.0, p.1 - origin.1);
    let collected: Vec<(i32, i32)> = match tool {
        Shape::Pencil | Shape::Highlighter => points.iter().map(|p| to_image(*p)).collect(),
        Shape::Polyline => {
            let mut out: Vec<(i32, i32)> = points.iter().map(|p| to_image(*p)).collect();
            if let Some(cursor) = cursor {
                out.push(to_image(cursor));
            }
            out
        }
        _ => {
            let end = cursor.unwrap_or(first);
            vec![to_image(first), to_image(end)]
        }
    };
    if collected.len() < 2 {
        return None;
    }
    let mut item = Item::new(tool, collected, color, width);
    item.fill = fill;
    item.line_style = line_style;
    item.arrow = arrow;
    item.strength = strength;
    item.opacity = opacity;
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::{
        build_draft, distance_to_segment, hex_color, parse_hex_color, rgb, simplify, Document,
        Item, Shape,
    };

    #[test]
    fn hex_color_round_trips() {
        assert_eq!(parse_hex_color("#E53E3E"), Some(rgb(0xE5, 0x3E, 0x3E)));
        assert_eq!(parse_hex_color("30b2ac"), Some(rgb(0x30, 0xB2, 0xAC)));
        assert_eq!(hex_color(rgb(0x1A, 0x20, 0x2C)), "#1A202C");
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#GGGGGG"), None);
    }

    fn rect_item() -> Item {
        Item::new(Shape::Rect, vec![(10, 10), (110, 60)], 0, 4)
    }

    #[test]
    fn segment_distance_works() {
        assert!((distance_to_segment((5, 5), (0, 0), (10, 0)) - 5.0).abs() < 0.01);
        assert!((distance_to_segment((20, 0), (0, 0), (10, 0)) - 10.0).abs() < 0.01);
        assert!((distance_to_segment((5, 0), (0, 0), (10, 0)) - 0.0).abs() < 0.01);
    }

    #[test]
    fn simplify_keeps_endpoints() {
        let points = vec![(0, 0), (1, 1), (2, 0), (3, 1), (4, 0)];
        let out = simplify(&points, 0.5);
        assert_eq!(out.first(), Some(&(0, 0)));
        assert_eq!(out.last(), Some(&(4, 0)));
    }

    #[test]
    fn hit_test_handles_line_and_rect() {
        let mut doc = Document::new();
        doc.push(rect_item());
        doc.push(Item::new(Shape::Line, vec![(200, 200), (300, 300)], 0, 4));
        assert_eq!(doc.hit_test(10, 30), Some(0));
        assert_eq!(doc.hit_test(250, 251), Some(1));
        assert_eq!(doc.hit_test(60, 35), None);
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut doc = Document::new();
        doc.push(rect_item());
        assert!(doc.undo());
        assert!(doc.items.is_empty());
        assert!(doc.redo());
        assert_eq!(doc.items.len(), 1);
    }

    #[test]
    fn draft_uses_image_coordinates() {
        let draft = build_draft(
            Shape::Rect,
            &[(100, 100)],
            Some((150, 170)),
            (100, 100),
            0,
            4,
            false,
            0,
            0,
            0,
            45,
        )
        .expect("draft");
        assert_eq!(draft.points, vec![(0, 0), (50, 70)]);

        let polyline = build_draft(
            Shape::Polyline,
            &[(10, 10), (20, 20)],
            Some((30, 40)),
            (10, 10),
            0,
            4,
            false,
            0,
            0,
            0,
            45,
        )
        .expect("polyline");
        assert_eq!(polyline.points, vec![(0, 0), (10, 10), (20, 30)]);
    }

    #[test]
    fn serial_numbers_increment() {
        let mut doc = Document::new();
        assert_eq!(doc.next_serial(), 1);
        assert_eq!(doc.next_serial(), 2);
        assert_eq!(doc.next_serial(), 3);
    }

    #[test]
    fn tool_switch_is_kept() {
        let mut doc = Document::new();
        assert_eq!(doc.tool(), None);
        doc.set_tool(Shape::Arrow);
        assert_eq!(doc.tool(), Some(Shape::Arrow));
        doc.toggle_tool(Shape::Arrow);
        assert_eq!(doc.tool(), None);
        doc.toggle_tool(Shape::Ellipse);
        assert_eq!(doc.tool(), Some(Shape::Ellipse));
    }
}
