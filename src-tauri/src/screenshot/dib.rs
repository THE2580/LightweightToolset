use std::ffi::c_void;

use windows_sys::Win32::Graphics::Gdi::*;

/// 一件常驻的 32bpp 自顶向下 DIB 段 + 其内存 DC。
///
/// 用 DIB 段而不是每帧 `SetDIBitsToDevice`：后者要把 16 MB 位图逐像素转成屏幕格式
/// （整屏一次 40 ms 量级），拖动时必然露出"先画暗、再画亮"的中间态。
pub(crate) struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl Surface {
    pub(crate) unsafe fn new(width: i32, height: i32) -> Option<Self> {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = width;
        // 负高度 = 自顶向下，内存行序与屏幕一致，BitBlt 坐标可直接对应。
        bi.bmiHeader.biHeight = -height;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() {
            DeleteDC(dc);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        Some(Self {
            dc,
            bitmap,
            old,
            bits: bits as *mut u8,
            width,
            height,
        })
    }

    pub(crate) fn dc(&self) -> HDC {
        self.dc
    }

    pub(crate) fn pixels(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.bits, self.width as usize * 4 * self.height as usize)
        }
    }

    /// 单像素填充，仅用于构造压暗用的 1×1 黑色源。
    pub(crate) fn set_pixel(&mut self, r: u8, g: u8, b: u8) {
        unsafe {
            *self.bits = b;
            *self.bits.add(1) = g;
            *self.bits.add(2) = r;
            *self.bits.add(3) = 255;
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
