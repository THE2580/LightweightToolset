use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;

use super::annotation::{rgb, Shape, PALETTE};
use super::overlay::Selection;

pub const TB_BTN: i32 = 49;
const GAP: i32 = 6;
const PAD: i32 = 0;
const DIVIDER: i32 = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Shapes,
    Lines,
    Pixels,
}

#[derive(Clone, Copy)]
pub struct Picks {
    pub shapes: Shape,
    pub lines: Shape,
    pub pixels: Shape,
}

impl Default for Picks {
    fn default() -> Self {
        Self {
            shapes: Shape::Rect,
            lines: Shape::Line,
            pixels: Shape::Mosaic,
        }
    }
}

pub fn group_of(shape: Shape) -> Option<Group> {
    match shape {
        Shape::Rect | Shape::Ellipse | Shape::Circle => Some(Group::Shapes),
        Shape::Line | Shape::Arrow | Shape::Polyline => Some(Group::Lines),
        Shape::Mosaic | Shape::Blur => Some(Group::Pixels),
        _ => None,
    }
}

impl Picks {
    pub fn current(&self, group: Group) -> Shape {
        match group {
            Group::Shapes => self.shapes,
            Group::Lines => self.lines,
            Group::Pixels => self.pixels,
        }
    }

    pub fn set(&mut self, group: Group, shape: Shape) {
        match group {
            Group::Shapes => self.shapes = shape,
            Group::Lines => self.lines = shape,
            Group::Pixels => self.pixels = shape,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Group(Group),
    Pick(Group, Shape),
    Tool(Shape),
    Color(usize),
    Width,
    Fill,
    LineStyle,
    ArrowType,
    LineStylePick(u8),
    ArrowPick(u8),
    Strength,
    PixelMode(Shape),
    Undo,
    Redo,
    Delete,
    Save,
    Copy,
    Cancel,
}

pub struct Layout {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub buttons: Vec<(Action, RECT)>,
    pub styles: Option<Vec<(Action, RECT)>>,
    pub style_bg: Option<(i32, i32, i32, i32)>,
    pub separators: Vec<(i32, i32, i32)>,
    pub style_separators: Vec<(i32, i32, i32)>,
    pub menus: Vec<(Action, RECT)>,
}

impl Layout {
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let mut left = self.x;
        let mut top = self.y;
        let mut right = self.x + self.width;
        let mut bottom = self.y + self.height;
        if let Some((px, py, pw, ph)) = self.style_bg {
            left = left.min(px - 2);
            top = top.min(py - 2);
            right = right.max(px + pw + 2);
            bottom = bottom.max(py + ph + 2);
        }
        for (_, rect) in &self.menus {
            left = left.min(rect.left);
            top = top.min(rect.top);
            right = right.max(rect.right);
            bottom = bottom.max(rect.bottom);
        }
        for group in [&self.styles] {
            if let Some(buttons) = group {
                for (_, rect) in buttons {
                    left = left.min(rect.left);
                    top = top.min(rect.top);
                    right = right.max(rect.right);
                    bottom = bottom.max(rect.bottom);
                }
            }
        }
        (left - 2, top - 2, right - left + 4, bottom - top + 4)
    }
}

/// 主栏 + 二级样式栏 + 分组展开面板。
pub fn layout(
    sel: Selection,
    screen: (i32, i32),
    tool: Option<Shape>,
    style_open: bool,
    picks: &Picks,
    line_menu_open: bool,
    arrow_menu_open: bool,
) -> Layout {
    let button = TB_BTN;
    let content = 13 * (button + GAP) + 2 * DIVIDER - GAP;
    let width = content + PAD * 2;
    let height = button;

    let mut x = sel.x + (sel.width - width) / 2;
    x = x.clamp(4, (screen.0 - width - 4).max(4));
    let mut y = sel.y + sel.height + 10;
    if y + height > screen.1 - 4 {
        y = (sel.y - 10 - height).max(4);
    }

    let top = y;
    let mut buttons: Vec<(Action, RECT)> = Vec::new();
    let mut separators: Vec<(i32, i32, i32)> = Vec::new();
    let mut bx = x + PAD;
    let mut push = |action: Action, w: i32, bx: &mut i32| {
        buttons.push((
            action,
            RECT {
                left: *bx,
                top,
                right: *bx + w,
                bottom: top + button,
            },
        ));
        *bx += w + GAP;
    };
    let mut sep = |bx: &mut i32, separators: &mut Vec<(i32, i32, i32)>| {
        separators.push((*bx + (DIVIDER - GAP) / 2, y, y + height));
        *bx += DIVIDER;
    };

    for group in [Group::Shapes, Group::Lines, Group::Pixels] {
        push(Action::Group(group), button, &mut bx);
    }
    for shape in [
        Shape::Pencil,
        Shape::Highlighter,
        Shape::Text,
        Shape::Serial,
    ] {
        push(Action::Tool(shape), button, &mut bx);
    }
    push(Action::Delete, button, &mut bx);
    sep(&mut bx, &mut separators);
    push(Action::Undo, button, &mut bx);
    push(Action::Redo, button, &mut bx);
    sep(&mut bx, &mut separators);
    push(Action::Cancel, button, &mut bx);
    push(Action::Save, button, &mut bx);
    push(Action::Copy, button, &mut bx);

    let mut layout = Layout {
        x,
        y,
        width,
        height,
        buttons,
        separators,
        styles: None,
        style_bg: None,
        style_separators: Vec::new(),
        menus: Vec::new(),
    };

    if tool.is_some() && style_open {
        let is_shape = matches!(tool, Some(Shape::Rect | Shape::Ellipse | Shape::Circle));
        let is_line = matches!(tool, Some(Shape::Line | Shape::Arrow | Shape::Polyline));
        let is_pixels = matches!(tool, Some(Shape::Mosaic | Shape::Blur));
        let line_pick = picks.lines;
        let row_h = TB_BTN;
        let mut ry = y + height + 4;
        if ry + row_h + 16 > screen.1 - 4 {
            ry = (y - 4 - row_h - 16).max(4);
        }
        let stop = ry;
        let mut style_buttons: Vec<(Action, RECT)> = Vec::new();
        let mut style_separators: Vec<(i32, i32, i32)> = Vec::new();
        let mut sx = x;
        let mut push_style = |action: Action, w: i32, sx: &mut i32| -> RECT {
            let rect = RECT {
                left: *sx,
                top: stop,
                right: *sx + w,
                bottom: stop + row_h,
            };
            style_buttons.push((action, rect));
            *sx += w + GAP;
            rect
        };
        let mut sep_style = |sx: &mut i32, seps: &mut Vec<(i32, i32, i32)>| {
            seps.push((*sx + (DIVIDER - GAP) / 2, ry, ry + row_h));
            *sx += DIVIDER;
        };

        let mut line_btn: Option<RECT> = None;
        let mut arrow_btn: Option<RECT> = None;
        if is_shape || is_line {
            line_btn = Some(push_style(Action::LineStyle, 121, &mut sx));
            sep_style(&mut sx, &mut style_separators);
        }
        if is_line && line_pick == Shape::Arrow {
            arrow_btn = Some(push_style(Action::ArrowType, 121, &mut sx));
            sep_style(&mut sx, &mut style_separators);
        }
        if !is_pixels {
            push_style(Action::Width, 80, &mut sx);
        }
        if is_shape {
            push_style(Action::Fill, 68, &mut sx);
            sep_style(&mut sx, &mut style_separators);
            for shape in [Shape::Rect, Shape::Ellipse, Shape::Circle] {
                push_style(Action::Pick(Group::Shapes, shape), button, &mut sx);
            }
            sep_style(&mut sx, &mut style_separators);
        } else if is_pixels {
            push_style(Action::Strength, 96, &mut sx);
            sep_style(&mut sx, &mut style_separators);
            for shape in [Shape::Mosaic, Shape::Blur] {
                push_style(Action::PixelMode(shape), button, &mut sx);
            }
        } else if is_line {
            sep_style(&mut sx, &mut style_separators);
            for shape in [Shape::Line, Shape::Arrow, Shape::Polyline] {
                push_style(Action::Pick(Group::Lines, shape), button, &mut sx);
            }
            sep_style(&mut sx, &mut style_separators);
        }
        if !is_pixels {
            for index in 0..PALETTE.len() {
                push_style(Action::Color(index), 34, &mut sx);
            }
        }
        for (action, rect) in &mut style_buttons {
            if matches!(action, Action::Color(_)) {
                let cy = (rect.top + rect.bottom) / 2;
                rect.top = cy - 17;
                rect.bottom = cy + 17;
            }
        }
        let row_w = sx - x;
        // 一级栏宽度只由自身按钮决定，不被二级栏撑长。
        layout.styles = Some(style_buttons);
        layout.style_bg = Some((x, ry, row_w, row_h));
        layout.style_separators = style_separators;
        if line_menu_open {
            if let Some(anchor) = line_btn {
                let menu_y = ry + row_h;
                for style in 0..2u8 {
                    layout.menus.push((
                        Action::LineStylePick(style),
                        RECT {
                            left: anchor.left,
                            top: menu_y + style as i32 * 32,
                            right: anchor.right,
                            bottom: menu_y + style as i32 * 32 + 30,
                        },
                    ));
                }
            }
        }
        if line_pick == Shape::Arrow && arrow_menu_open {
            if let Some(anchor) = arrow_btn {
                let menu_y = ry + row_h;
                for style in 1..3u8 {
                    layout.menus.push((
                        Action::ArrowPick(style),
                        RECT {
                            left: anchor.left,
                            top: menu_y + (style as i32 - 1) * 32,
                            right: anchor.right,
                            bottom: menu_y + (style as i32 - 1) * 32 + 30,
                        },
                    ));
                }
            }
        }
    }

    layout
}

pub fn hit(layout: &Layout, x: i32, y: i32) -> Option<Action> {
    for (action, rect) in &layout.menus {
        if inside(*rect, x, y) {
            return Some(*action);
        }
    }
    for group in [&layout.styles] {
        if let Some(buttons) = group {
            for (action, rect) in buttons {
                if inside(*rect, x, y) {
                    return Some(*action);
                }
            }
        }
    }
    for (action, rect) in &layout.buttons {
        if inside(*rect, x, y) {
            return Some(*action);
        }
    }
    None
}

fn inside(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

pub fn tooltip(action: Action) -> String {
    match action {
        Action::Group(Group::Shapes) => "形状：矩形 / 椭圆 / 正圆".to_owned(),
        Action::Group(Group::Lines) => "线条：直线 / 箭头 / 折线".to_owned(),
        Action::Group(Group::Pixels) => "像素：马赛克 / 模糊".to_owned(),
        Action::Tool(shape) => shape.label().to_owned(),
        Action::Pick(_, shape) => shape.label().to_owned(),
        Action::Color(_) => "颜色".to_owned(),
        Action::Width => "线宽：滚轮调整".to_owned(),
        Action::Fill => "填充开关".to_owned(),
        Action::Strength => "强度：滚轮调整".to_owned(),
        Action::PixelMode(_) => "像素模式".to_owned(),
        Action::LineStyle => "线型".to_owned(),
        Action::ArrowType => "箭头类型".to_owned(),
        Action::LineStylePick(0) => "实线".to_owned(),
        Action::LineStylePick(_) => "虚线".to_owned(),
        Action::ArrowPick(0) => "无箭头".to_owned(),
        Action::ArrowPick(1) => "单向箭头".to_owned(),
        Action::ArrowPick(_) => "双向箭头".to_owned(),
        Action::Undo => "撤销 (Ctrl+Z)".to_owned(),
        Action::Redo => "重做 (Ctrl+Y)".to_owned(),
        Action::Delete => "删除选中标注 (Delete)".to_owned(),
        Action::Save => "保存到目录…".to_owned(),
        Action::Copy => "复制并完成 (Ctrl+C)".to_owned(),
        Action::Cancel => "取消 (Esc)".to_owned(),
    }
}

fn round_panel(hdc: HDC, rect: RECT, fill_color: u32, border_color: u32) {
    unsafe {
        let brush = CreateSolidBrush(fill_color);
        let pen = CreatePen(PS_SOLID, 1, border_color);
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, pen);
        RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 10, 10);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        DeleteObject(pen);
        DeleteObject(brush);
    }
}

