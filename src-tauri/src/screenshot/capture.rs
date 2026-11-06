use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use super::dib::Surface;

/// 虚拟桌面矩形（物理像素）：(x, y, width, height)。
pub fn virtual_rect() -> Result<(i32, i32, i32, i32), String> {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if width <= 0 || height <= 0 {
            return Err(format!("虚拟桌面尺寸无效: {width}x{height}"));
        }
        Ok((x, y, width, height))
    }
}

/// 直接把屏幕 BitBlt 进目标 DIB 段——省掉 `GetDIBits` 与一次整图拷贝。
pub fn capture_into(surface: &Surface) -> Result<(), String> {
    let (vx, vy, width, height) = virtual_rect()?;
    if width != surface.width || height != surface.height {
        return Err(format!(
            "目标 DIB 尺寸不匹配: {}x{} vs {}x{}",
            surface.width, surface.height, width, height
        ));
    }
    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return Err("GetDC(NULL) 失败".to_owned());
        }
        let copied = BitBlt(
            surface.dc(),
            0,
            0,
            width,
            height,
            screen_dc,
            vx,
            vy,
            SRCCOPY,
        );
        ReleaseDC(std::ptr::null_mut(), screen_dc);
        if copied == 0 {
            return Err("BitBlt 失败".to_owned());
        }
        Ok(())
    }
}
