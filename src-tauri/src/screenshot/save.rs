use std::path::PathBuf;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// 把自上而下的 BGRA 编码为 PNG 字节。
pub fn encode_png(bgra_top_down: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut rgba = bgra_top_down.to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("写入 PNG 头失败: {error}"))?;
        writer
            .write_image_data(&rgba)
            .map_err(|error| format!("写入 PNG 数据失败: {error}"))?;
    }
    Ok(out)
}

pub fn default_dir() -> PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Pictures")
        .join("LightweightToolset")
        .join("Screenshots")
}

fn timestamp_name() -> String {
    unsafe {
        let mut st: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut st);
        format!(
            "Screenshot_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.png",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    }
}

/// 写入指定目录并返回最终路径；同一秒内重复截图自动加 `-1`、`-2` 后缀。
pub fn write_png_to(dir: &std::path::Path, png: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("创建截图目录失败: {error}"))?;
    let base = timestamp_name();
    let mut path = dir.join(&base);
    let mut index = 1;
    while path.exists() {
        let stem = base.trim_end_matches(".png");
        path = dir.join(format!("{stem}-{index}.png"));
        index += 1;
        if index > 999 {
            return Err("截图文件名冲突过多".to_owned());
        }
    }
    std::fs::write(&path, png).map_err(|error| format!("写入截图文件失败: {error}"))?;
    Ok(path)
}

/// 写入默认目录。
pub fn write_file(png: &[u8]) -> Result<PathBuf, String> {
    write_png_to(&default_dir(), png)
}