fn fill(hdc: HDC, rect: RECT, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        DeleteObject(brush);
    }
}

fn draw_line_sample(hdc: HDC, rect: RECT, color: u32, style: u8) {
    let cy = (rect.top + rect.bottom) / 2;
    let left = rect.left + 15;
    let right = rect.right - 28;
    if style == 1 {
        let mut x = left;
        while x < right {
            let end = (x + 8).min(right);
            line(hdc, x, cy, end, cy, 2, color);
            x += 14;
        }
    } else {
        line(hdc, left, cy, right, cy, 2, color);
    }
}

fn draw_menu_line(hdc: HDC, rect: RECT, color: u32, style: u8) {
    let cy = (rect.top + rect.bottom) / 2;
    let left = rect.left + 10;
    let right = rect.right - 15;
    if style == 1 {
        let mut x = left;
        while x < right {
            let end = (x + 8).min(right);
            line(hdc, x, cy, end, cy, 2, color);
            x += 14;
        }
    } else {
        line(hdc, left, cy, right, cy, 2, color);
    }
}

fn draw_chevron(hdc: HDC, rect: RECT, color: u32, open: bool) {
    let x = rect.right - 10;
    let y = (rect.top + rect.bottom) / 2;
    if open {
        line(hdc, x - 4, y + 1, x, y - 3, 2, color);
        line(hdc, x + 4, y + 1, x, y - 3, 2, color);
    } else {
        line(hdc, x - 4, y - 3, x, y + 1, 2, color);
        line(hdc, x + 4, y - 3, x, y + 1, 2, color);
    }
}

