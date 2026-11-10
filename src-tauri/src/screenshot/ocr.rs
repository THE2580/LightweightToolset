//! 本地 OCR（PP-OCRv6 small + `pure_onnx_ocr`，纯 Rust 推理，无 C/C++ 运行时）。
//!
//! 内存策略（与 PixPin 对齐）：所有识别都排进一条独立工作线程，模型在这条线程里
//! **按需加载、每识别完就丢弃**，常驻内存只留识别结果（文本 + 字符包围盒）。
//! 代价是每张图都要重新加载模型（约 100–300ms），由「正在检测文本」占位覆盖。
//!
//! 模型放在 exe 同级的 `ocr/ppocr_v6_small/`（安装包按 resources 带过去），
//! 开发期回落到仓库的 `src-tauri/resources/ocr/ppocr_v6_small/`。

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::OnceLock;

use pure_onnx_ocr::{OcrEngine, OcrEngineBuilder};

/// 识别置信度下限：低于它的行直接丢掉，避免把噪点当文字。
const MIN_CONFIDENCE: f32 = 0.35;
/// 模型目录名（exe 同级 `ocr/<名字>`）。
const MODEL_DIR: &str = "ppocr_v6_small";

/// 一个可选单元：一个字符 + 图像坐标包围盒 + 所属行号。
#[derive(Clone)]
pub struct Word {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub line: usize,
}

impl Word {
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// 一张贴图的识别结果：按阅读顺序排列的字符单元，以及每行的纯文本。
pub struct Text {
    pub words: Vec<Word>,
    pub lines: Vec<String>,
}

impl Text {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

/// 全角/宽字符：中文、日文、全角标点。
fn is_wide(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x303F | 0x3040..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
    )
}

/// 字符在行内的相对宽度权重：中日韩 1.0、ASCII 0.55、其它 0.9。
fn char_weight(c: char) -> f32 {
    if c.is_ascii() {
        0.55
    } else if is_wide(c) {
        1.0
    } else {
        0.9
    }
}

/// 把一行文本按权重拆成字符单元，填进 `words`。
fn push_line_chars(
    words: &mut Vec<Word>,
    line: usize,
    text: &str,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || width <= 0 || height <= 0 {
        return;
    }
    let total: f32 = chars.iter().map(|c| char_weight(*c)).sum();
    if total <= 0.0 {
        return;
    }
    let mut offset = 0.0f32;
    for c in chars {
        let weight = char_weight(c);
        let start = left + (width as f32 * offset / total).round() as i32;
        offset += weight;
        let end = left + (width as f32 * offset / total).round() as i32;
        words.push(Word {
            text: c.to_string(),
            x: start,
            y: top,
            width: (end - start).max(1),
            height,
            line,
        });
    }
}

/// 选中单元的文本：按阅读顺序拼接，跨行换行。
pub fn selected_text(text: &Text, selected: &[usize]) -> String {
    let mut out = String::new();
    let mut current: Option<usize> = None;
    for &index in selected {
        let Some(word) = text.words.get(index) else {
            continue;
        };
        if let Some(line) = current {
            if line != word.line {
                out.push('\n');
            }
        }
        current = Some(word.line);
        out.push_str(&word.text);
    }
    out
}

/// 整张图的文本（行间换行）。
pub fn all_text(text: &Text) -> String {
    text.lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect::<Vec<String>>()
        .join("\n")
}

fn candidate_dir(root: &std::path::Path) -> PathBuf {
    root.join(MODEL_DIR)
}

fn has_models(dir: &std::path::Path) -> bool {
    dir.join("det").join("inference.onnx").exists() && dir.join("rec").join("inference.onnx").exists()
}

/// 解析模型目录：exe 同级 `ocr/` > 仓库资源目录（开发期）。
fn model_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| candidate_dir(&parent.join("ocr"))))
    {
        if has_models(&dir) {
            return Ok(dir);
        }
    }
    // 开发期（直接跑 target/debug 或资源还没复制到临时目录）回落到仓库资源目录。
    let dev = candidate_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("ocr"));
    if has_models(&dev) {
        return Ok(dev);
    }
    Err("找不到 OCR 模型（应位于 exe 同级的 ocr/ppocr_v6_small）".to_owned())
}

