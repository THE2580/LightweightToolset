use std::path::PathBuf;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// 默认文件名模板：与旧版 `Screenshot_2026-11-08_14-21-53.png` 完全一致。
pub const DEFAULT_TEMPLATE: &str = "Screenshot_$yyyy$-$MM$-$dd$_$HH$-$mm$-$ss$";

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

fn local_time() -> SYSTEMTIME {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetLocalTime(&mut st);
    }
    st
}

/// Windows 文件名非法字符统一替换成 `-`，并去掉结尾的点与空格。
fn sanitize_file_name(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            _ => ch,
        })
        .collect();
    replaced.trim().trim_end_matches(['.', ' ']).to_owned()
}

/// 渲染文件模板：支持 `$yyyy$ $MM$ $dd$ $HH$ $mm$ $ss$`，输出带 `.png` 的文件名。
pub fn render_file_name(template: &str) -> String {
    let st = local_time();
    let template = template.trim();
    let template = if template.is_empty() {
        DEFAULT_TEMPLATE
    } else {
        template
    };
    let rendered = template
        .replace("$yyyy$", &format!("{:04}", st.wYear))
        .replace("$MM$", &format!("{:02}", st.wMonth))
        .replace("$dd$", &format!("{:02}", st.wDay))
        .replace("$HH$", &format!("{:02}", st.wHour))
        .replace("$mm$", &format!("{:02}", st.wMinute))
        .replace("$ss$", &format!("{:02}", st.wSecond));
    let stem = rendered
        .strip_suffix(".png")
        .or_else(|| rendered.strip_suffix(".PNG"))
        .unwrap_or(&rendered);
    let stem = sanitize_file_name(stem);
    if stem.is_empty() {
        format!(
            "Screenshot_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.png",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    } else {
        format!("{stem}.png")
    }
}

/// 写入指定目录并返回最终路径；同名文件自动加 `-1`、`-2` 后缀。
pub fn write_png_to(dir: &std::path::Path, png: &[u8], template: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("创建截图目录失败: {error}"))?;
    let base = render_file_name(template);
    let stem = base.trim_end_matches(".png").to_owned();
    let mut path = dir.join(&base);
    let mut index = 1;
    while path.exists() {
        path = dir.join(format!("{stem}-{index}.png"));
        index += 1;
        if index > 999 {
            return Err("截图文件名冲突过多".to_owned());
        }
    }
    std::fs::write(&path, png).map_err(|error| format!("写入截图文件失败: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::render_file_name;

    #[test]
    fn renders_default_template_shape() {
        let name = render_file_name("");
        assert!(name.starts_with("Screenshot_"), "{name}");
        assert!(name.ends_with(".png"), "{name}");
        assert_eq!(name.len(), "Screenshot_2026-11-08_14-21-53.png".len());
    }

    #[test]
    fn strips_extension_and_sanitizes() {
        let name = render_file_name("shot:$yyyy$/$MM$.png");
        assert!(name.ends_with(".png"), "{name}");
        assert!(!name.contains('/') && !name.contains(':'), "{name}");
    }

    #[test]
    fn empty_template_falls_back() {
        assert!(render_file_name("   ").starts_with("Screenshot_"));
    }
}