fn stroke_rect(hdc: HDC, rect: RECT, color: u32, thickness: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, thickness.max(1), color);
        let old_pen = SelectObject(hdc, pen);
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        Rectangle(hdc, rect.left, rect.top, rect.right, rect.bottom);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        DeleteObject(pen);
    }
}

fn line(hdc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, thickness: i32, color: u32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, thickness.max(1), color);
        let old_pen = SelectObject(hdc, pen);
        MoveToEx(hdc, x1, y1, std::ptr::null_mut());
        LineTo(hdc, x2, y2);
        SelectObject(hdc, old_pen);
        DeleteObject(pen);
    }
}

fn solid_head(hdc: HDC, tip: (i32, i32), dir: (f32, f32), size: f32, color: u32) {
    let nx = -dir.1;
    let ny = dir.0;
    let bx = tip.0 as f32 - dir.0 * size;
    let by = tip.1 as f32 - dir.1 * size;
    let pts = [
        windows_sys::Win32::Foundation::POINT { x: tip.0, y: tip.1 },
        windows_sys::Win32::Foundation::POINT {
            x: (bx + nx * size * 0.6) as i32,
            y: (by + ny * size * 0.6) as i32,
        },
        windows_sys::Win32::Foundation::POINT {
            x: (bx - nx * size * 0.6) as i32,
            y: (by - ny * size * 0.6) as i32,
        },
    ];
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush);
        let pen = CreatePen(PS_SOLID, 1, color);
        let old_pen = SelectObject(hdc, pen);
        Polygon(hdc, pts.as_ptr(), 3);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        DeleteObject(pen);
        DeleteObject(brush);
    }
}