/// 加载一次模型。调用方用完必须丢弃，不要让引擎常驻。
fn build_engine() -> Result<OcrEngine, String> {
    let dir = model_dir()?;
    let det_model = std::fs::read(dir.join("det").join("inference.onnx"))
        .map_err(|error| format!("读取检测模型失败：{error}"))?;
    let det_yaml = std::fs::read_to_string(dir.join("det").join("inference.yml"))
        .map_err(|error| format!("读取检测配置失败：{error}"))?;
    let rec_model = std::fs::read(dir.join("rec").join("inference.onnx"))
        .map_err(|error| format!("读取识别模型失败：{error}"))?;
    let rec_yaml = std::fs::read_to_string(dir.join("rec").join("inference.yml"))
        .map_err(|error| format!("读取识别配置失败：{error}"))?;
    OcrEngineBuilder::new()
        .det_model_bytes(det_model)
        .det_config_yaml(det_yaml)
        .rec_model_bytes(rec_model)
        .rec_config_yaml(rec_yaml)
        .build()
        .map_err(|error| format!("加载 OCR 模型失败：{error}"))
}

/// 一次识别任务：像素 + 尺寸 + 回传通道。
struct Job {
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    reply: mpsc::Sender<Result<Text, String>>,
}

/// 把已经释放的页还给系统，让「识别完」的内存立刻回落到基线附近。
fn trim_working_set() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// 单条 OCR 工作线程：串行识别、按需加载、用完释放。
fn worker() -> &'static mpsc::Sender<Job> {
    static SENDER: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("lwt-ocr".to_owned())
            .spawn(move || {
                let mut engine: Option<OcrEngine> = None;
                while let Ok(job) = receiver.recv() {
                    if engine.is_none() {
                        match build_engine() {
                            Ok(built) => engine = Some(built),
                            Err(error) => {
                                let _ = job.reply.send(Err(error));
                                continue;
                            }
                        }
                    }
                    let result = match engine.as_ref() {
                        Some(engine) => run_engine(engine, &job.pixels, job.width, job.height),
                        None => Err("加载 OCR 模型失败".to_owned()),
                    };
                    let _ = job.reply.send(result);
                    // 识别结束立刻丢弃引擎：常驻内存回到没有 OCR 时的水平，
                    // 下一张图再重新加载。
                    engine = None;
                    // 释放后把工作集还给系统：模型与中间张量已经 drop，但分配器
                    // 往往把页留在进程里（PixPin 的效果就是识别完立刻回落）。
                    trim_working_set();
                }
            })
            .is_ok();
        if !spawned {
            // 线程起不来时给调用方一个明确的失败通道。
            let (sender, _receiver) = mpsc::channel::<Job>();
            return sender;
        }
        sender
    })
}

/// 同步识别一张自顶向下 BGRA 图（内部排队到 OCR 工作线程）。
pub fn recognize(pixels: &[u8], width: i32, height: i32) -> Result<Text, String> {
    if width <= 0 || height <= 0 {
        return Err("图像尺寸无效".to_owned());
    }
    let expected = width as usize * height as usize * 4;
    if pixels.len() < expected {
        return Err("图像数据不完整".to_owned());
    }
    let (reply, receiver) = mpsc::channel();
    worker()
        .send(Job {
            pixels: pixels[..expected].to_vec(),
            width,
            height,
            reply,
        })
        .map_err(|_| "OCR 工作线程不可用".to_owned())?;
    receiver.recv().map_err(|_| "OCR 工作线程已退出".to_owned())?
}

