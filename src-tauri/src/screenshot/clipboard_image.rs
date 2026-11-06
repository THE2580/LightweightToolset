use std::ffi::c_void;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

/// 把裁切结果写入剪贴板：
/// - `CF_DIB`（32bpp BI_RGB，自下而上）覆盖传统应用；
/// - `PNG` 格式覆盖浏览器、微信等现代应用。
pub fn copy_image(
    width: i32,
    height: i32,
    bottom_up_bgra: &[u8],
    png: Option<&[u8]>,
) -> Result<(), String> {
    let image_size = bottom_up_bgra.len();
    let dib_size = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>()
        + image_size;

    unsafe {
        if OpenClipboard(std::ptr::null_mut::<c_void>() as HWND) == 0 {
            return Err("打开剪贴板失败".to_owned());
        }
        let result = (|| -> Result<(), String> {
            EmptyClipboard();

            let handle = GlobalAlloc(GMEM_MOVEABLE, dib_size);
            if handle.is_null() {
                return Err("分配 DIB 内存失败".to_owned());
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return Err("锁定 DIB 内存失败".to_owned());
            }
            let header = ptr as *mut u8;
            let mut bi: windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER = std::mem::zeroed();
            bi.biSize = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>()
                as u32;
            bi.biWidth = width;
            bi.biHeight = height;
            bi.biPlanes = 1;
            bi.biBitCount = 32;
            bi.biCompression = windows_sys::Win32::Graphics::Gdi::BI_RGB;
            bi.biSizeImage = image_size as u32;
            std::ptr::copy_nonoverlapping(
                &bi as *const _ as *const u8,
                header,
                std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>(),
            );
            std::ptr::copy_nonoverlapping(
                bottom_up_bgra.as_ptr(),
                header.add(std::mem::size_of::<
                    windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER,
                >()),
                image_size,
            );
            GlobalUnlock(handle);
            if SetClipboardData(CF_DIB as u32, handle).is_null() {
                return Err("写入 CF_DIB 失败".to_owned());
            }

            if let Some(png) = png {
                let format = RegisterClipboardFormatW(windows_sys::w!("PNG"));
                if format != 0 {
                    let png_handle = GlobalAlloc(GMEM_MOVEABLE, png.len());
                    if !png_handle.is_null() {
                        let png_ptr = GlobalLock(png_handle);
                        if !png_ptr.is_null() {
                            std::ptr::copy_nonoverlapping(
                                png.as_ptr(),
                                png_ptr as *mut u8,
                                png.len(),
                            );
                            GlobalUnlock(png_handle);
                            if SetClipboardData(format, png_handle).is_null() {
                                // PNG 只是增强格式，失败不影响主链路。
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        CloseClipboard();
        result
    }
}

/// 把一段文本写入剪贴板（放大镜下按 C 复制颜色值）。
pub fn copy_text(text: &str) -> Result<(), String> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        if OpenClipboard(std::ptr::null_mut::<c_void>() as HWND) == 0 {
            return Err("打开剪贴板失败".to_owned());
        }
        let result = (|| -> Result<(), String> {
            EmptyClipboard();
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if handle.is_null() {
                return Err("分配文本内存失败".to_owned());
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return Err("锁定文本内存失败".to_owned());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
            GlobalUnlock(handle);
            if SetClipboardData(CF_UNICODETEXT as u32, handle).is_null() {
                return Err("写入 CF_UNICODETEXT 失败".to_owned());
            }
            Ok(())
        })();
        CloseClipboard();
        result
    }
}