fn icon(hdc: HDC, shape: Shape, rect: RECT, color: u32) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let r = 14;
    match shape {
        Shape::Rect => stroke_rect(
            hdc,
            RECT {
                left: cx - r,
                top: cy - r + 2,
                right: cx + r,
                bottom: cy + r - 2,
            },
            color,
            3,
        ),
        Shape::Ellipse => unsafe {
            let pen = CreatePen(PS_SOLID, 3, color);
            let old = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            Ellipse(hdc, cx - r, cy - r + 3, cx + r, cy + r - 3);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old);
            DeleteObject(pen);
        },
        Shape::Circle => unsafe {
            let pen = CreatePen(PS_SOLID, 3, color);
            let old = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old);
            DeleteObject(pen);
        },
        Shape::Line => line(hdc, cx - r, cy + r, cx + r, cy - r, 3, color),
        Shape::Arrow => {
            line(hdc, cx - r, cy + r, cx + r - 5, cy - r + 5, 3, color);
            solid_head(hdc, (cx + r, cy - r), (0.707, -0.707), 9.0, color);
        }
        Shape::Polyline => {
            line(hdc, cx - r, cy + 5, cx - 3, cy - 7, 3, color);
            line(hdc, cx - 3, cy - 7, cx + 4, cy + 7, 3, color);
            line(hdc, cx + 4, cy + 7, cx + r, cy - 5, 3, color);
        }
        Shape::Pencil => {
            let (tx, ty) = (cx - 8, cy + 8);
            let (hx, hy) = (cx + 8, cy - 8);
            line(hdc, tx, ty, hx, hy, 3, color);
            line(hdc, tx + 3, ty + 1, hx + 1, hy - 3, 2, color);
            line(hdc, tx, ty, tx - 3, ty + 2, 3, color);
            line(hdc, tx - 3, ty + 2, tx + 1, ty - 3, 3, color);
            line(hdc, tx + 1, ty - 3, tx + 4, ty, 3, color);
        }
        Shape::Highlighter => {
            line(hdc, cx - 6, cy + 6, cx + 6, cy - 6, 9, color);
            line(hdc, cx - 9, cy + 9, cx - 5, cy + 5, 4, color);
            line(hdc, cx + 6, cy - 6, cx + 9, cy - 9, 3, color);
        }
        Shape::Mosaic => {
            let size = 10;
            let gap = 3;
            for row in 0..2 {
                for col in 0..2 {
                    let left = cx - size - gap / 2 + col * (size + gap);
                    let top = cy - size - gap / 2 + row * (size + gap);
                    fill(
                        hdc,
                        RECT {
                            left,
                            top,
                            right: left + size,
                            bottom: top + size,
                        },
                        color,
                    );
                }
            }
        }
        Shape::Blur => {
            for i in 0..4 {
                let y = cy - 8 + i * 5;
                line(hdc, cx - r, y, cx + r, y, 3, color);
            }
        }
        Shape::Text => {
            line(hdc, cx - 9, cy - 11, cx + 9, cy - 11, 3, color);
            line(hdc, cx, cy - 11, cx, cy + 11, 3, color);
        }
        Shape::Serial => unsafe {
            let pen = CreatePen(PS_SOLID, 3, color);
            let old = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old);
            DeleteObject(pen);
            line(hdc, cx, cy - 7, cx, cy + 7, 3, color);
            line(hdc, cx - 3, cy - 3, cx, cy - 7, 2, color);
        },
    }
}

fn shapes_group_icon(hdc: HDC, rect: RECT, color: u32) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    stroke_rect(
        hdc,
        RECT {
            left: cx - 11,
            top: cy - 11,
            right: cx + 11,
            bottom: cy + 11,
        },
        color,
        2,
    );
}