/// 用已经加载好的引擎跑一张图。
fn run_engine(engine: &OcrEngine, pixels: &[u8], width: i32, height: i32) -> Result<Text, String> {
    use image::{DynamicImage, RgbaImage};

    // BGRA → RGBA；截图 DIB 的 alpha 恒为 0，这里统一补成不透明。
    let expected = width as usize * height as usize * 4;
    let mut rgba = vec![0u8; expected];
    for (source, target) in pixels[..expected].chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        target[0] = source[2];
        target[1] = source[1];
        target[2] = source[0];
        target[3] = 255;
    }
    let buffer = RgbaImage::from_raw(width as u32, height as u32, rgba)
        .ok_or_else(|| "图像数据不完整".to_owned())?;
    let results = engine
        .run_from_image(&DynamicImage::ImageRgba8(buffer))
        .map_err(|error| format!("文字识别失败：{error}"))?;
    Ok(build_text(&results, width, height))
}

/// 把识别结果转成按行排列的字符单元。
fn build_text(results: &[pure_onnx_ocr::OcrResult], width: i32, height: i32) -> Text {
    // 先按位置排序，保证「阅读顺序」稳定（检测模型的输出顺序不保证）。
    let mut rows: Vec<(&str, i32, i32, i32, i32)> = Vec::new();
    for result in results {
        let text = result.text.trim();
        if text.is_empty() || result.confidence < MIN_CONFIDENCE {
            continue;
        }
        let points = result.bounding_box.exterior().points();
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for point in points {
            let x = point.x().round() as i32;
            let y = point.y().round() as i32;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
        if right <= left || bottom <= top {
            continue;
        }
        // 检测框会略微超出图像边界，夹回图像内，选区才不会画到外面。
        let left = left.clamp(0, width - 1);
        let top = top.clamp(0, height - 1);
        let right = right.clamp(left + 1, width);
        let bottom = bottom.clamp(top + 1, height);
        rows.push((text, left, top, right - left, bottom - top));
    }
    rows.sort_by_key(|(_, left, top, _, _)| (*top / 8, *left));

    let mut words: Vec<Word> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (line, (text, left, top, box_width, box_height)) in rows.into_iter().enumerate() {
        lines.push(text.to_owned());
        push_line_chars(&mut words, line, text, left, top, box_width, box_height);
    }
    Text { words, lines }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(text: &str, line: usize) -> Word {
        Word {
            text: text.to_owned(),
            x: 0,
            y: (line as i32) * 24,
            width: 12,
            height: 20,
            line,
        }
    }

    #[test]
    fn selected_text_joins_chars_without_extra_spaces() {
        let text = Text {
            words: vec![cell("你", 0), cell("好", 0), cell("H", 1), cell("i", 1)],
            lines: vec!["你好".to_owned(), "Hi".to_owned()],
        };
        assert_eq!(selected_text(&text, &[0, 1]), "你好");
        assert_eq!(selected_text(&text, &[0, 1, 2, 3]), "你好\nHi");
        assert_eq!(selected_text(&text, &[2, 3]), "Hi");
        // 越界下标忽略。
        assert_eq!(selected_text(&text, &[0, 99]), "你");
    }

    #[test]
    fn all_text_skips_blank_lines() {
        let text = Text {
            words: vec![cell("Hi", 0)],
            lines: vec!["Hi".to_owned(), "  ".to_owned()],
        };
        assert_eq!(all_text(&text), "Hi");
    }

    #[test]
    fn line_chars_cover_the_line_box() {
        let mut words = Vec::new();
        push_line_chars(&mut words, 0, "截图工具", 100, 50, 96, 24);
        assert_eq!(words.len(), 4);
        // 覆盖整行、且首尾对齐检测框。
        assert_eq!(words[0].x, 100);
        let last = words.last().unwrap();
        assert_eq!(last.x + last.width, 196);
        assert!(words.windows(2).all(|pair| pair[0].x + pair[0].width == pair[1].x));
    }

    #[test]
    fn ascii_chars_are_narrower_than_cjk() {
        let mut words = Vec::new();
        push_line_chars(&mut words, 0, "a中", 0, 0, 100, 20);
        assert!(words[0].width < words[1].width);
    }

    /// 端到端探针：用 GDI 画一行文字再走本地 OCR。
    /// 模型加载 + 推理要一两秒，默认不跑；用 `--ignored` 执行。
    #[test]
    #[ignore]
    fn recognize_rendered_text() {
        use windows_sys::Win32::Graphics::Gdi::{
            CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, FillRect, SelectObject,
            SetBkMode, SetTextColor, DT_LEFT, DT_SINGLELINE, DT_VCENTER, TRANSPARENT,
        };

        let surface = unsafe { crate::screenshot::dib::Surface::new(560, 96) }.expect("surface");
        unsafe {
            let white = CreateSolidBrush(0x00FF_FFFF);
            let rect = windows_sys::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: 560,
                bottom: 96,
            };
            FillRect(surface.dc(), &rect, white);
            DeleteObject(white);

            let face: Vec<u16> = "Microsoft YaHei UI\0".encode_utf16().collect();
            let font = CreateFontW(
                -40, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, face.as_ptr(),
            );
            let old = SelectObject(surface.dc(), font);
            SetBkMode(surface.dc(), TRANSPARENT as i32);
            SetTextColor(surface.dc(), 0x0000_0000);
            let text: Vec<u16> = "截图工具 Hello 123 中文测试\0".encode_utf16().collect();
            let mut area = rect;
            DrawTextW(
                surface.dc(),
                text.as_ptr(),
                text.len() as i32 - 1,
                &mut area,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            SelectObject(surface.dc(), old);
            DeleteObject(font);
        }

        let text = recognize(surface.pixels(), surface.width, surface.height).expect("ocr");
        let all = all_text(&text);
        eprintln!("ocr text: {all}");
        assert!(all.contains("截图工具"), "unexpected ocr text: {all}");
        assert!(all.contains("Hello"), "unexpected ocr text: {all}");
    }

    /// 用一张真实截图跑两遍，打印识别结果与耗时（调参用）。
    /// `LWT_OCR_IMAGE=<png 路径> cargo test --lib screenshot::ocr::tests::recognize_image_path -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn recognize_image_path() {
        fn private_mb() -> f64 {
            use windows_sys::Win32::System::ProcessStatus::{
                GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
            };
            use windows_sys::Win32::System::Threading::GetCurrentProcess;
            let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let ok = unsafe {
                GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
            };
            if ok == 0 {
                return 0.0;
            }
            counters.WorkingSetSize as f64 / (1024.0 * 1024.0)
        }

        let Ok(path) = std::env::var("LWT_OCR_IMAGE") else {
            eprintln!("LWT_OCR_IMAGE not set, skip");
            return;
        };
        let bytes = std::fs::read(&path).expect("read image");
        let image = image::load_from_memory(&bytes).expect("decode image");
        let rgba = image.to_rgba8();
        let (width, height) = (rgba.width() as i32, rgba.height() as i32);
        let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
        for (source, target) in rgba.as_raw().chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
            target[0] = source[2];
            target[1] = source[1];
            target[2] = source[0];
            target[3] = 255;
        }
        eprintln!("mem.idle {:.1}MB", private_mb());
        for round in 0..3 {
            let started = std::time::Instant::now();
            let text = recognize(&bgra, width, height).expect("ocr run");
            eprintln!(
                "round {round}: {:?} for {width}x{height}, mem.during {:.1}MB",
                started.elapsed(),
                private_mb()
            );
            if round == 0 {
                eprintln!("--- lines ---\n{}", all_text(&text));
            }
            // 让工作线程把引擎丢弃后再量一次。
            std::thread::sleep(std::time::Duration::from_millis(600));
            eprintln!("round {round}: mem.after_release {:.1}MB", private_mb());
        }
    }
}
