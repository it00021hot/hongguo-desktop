//! 元测试：gpui 图片通道序不变量（背景见 `src/image_upload.rs`）——全壳只有
//! `image_upload` 直接构造 `RenderImage` 且确实过了 BGRA 交换。喂 RGBA 不报
//! 错，只会 R/B 整体互换（蓝底变橙底），渲染路径没有视觉测试，错了只能靠
//! 人眼发现。只读源码文本。

use std::path::{Path, PathBuf};

/// 造 `RenderImage` 的允许位置（相对 crate 根）。
const ALLOWED: &str = "src/image_upload.rs";

/// 交换函数名（定义在 `hongguo-media::bgra`，此处只认名字，不引依赖）。
const SWAP_FN: &str = "swap_red_blue_in_place";

/// 去掉行注释尾巴后的代码文本（`//` 之后一律不看，含 `///` 与 `//!`）。
fn code_only(text: &str) -> String {
    text.lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_sources(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            out.push((path, text));
        }
    }
}

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(
        root.is_dir(),
        "源码目录不存在（布局变了就把这条守卫一起改）：{}",
        root.display()
    );
    let mut raw = Vec::new();
    collect_sources(&root, &mut raw);
    assert!(!raw.is_empty(), "没扫到任何源码，守卫等于没跑");
    raw.into_iter()
        .map(|(path, text)| {
            let rel = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            (rel, code_only(&text))
        })
        .collect()
}

/// 唯一构造点：别处（home / rank / 以后的新视图）不许直接 `RenderImage::new`。
#[test]
fn render_image_is_constructed_only_in_image_upload() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .filter(|(rel, code)| code.contains("RenderImage::new(") && rel != ALLOWED)
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "这些文件绕过了 image_upload::render_image 直接造 RenderImage（会漏掉 RGBA→BGRA，画面上 R/B 整体互换）：{offenders:?}",
    );
}

/// 构造点本身要过交换：唯一造 `RenderImage` 的函数必须调用 BGRA 交换。
#[test]
fn image_upload_hands_gpui_bgra() {
    let (_, code) = sources()
        .into_iter()
        .find(|(rel, _)| rel == ALLOWED)
        .expect("image_upload.rs 不见了：造帧入口被挪走了吗");
    assert!(
        code.contains(SWAP_FN),
        "{ALLOWED} 里找不到 {SWAP_FN}：交出去的还是 RGBA，画面上 R/B 会整体互换",
    );
    assert!(
        code.contains("RenderImage::new("),
        "{ALLOWED} 里没有 RenderImage::new：守卫的假设失效了，请一起更新",
    );
}