fn curved_arrow(hdc: HDC, rect: RECT, color: u32, flip: bool) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let radius = 12.0f32;
    // 屏幕坐标：角度 0 在右侧，负角在上。开口 70°，朝左（撤销）/朝右（重做）。
    let start = 145.0f32.to_radians();
    let end = -145.0f32.to_radians();
    let steps = 32;
    let mut points: Vec<windows_sys::Win32::Foundation::POINT> = Vec::new();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = start + (end - start) * t;
        let mut x = radius * angle.cos();
        let y = radius * angle.sin();
        if flip {
            x = -x;
        }
        points.push(windows_sys::Win32::Foundation::POINT {
            x: cx + x as i32,
            y: cy + y as i32,
        });
    }
    unsafe {
        let pen = CreatePen(PS_SOLID, 2, color);
        let old = SelectObject(hdc, pen);
        Polyline(hdc, points.as_ptr(), points.len() as i32);
        SelectObject(hdc, old);
        DeleteObject(pen);
    }

    // 箭头：圆弧端点为底座中心，尖端沿该点的切线方向。
    let base = points[points.len() - 1];
    let prev = points[points.len() - 2];
    let dx = (base.x - prev.x) as f32;
    let dy = (base.y - prev.y) as f32;
    let length = (dx * dx + dy * dy).sqrt().max(0.001);
    let tx = dx / length;
    let ty = dy / length;
    let nx = -ty;
    let ny = tx;
    let size = 8.0f32;
    let half = size * 0.55;
    let tip_x = base.x as f32 + tx * size;
    let tip_y = base.y as f32 + ty * size;
    let points_tri = [
        windows_sys::Win32::Foundation::POINT {
            x: tip_x as i32,
            y: tip_y as i32,
        },
        windows_sys::Win32::Foundation::POINT {
            x: (base.x as f32 + nx * half) as i32,
            y: (base.y as f32 + ny * half) as i32,
        },
        windows_sys::Win32::Foundation::POINT {
            x: (base.x as f32 - nx * half) as i32,
            y: (base.y as f32 - ny * half) as i32,
        },
    ];
    unsafe {
        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(hdc, brush);
        let pen = CreatePen(PS_SOLID, 1, color);
        let old_pen = SelectObject(hdc, pen);
        Polygon(hdc, points_tri.as_ptr(), points_tri.len() as i32);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        DeleteObject(pen);
        DeleteObject(brush);
    }
}

fn action_icon(hdc: HDC, action: Action, rect: RECT, color: u32) {
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    match action {
        Action::Undo => curved_arrow(hdc, rect, color, false),
        Action::Redo => curved_arrow(hdc, rect, color, true),
        Action::Delete => {
            line(hdc, cx - 8, cy - 6, cx + 8, cy - 6, 2, color);
            line(hdc, cx - 4, cy - 10, cx + 4, cy - 10, 2, color);
            stroke_rect(
                hdc,
                RECT {
                    left: cx - 7,
                    top: cy - 3,
                    right: cx + 7,
                    bottom: cy + 10,
                },
                color,
                2,
            );
        }
        Action::Save => {
            stroke_rect(
                hdc,
                RECT {
                    left: cx - 10,
                    top: cy - 10,
                    right: cx + 10,
                    bottom: cy + 10,
                },
                color,
                2,
            );
            line(hdc, cx - 5, cy - 10, cx + 5, cy - 10, 3, color);
            stroke_rect(
                hdc,
                RECT {
                    left: cx - 6,
                    top: cy + 1,
                    right: cx + 6,
                    bottom: cy + 9,
                },
                color,
                2,
            );
        }
        Action::Copy => {
            stroke_rect(
                hdc,
                RECT {
                    left: cx - 10,
                    top: cy - 10,
                    right: cx + 4,
                    bottom: cy + 4,
                },
                color,
                2,
            );
            stroke_rect(
                hdc,
                RECT {
                    left: cx - 4,
                    top: cy - 4,
                    right: cx + 10,
                    bottom: cy + 10,
                },
                color,
                2,
            );
        }
        Action::Cancel => {
            line(hdc, cx - 8, cy - 8, cx + 8, cy + 8, 3, color);
            line(hdc, cx + 8, cy - 8, cx - 8, cy + 8, 3, color);
        }
        _ => {}
    }
}

pub struct ToolbarState {
    pub selection: Selection,
    pub screen: (i32, i32),
    pub cursor: Option<(i32, i32)>,
    pub tool: Option<Shape>,
    pub picks: Picks,

    pub style_open: bool,
    pub color_index: usize,
    pub width: i32,
    pub fill: bool,
    pub strength: i32,
    pub line_style: u8,
    pub arrow_style: u8,
    pub line_menu_open: bool,
    pub arrow_menu_open: bool,
    pub delete_mode: bool,
}

