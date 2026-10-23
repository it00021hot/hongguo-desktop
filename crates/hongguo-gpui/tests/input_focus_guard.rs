//! 守卫：输入框里的按键不许被舞台快捷键劫持；表情面板不许把点击漏到画面层。
//!
//! 本壳是 bin-only（`[[bin]] test = false`——跑起来会开窗口的那种，不能进
//! 测试集合），源码内放不了单测，所以这条守卫做成**集成测试**：它只读源码
//! 文本，不开窗口、不联网（同 `render_image_guard.rs` 的做法）。
//!
//! 盯四件事（Tauri 侧早有同款实现，GPUI 端口漏了，用户实测复现）：
////! 1. 舞台的 `on_key_down` 必须有「焦点在输入框就让路」的守卫——空格默认
//!    是播放/暂停键，输入框里按空格会把视频暂停；←→/↑↓ 会快进/切集。
//! 2. 舞台的「用户在场」副作用（`wake_on_input`）必须排在守卫**之前**：
//!    输入框吃掉 Delete 这类按键时事件根本到不了舞台，只靠舞台那条路，
//!    控件会在输入框里打字时一直藏着（用户实测「按删除会隐藏播放控件」）。
//! 3. 弹幕/评论输入框的 `Change` 事件要唤醒控件（输入即用户在场）。
//! 4. 弹幕表情面板浮在**控制条上方**（bottom 38px + 176px 高），控制条自己
//!    的 bounds 罩不到它 → 控制条那层的拦截拦不住，点击会一路落到舞台
//!    画面层（点画面 = 暂停）。

use std::path::{Path, PathBuf};

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

/// 全部源码（相对 crate 根的路径 → 去注释后的代码）。
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

fn source_of(rel: &str) -> String {
    sources()
        .into_iter()
        .find(|(path, _)| path == rel)
        .map(|(_, code)| code)
        .unwrap_or_else(|| panic!("找不到源码文件：{rel}"))
}

/// 舞台的键盘处理块（`on_key_down` 到该链式调用的收尾）。
fn stage_key_handler() -> String {
    let code = source_of("src/home/stage/render.rs");
    let start = code
        .find(".on_key_down(")
        .expect("舞台没有 on_key_down 了：快捷键入口被挪走了吗");
    let end = code[start..]
        .find("),\n            .child(")
        .map(|offset| start + offset)
        .unwrap_or_else(|| {
            code[start..]
                .find(".child(cover)")
                .map(|offset| start + offset)
                .expect("舞台的键盘处理块收尾找不到了")
        });
    code[start..end].to_string()
}

/// 1 + 2：输入框焦点时舞台让路快捷键，但「用户在场」副作用先做。
#[test]
fn stage_shortcuts_yield_to_focused_input() {
    let handler = stage_key_handler();
    assert!(
        handler.contains("focused_input"),
        "舞台的 on_key_down 没有「焦点在输入框就让路」的守卫：输入框里按空格会暂停、按方向键会切集",
    );
    let wake = handler
        .find("wake_on_input")
        .expect("舞台的 on_key_down 不再做「用户在场」副作用了");
    let guard = handler
        .find("focused_input")
        .expect("舞台的 on_key_down 没有输入框守卫");
    assert!(
        wake < guard,
        "「用户在场」副作用必须排在输入框守卫**之前**：输入框吃掉 Delete 这类按键时事件到不了舞台，控件会一直藏着",
    );
}

/// 3：输入框内容变化 = 用户在打字 → 唤醒控件。
#[test]
fn typing_in_inputs_wakes_the_controls() {
    let code = source_of("src/home/events.rs");
    assert!(
        code.contains("InputEvent::Change"),
        "输入框事件订阅里没有 Change 分支：在输入框里打字不会唤醒控件",
    );
    assert!(
        code.contains("wake_controls"),
        "输入框的 Change 事件没有唤醒控件：输入框里打字时控件会一直藏着",
    );
}

/// 4：弹幕表情面板自己拦鼠标（它浮在控制条上方，控制条罩不到）。
#[test]
fn danmaku_emoji_picker_swallows_mouse() {
    let code = source_of("src/home/controls/danmaku_bar.rs");
    let picker = code
        .find("danmaku_emoji_open.then")
        .map(|at| &code[at..])
        .expect("弹幕表情面板不见了");
    assert!(
        picker.contains("stop_propagation"),
        "弹幕表情面板没有拦鼠标：面板浮在控制条上方，点表情会落到舞台画面层（点画面 = 暂停）",
    );
}