pub unsafe fn draw(hdc: HDC, state: &ToolbarState) {
    let layout = layout(
        state.selection,
        state.screen,
        state.tool,
        state.style_open,
        &state.picks,
        state.line_menu_open,
        state.arrow_menu_open,
    );

    let bg = RECT {
        left: layout.x,
        top: layout.y,
        right: layout.x + layout.width,
        bottom: layout.y + layout.height,
    };
    round_panel(hdc, bg, rgb(0xff, 0xff, 0xff), rgb(0xd0, 0xd0, 0xd0));

    let cursor = state.cursor;
    let mut hovered: Option<(Action, RECT)> = None;

    let mut draw_buttons = |buttons: &[(Action, RECT)]| {
        for (action, rect) in buttons {
            let is_hovered = cursor.map(|(x, y)| inside(*rect, x, y)).unwrap_or(false);
            if is_hovered {
                hovered = Some((*action, *rect));
            }
            let active = match action {
                Action::Tool(shape) => Some(*shape) == state.tool,
                Action::Pick(_, shape) => Some(*shape) == state.tool,
                Action::Group(group) => {
                    state.tool.and_then(group_of) == Some(*group)
                }
                Action::Color(index) => *index == state.color_index,
                Action::Fill => state.fill,
                Action::LineStyle => false,
                Action::ArrowType => false,
                Action::PixelMode(shape) => Some(*shape) == state.tool,
                Action::Strength => false,
                Action::Delete => state.delete_mode,
                _ => false,
            };
            if is_hovered || active {
                fill(
                    hdc,
                    *rect,
                    if active {
                        rgb(0xb8, 0xd2, 0xff)
                    } else {
                        rgb(0xf2, 0xf2, 0xf2)
                    },
                );
            }
            let icon_color = rgb(0x20, 0x20, 0x20);
            match action {
                Action::Group(group) => match group {
                    Group::Shapes => shapes_group_icon(hdc, *rect, icon_color),
                    Group::Lines => icon(hdc, Shape::Line, *rect, icon_color),
                    Group::Pixels => icon(hdc, Shape::Mosaic, *rect, icon_color),
                },
                Action::Tool(shape) => icon(hdc, *shape, *rect, icon_color),
                Action::Pick(_, shape) => icon(hdc, *shape, *rect, icon_color),
                Action::Color(index) => {
                    let inset = 6;
                    fill(
                        hdc,
                        RECT {
                            left: rect.left + inset,
                            top: rect.top + inset,
                            right: rect.right - inset,
                            bottom: rect.bottom - inset,
                        },
                        PALETTE[*index],
                    );
                    if active {
                        stroke_rect(hdc, *rect, rgb(0xff, 0xff, 0xff), 2);
                    }
                }
                Action::Width => {
                    let cy = (rect.top + rect.bottom) / 2;
                    let radius = (state.width / 2).clamp(2, 15);
                    let dot_x = rect.left + 24;
                    let brush = CreateSolidBrush(rgb(0x10, 0x10, 0x10));
                    let pen = CreatePen(PS_SOLID, 1, rgb(0x10, 0x10, 0x10));
                    let old_brush = SelectObject(hdc, brush);
                    let old_pen = SelectObject(hdc, pen);
                    Ellipse(hdc, dot_x - radius, cy - radius, dot_x + radius, cy + radius);
                    SelectObject(hdc, old_pen);
                    SelectObject(hdc, old_brush);
                    DeleteObject(pen);
                    DeleteObject(brush);

                    let text = state.width.to_string();
                    let font = CreateFontW(
                        -19,
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
                    );
                    let old_font = SelectObject(hdc, font);
                    SetBkMode(hdc, TRANSPARENT as i32);
                    SetTextColor(hdc, rgb(0x20, 0x20, 0x20));
                    let wide: Vec<u16> = text.encode_utf16().collect();
                    TextOutW(hdc, dot_x + radius + 10, cy - 11, wide.as_ptr(), wide.len() as i32);
                    SelectObject(hdc, old_font);
                    DeleteObject(font);
                }
                Action::Strength => {
                    let text = format!("强度 {}", state.strength);
                    let font = CreateFontW(
                        -21,0,0,0,400,0,0,0,
                        DEFAULT_CHARSET as u32,
                        OUT_DEFAULT_PRECIS as u32,
                        CLIP_DEFAULT_PRECIS as u32,
                        CLEARTYPE_QUALITY as u32,
                        (DEFAULT_PITCH | FF_DONTCARE) as u32,
                        windows_sys::w!("Microsoft YaHei") as *const u16,
                    );
                    let old_font = SelectObject(hdc, font);
                    SetBkMode(hdc, TRANSPARENT as i32);
                    SetTextColor(hdc, icon_color);
                    let wide: Vec<u16> = text.encode_utf16().collect();
                    let mut rc = RECT {
                        left: rect.left + 12,
                        top: rect.top,
                        right: rect.right,
                        bottom: rect.bottom,
                    };
                    DrawTextW(
                        hdc,
                        wide.as_ptr(),
                        wide.len() as i32,
                        &mut rc,
                        DT_SINGLELINE | DT_VCENTER | DT_LEFT,
                    );
                    SelectObject(hdc, old_font);
                    DeleteObject(font);
                    if active {
                        stroke_rect(hdc, *rect, rgb(0x30, 0x7d, 0xef), 2);
                    }
                }
                Action::PixelMode(shape) => {
                    icon(hdc, *shape, *rect, icon_color);
                    if active {
                        stroke_rect(hdc, *rect, rgb(0x30, 0x7d, 0xef), 2);
                    }
                }
                Action::LineStyle => {
                    draw_line_sample(
                        hdc,
                        *rect,
                        icon_color,
                        if state.line_style == 1 { 1 } else { 0 },
                    );
                    draw_chevron(hdc, *rect, rgb(0x80, 0x80, 0x80), state.line_menu_open);
                    if active {
                        stroke_rect(hdc, *rect, rgb(0x30, 0x7d, 0xef), 2);
                    }
                }
                Action::ArrowType => {
                    let cy = (rect.top + rect.bottom) / 2;
                    let left = rect.left + 14;
                    let right = rect.right - 30;
                    line(hdc, left, cy, right - 4, cy, 2, icon_color);
                    solid_head(hdc, (right, cy), (1.0, 0.0), 9.0, icon_color);
                    if state.arrow_style == 2 {
                        solid_head(hdc, (left, cy), (-1.0, 0.0), 9.0, icon_color);
                    }
                    draw_chevron(hdc, *rect, rgb(0x80, 0x80, 0x80), state.arrow_menu_open);
                    if active {
                        stroke_rect(hdc, *rect, rgb(0x30, 0x7d, 0xef), 2);
                    }
                }
                Action::Fill => {
            let inset = 8;
            let square = RECT {
                left: rect.left + inset,
                top: rect.top + inset,
                right: rect.right - inset,
                bottom: rect.bottom - inset,
            };
            stroke_rect(hdc, square, icon_color, 2);
            let mut offset = -((square.bottom - square.top) as i32);
            while offset < (square.right - square.left) {
                let x1 = square.left + 4 + offset;
                let y1 = square.bottom - 4;
                let x2 = x1 + (square.bottom - square.top - 8);
                let y2 = square.top + 4;
                if x2 > square.left + 2 && x1 < square.right - 2 {
                    let start_x = x1.max(square.left + 2);
                    let start_y = y1 - (start_x - x1);
                    let end_x = x2.min(square.right - 2);
                    let end_y = y2 + (x2 - end_x);
                    line(hdc, start_x, start_y, end_x, end_y, 1, icon_color);
                }
                offset += 7;
            }
                    
                    if state.fill {
                        stroke_rect(hdc, *rect, rgb(0xff, 0xff, 0xff), 2);
                    }
                }
                other => action_icon(hdc, *other, *rect, icon_color),
            }
        }
    };

    let buttons = layout.buttons.clone();
    let clip = CreateRoundRectRgn(
        bg.left,
        bg.top,
        bg.right + 1,
        bg.bottom + 1,
        10,
        10,
    );
    SelectClipRgn(hdc, clip);
    draw_buttons(&buttons);
    for (x, top, bottom) in &layout.separators {
        line(hdc, *x, *top, *x, *bottom, 1, rgb(0xb0, 0xb0, 0xb0));
    }
    SelectClipRgn(hdc, std::ptr::null_mut());
    DeleteObject(clip);
    if let (Some((px, py, pw, ph)), Some(styles)) = (layout.style_bg, layout.styles.clone()) {
        let bg = RECT {
            left: px,
            top: py,
            right: px + pw,
            bottom: py + ph,
        };
        round_panel(hdc, bg, rgb(0xff, 0xff, 0xff), rgb(0xd0, 0xd0, 0xd0));
        let clip = CreateRoundRectRgn(bg.left, bg.top, bg.right + 1, bg.bottom + 1, 10, 10);
        SelectClipRgn(hdc, clip);
        draw_buttons(&styles);
        for (x, top, bottom) in &layout.style_separators {
            line(hdc, *x, *top, *x, *bottom, 1, rgb(0xb0, 0xb0, 0xb0));
        }
        SelectClipRgn(hdc, std::ptr::null_mut());
        DeleteObject(clip);
    }
    if !layout.menus.is_empty() {
        let mut left = i32::MAX;
        let mut top = i32::MAX;
        let mut right = i32::MIN;
        let mut bottom = i32::MIN;
        for (_, rect) in &layout.menus {
            left = left.min(rect.left);
            top = top.min(rect.top);
            right = right.max(rect.right);
            bottom = bottom.max(rect.bottom);
        }
        let bg = RECT {
            left,
            top,
            right,
            bottom,
        };
        fill(hdc, bg, rgb(0xff, 0xff, 0xff));
        stroke_rect(hdc, bg, rgb(0xc0, 0xc0, 0xc0), 1);
        for (action, rect) in &layout.menus {
            let is_hovered = cursor.map(|(x, y)| inside(*rect, x, y)).unwrap_or(false);
            if is_hovered {
                fill(hdc, *rect, rgb(0xb8, 0xd2, 0xff));
                hovered = Some((*action, *rect));
            }
            match action {
                Action::LineStylePick(style) => {
                    draw_menu_line(hdc, *rect, rgb(0x20, 0x20, 0x20), *style);
                }
                Action::ArrowPick(style) => {
                    let cy = (rect.top + rect.bottom) / 2;
                    let left = rect.left + 14;
                    let right = rect.right - 14;
                    let color = rgb(0x20, 0x20, 0x20);
                    line(hdc, left, cy, right - 4, cy, 2, color);
                    solid_head(hdc, (right, cy), (1.0, 0.0), 9.0, color);
                    if *style == 2 {
                        solid_head(hdc, (left, cy), (-1.0, 0.0), 9.0, color);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some((action, rect)) = hovered {
        let text = tooltip(action);
        if !text.is_empty() {
            draw_tooltip(hdc, &text, rect, state.screen);
        }
    }
}

unsafe fn draw_tooltip(hdc: HDC, text: &str, anchor: RECT, screen: (i32, i32)) {
    let font = CreateFontW(
        -15,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        windows_sys::w!("Microsoft YaHei") as *const u16,
    );
    let old_font = SelectObject(hdc, font);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut extent: windows_sys::Win32::Foundation::SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut extent);

    let width = extent.cx + 16;
    let height = extent.cy + 8;
    let mut left = anchor.left + (anchor.right - anchor.left - width) / 2;
    left = left.clamp(4, (screen.0 - width - 4).max(4));
    let mut top = anchor.top - height - 6;
    if top < 4 {
        top = anchor.bottom + 6;
    }

    let bg = RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    fill(hdc, bg, rgb(0x10, 0x10, 0x14));
    stroke_rect(hdc, bg, rgb(0x5a, 0x5a, 0x62), 1);
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, rgb(0xf0, 0xf0, 0xf0));
    TextOutW(hdc, left + 8, top + 4, wide.as_ptr(), wide.len() as i32);

    SelectObject(hdc, old_font);
    DeleteObject(font);
}

// ---- 选区调整 ----

pub const HANDLE_RADIUS: i32 = 9;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

pub const HANDLES: [Handle; 8] = [
    Handle::NorthWest,
    Handle::North,
    Handle::NorthEast,
    Handle::East,
    Handle::SouthEast,
    Handle::South,
    Handle::SouthWest,
    Handle::West,
];

pub fn handle_points(sel: Selection) -> [(i32, i32); 8] {
    let mut points = [(0, 0); 8];
    for (index, handle) in HANDLES.iter().enumerate() {
        points[index] = handle_point(sel, *handle);
    }
    points
}

fn handle_point(sel: Selection, handle: Handle) -> (i32, i32) {
    match handle {
        Handle::NorthWest => (sel.x, sel.y),
        Handle::North => (sel.x + sel.width / 2, sel.y),
        Handle::NorthEast => (sel.x + sel.width, sel.y),
        Handle::East => (sel.x + sel.width, sel.y + sel.height / 2),
        Handle::SouthEast => (sel.x + sel.width, sel.y + sel.height),
        Handle::South => (sel.x + sel.width / 2, sel.y + sel.height),
        Handle::SouthWest => (sel.x, sel.y + sel.height),
        Handle::West => (sel.x, sel.y + sel.height / 2),
    }
}

pub fn handle_at(sel: Selection, x: i32, y: i32) -> Option<Handle> {
    for handle in HANDLES {
        let (hx, hy) = handle_point(sel, handle);
        if (x - hx).abs() <= HANDLE_RADIUS && (y - hy).abs() <= HANDLE_RADIUS {
            return Some(handle);
        }
    }
    None
}

pub fn border_at(sel: Selection, x: i32, y: i32) -> bool {
    let inside = x >= sel.x && y >= sel.y && x <= sel.x + sel.width && y <= sel.y + sel.height;
    if !inside {
        return false;
    }
    let edge = 6;
    x - sel.x <= edge
        || sel.x + sel.width - x <= edge
        || y - sel.y <= edge
        || sel.y + sel.height - y <= edge
}

pub fn resize(sel: Selection, handle: Handle, x: i32, y: i32, screen: (i32, i32)) -> Selection {
    let mut left = sel.x;
    let mut top = sel.y;
    let mut right = sel.x + sel.width;
    let mut bottom = sel.y + sel.height;
    let x = x.clamp(0, screen.0);
    let y = y.clamp(0, screen.1);
    match handle {
        Handle::NorthWest => {
            left = x;
            top = y;
        }
        Handle::North => top = y,
        Handle::NorthEast => {
            right = x;
            top = y;
        }
        Handle::East => right = x,
        Handle::SouthEast => {
            right = x;
            bottom = y;
        }
        Handle::South => bottom = y,
        Handle::SouthWest => {
            left = x;
            bottom = y;
        }
        Handle::West => left = x,
    }
    if right - left < 8 {
        if matches!(handle, Handle::NorthWest | Handle::SouthWest | Handle::West) {
            left = right - 8;
        } else {
            right = left + 8;
        }
    }
    if bottom - top < 8 {
        if matches!(handle, Handle::NorthWest | Handle::NorthEast | Handle::North) {
            top = bottom - 8;
        } else {
            bottom = top + 8;
        }
    }
    Selection {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

pub fn translate(sel: Selection, dx: i32, dy: i32, screen: (i32, i32)) -> Selection {
    let x = (sel.x + dx).clamp(0, (screen.0 - sel.width).max(0));
    let y = (sel.y + dy).clamp(0, (screen.1 - sel.height).max(0));
    Selection {
        x,
        y,
        width: sel.width,
        height: sel.height,
    }
}
