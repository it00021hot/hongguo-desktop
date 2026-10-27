//! 元测试：可点击元素的键盘可达闸门。
//!
//! 本壳是 bin-only（`[[bin]] test = false`——跑起来会开窗口的那种，不能进
//! 测试集合），源码内放不了单测，所以这条守卫做成**集成测试**：它只读源码
//! 文本，不开窗口、不联网。
//!
//! 盯一件事：**全壳不许出现「手搓可点击 div」**——也就是 `div()` /
//! `h_flex()` / `v_flex()` 起头、链上带 `.on_click()` 的写法。这种元素鼠标
//! 可点、键盘永远够不到（没有焦点句柄、没有 role、没有 Enter/Space 激活），
//! 屏幕阅读器与纯键盘用户直接被挡在门外。
//!
//! 正确写法是换 gpui-kit 的现成件：
//! - `gpui_kit::base::Button`——无样式按钮，自带焦点遍历、Enter/Space 激活、
//!   role 与无障碍名称，视觉全由应用的 styled 链决定（像素不变的前提）；
//! - `gpui_kit::base::Checkbox`——勾选语义（列表页的选择圆钮）；
//! - `gpui_kit::component::button::Button`——带变体的成品按钮；
//! - 本地 helper（如 `pill()`、`pick_dot()`）——内部返回的也是上面两种。
//!
//! Tauri 壳（对齐基准）里这些列表卡本来就是 `role="button" tabIndex={0}`，
//! GPUI 侧的裸 `div().on_click(...)` 是与基线不一致的偏差。
//!
//! 为什么值得单独守：这类退化**编译不报错、测试不失败、截图也看不出来**
//! （鼠标点得动），只有真用 Tab 走一遍才发现元素跳过去了——手测一次就忘，
//! 下次加卡片又手搓一个。
//!
//! 唯一的例外是**指针便利层**：鼠标上多一个可点区域、但它的动作在键盘上
//! 已有等价入口（控制栏的播放/暂停钮、舞台的空格快捷键）。这类元素不该进
//! 无障碍树（否则屏幕阅读器会念出一个覆盖整块画面的巨按钮），所以留在源码
//! 里，用 `// keyboard-skip: <理由>` 显式标注——闸门认这个标记，但**理由不能
//! 为空**：豁免要写在代码里、写在人眼前，不是藏在测试里的名单。

use std::path::{Path, PathBuf};

/// 纳入闸门的目录（相对 crate 根）。
const SCANNED: &[&str] = &["src"];

/// 指针便利层的显式豁免标记（后缀必须带理由）。
const SKIP_MARKER: &str = "keyboard-skip:";

/// 链头类别。
#[derive(PartialEq, Debug)]
enum Head {
    /// div 系（手搓可点击的嫌疑形状）。
    Div,
    /// 组件库控件或返回组件的本地 helper。
    Component,
    /// 不是链头（方法调用行、参数行、语句收尾等）。
    None,
}

/// 一行去掉行注释与字符串字面量内容后的代码（保留结构与缩进）。
///
/// 字符串字面量一并抹掉：`.on_click(` 出现在文案里不算可点击元素。
fn code_part(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                out.push('"');
            } else {
                out.push(' ');
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push('"');
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            break;
        }
        out.push(c);
    }
    out
}

/// 判定一行是不是一条链的链头，以及链头是什么。
///
/// 判据看**行首**：builder 链都从构造调用起行，`.` 开头的是方法续行、
/// 参数行都不是链头。
fn chain_head_kind(code: &str) -> Head {
    let trimmed = code.trim_start();
    // `let x = div()` 的等号前缀
    let expr = match trimmed.find('=') {
        Some(at) if trimmed[..at].trim_start().starts_with("let ") => {
            trimmed[at + 1..].trim_start()
        }
        _ => trimmed,
    };
    if expr.starts_with('.') {
        return Head::None;
    }
    let Some(at) = expr.find('(') else {
        return Head::None;
    };
    let callee = expr[..at].trim();
    let last = callee.rsplit("::").next().unwrap_or(callee).trim();
    if last.is_empty() || !last.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Head::None;
    }
    match last {
        "div" | "h_flex" | "v_flex" => Head::Div,
        // 小写起头的调用：小写函数（本地 helper 返回组件）或 `X::new(` 的
        // `new` 段，都算组件链头。
        _ if last.starts_with(|c: char| c.is_lowercase() || c == '_') => Head::Component,
        _ => Head::None,
    }
}

/// 行首缩进宽度（制表符按 1 计——本仓源码统一空格缩进）。
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// 这行是不是纯注释行。
fn is_comment_only(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// 这行是不是多行参数的收尾行（`}` / `})` / `),` / `];` …）。
///
/// 收尾行必须**跳过而不是截断**回溯：`.bg(if active { … })` 这种多行参数
/// 的收尾与链的方法续行同缩进，截断了就找不到链头（真 bug 会被漏掉）。
fn is_bracket_closer(code: &str) -> bool {
    let t = code.trim();
    t.starts_with('}') || t.starts_with(')') || t.starts_with(']')
}

/// 往上找链头：返回（行号, 类别）。
///
/// 回溯按**缩进**分层：链头比它的方法续行浅一层，而嵌在参数里的子链比方法
/// 续行深一层，所以「缩进比本行更深」的行一律跳过（那是嵌套子链，不是本链
/// 的头）。这样链里 `.child(...)` 与 `.on_click(...)` 的先后顺序不影响判定。
///
/// 浅一层的行分三类：
/// - 方法续行（以 `.` 开头）→ 继续往上找；
/// - 多行参数的收尾行 → 继续往上找；
/// - 其余 → 这就是链头。
fn chain_head_above(lines: &[&str], start: usize, base_indent: usize) -> Option<(usize, Head)> {
    for up in (0..start).rev() {
        let candidate = lines[up];
        let code = code_part(candidate);
        if code.trim().is_empty() {
            // 纯注释行跳过（注释不是链的一部分），真空行才截断链
            if is_comment_only(candidate) {
                continue;
            }
            break;
        }
        if indent_of(candidate) > base_indent {
            continue;
        }
        let trimmed = code.trim();
        if trimmed.starts_with('.') || is_bracket_closer(trimmed) {
            continue;
        }
        return Some((up, chain_head_kind(&code)));
    }
    None
}

/// 同行接收者是不是闭包参数（`.when(cond, |s| s.on_click(…)`）。
fn prefix_receiver_is_closure_param(prefix: &str) -> bool {
    let p = prefix.trim_end();
    let Some(bar) = p.rfind('|') else {
        return false;
    };
    let after = p[bar + 1..].trim();
    if after.is_empty() || !after.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    // `|` 之前必须是同一个标识符（`|s| s`）
    let before = p[..bar].trim_end();
    let Some(bar2) = before.rfind('|') else {
        return false;
    };
    before[bar2 + 1..].trim() == after
}

/// 裸标识符接收者（`s.on_click(…)`）：取标识符名。
fn bare_ident(prefix: &str) -> Option<&str> {
    let p = prefix.trim();
    if p.is_empty() || !p.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(p)
}

/// 这行有没有把 `ident` 绑成闭包参数（`|ident|` / `|ident,` / `|mut ident`）。
fn binds_closure_param(code: &str, ident: &str) -> bool {
    let mut rest = code;
    while let Some(at) = rest.find('|') {
        let after = &rest[at + 1..];
        let name: String = after
            .trim_start()
            .strip_prefix("mut ")
            .unwrap_or(after.trim_start())
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name == ident {
            return true;
        }
        rest = &rest[at + 1..];
    }
    false
}

/// 往上找：这个标识符是不是某个闭包的参数（`.when(cond, |s| { s.on_click(…) })`）。
fn is_closure_param_above(lines: &[&str], start: usize, ident: &str) -> bool {
    let low = start.saturating_sub(60);
    for up in (low..start).rev() {
        let code = code_part(lines[up]);
        if code.trim().is_empty() {
            continue;
        }
        if binds_closure_param(&code, ident) {
            return true;
        }
    }
    false
}

/// 链头（或它紧挨着的上方注释块）带显式豁免标记。
fn has_skip_marker(lines: &[&str], head: usize) -> bool {
    let marked = |line: &str| {
        line.contains(SKIP_MARKER)
            && line
                .split_once(SKIP_MARKER)
                .map(|(_, reason)| !reason.trim().is_empty())
                .unwrap_or(false)
    };
    if marked(lines[head]) {
        return true;
    }
    // 链头上方**连续**的注释块里出现标记即放行：豁免要写在人眼前（代码里），
    // 不是藏在测试里的名单；连续性保证标记不会飘到别的元素头上。
    for up in (0..head).rev() {
        if !is_comment_only(lines[up]) {
            break;
        }
        if marked(lines[up]) {
            return true;
        }
    }
    false
}

/// 找出该文件里所有「手搓可点击 div」的链头行号（1 起）。
///
/// 判据：某个 `.on_click(` 所在行，向上回溯找链头——先遇见组件链头即合规，
/// 先遇见 div 链头即违规。
///
/// 同行的接收者先看：`button.on_click(` 这种挂在组件变量上的调用不回溯；
/// 裸标识符 `s.on_click(` 要看它是不是闭包参数（是就当链的延续继续回溯，
/// 不是就当组件变量放过）。
fn hand_rolled_click_lines(code: &str) -> Vec<usize> {
    let raw: Vec<&str> = code.lines().collect();
    let mut offenders = Vec::new();
    for (index, line) in raw.iter().enumerate() {
        let code = code_part(line);
        let Some(at) = code.find(".on_click(") else {
            continue;
        };
        let prefix = code[..at].trim_end();
        let head = if prefix.is_empty() || prefix_receiver_is_closure_param(prefix) {
            // 方法续行 / 闭包参数当接收者 → 往上找链头
            chain_head_above(&raw, index, indent_of(raw[index]))
        } else {
            match chain_head_kind(prefix) {
                Head::Div => Some((index, Head::Div)),
                Head::Component => None,
                Head::None => match bare_ident(prefix) {
                    Some(ident) if is_closure_param_above(&raw, index, ident) => {
                        chain_head_above(&raw, index, indent_of(raw[index]))
                    }
                    _ => None,
                },
            }
        };
        let Some((head_line, kind)) = head else {
            continue;
        };
        if kind != Head::Div {
            continue;
        }
        if has_skip_marker(&raw, head_line) {
            continue;
        }
        offenders.push(head_line + 1);
    }
    offenders
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut raw = Vec::new();
    for dir in SCANNED {
        let path = root.join(dir);
        assert!(
            path.is_dir(),
            "待扫目录不存在（布局变了就把这条守卫一起改）：{}",
            path.display()
        );
        collect_sources(&path, &mut raw);
    }
    assert!(!raw.is_empty(), "没扫到任何源码，守卫等于没跑");
    raw.into_iter()
        .map(|(path, code)| {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            (rel, code)
        })
        .collect()
}

/// 全壳不许有手搓可点击 div：换成 `base::Button` 才能被键盘走到。
#[test]
fn clickable_elements_are_keyboard_reachable() {
    let offenders: Vec<String> = sources()
        .into_iter()
        .flat_map(|(rel, code)| {
            hand_rolled_click_lines(&code)
                .into_iter()
                .map(move |line| format!("{rel}:{line}"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "这些位置是手搓可点击 div（鼠标可点、键盘够不到，也没有 role 与无障碍名称）：\
         换成 gpui_kit::base::Button（自带焦点遍历 + Enter/Space 激活，视觉仍由 styled 链控制）；\
         确实只是指针便利层（键盘已有等价入口）的，在链头写 `// keyboard-skip: <理由>`：{offenders:?}",
    );
}

// —— 检测器自身的单元测试（把判据钉住，免得以后悄悄退化）——

/// 命中的行内容（便于断言「命中的是哪一行」）。
fn offenders_text(code: &str) -> Vec<String> {
    hand_rolled_click_lines(code)
        .into_iter()
        .map(|line| {
            code.lines()
                .nth(line - 1)
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .collect()
}

#[test]
fn detector_flags_hand_rolled_div_chain() {
    let code = "\
fn demo() {
    div()
        .id(\"x\")
        .cursor_pointer()
        .on_click(|_, _, _| {})
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

#[test]
fn detector_accepts_base_button_chain() {
    let code = "\
fn demo() {
    BaseButton::new(\"x\")
        .id(\"x\")
        .cursor_pointer()
        .on_click(|_, _, _| {})
}
";
    assert!(offenders_text(code).is_empty());
}

/// 链序无关：`.child(...)` 在 `.on_click(...)` 之前也不该误报。
#[test]
fn detector_accepts_base_button_with_nested_child_before_on_click() {
    let code = "\
fn demo() {
    BaseButton::new(\"x\")
        .child(
            div()
                .text_sm()
                .child(\"hi\"),
        )
        .on_click(|_, _, _| {})
}
";
    assert!(offenders_text(code).is_empty());
}

/// 链序无关：`.on_click(...)` 在 `.child(...)` 之前也不该误报。
#[test]
fn detector_accepts_base_button_with_on_click_before_nested_child() {
    let code = "\
fn demo() {
    BaseButton::new(\"x\")
        .on_click(|_, _, _| {})
        .child(
            div()
                .text_sm(),
        )
}
";
    assert!(offenders_text(code).is_empty());
}

/// 本地 helper 返回组件（`pill(...)`）不算手搓。
#[test]
fn detector_accepts_local_component_helper() {
    let code = "\
fn demo() {
    h_flex()
        .child(
            pill(id, label, active)
                .on_click(|_, _, _| {})
        )
}
";
    assert!(offenders_text(code).is_empty());
}

/// 挂在组件变量上的方法调用不算手搓。
#[test]
fn detector_accepts_on_click_on_component_variable() {
    let code = "\
fn demo() {
    h_flex()
        .child(button.on_click(|_, _, _| {}))
}
";
    assert!(offenders_text(code).is_empty());
}

/// 嵌套参数里的裸 div 仍然要抓（它自己就是手搓可点击）。
#[test]
fn detector_flags_hand_rolled_div_nested_in_argument() {
    let code = "\
fn demo() {
    h_flex()
        .child(
            div()
                .cursor_pointer()
                .on_click(|_, _, _| {}),
        )
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

/// 多行参数的收尾行不能截断回溯（真 bug 曾因此被漏掉）。
#[test]
fn detector_flags_hand_rolled_div_after_multiline_argument() {
    let code = "\
fn demo() {
    div()
        .bg(if active {
            color_a
        } else {
            color_b
        })
        .on_click(|_, _, _| {})
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

/// 注释行不能截断回溯。
#[test]
fn detector_flags_hand_rolled_div_after_comment_line() {
    let code = "\
fn demo() {
    div()
        // 一条注释
        .id(\"x\")
        .on_click(|_, _, _| {})
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

/// 闭包参数当接收者（单行）：`.when(cond, |s| s.on_click(…)`）仍算手搓。
#[test]
fn detector_flags_hand_rolled_div_with_closure_receiver() {
    let code = "\
fn demo() {
    div()
        .when(active, |s| s.on_click(|_, _, _| {}))
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

/// 闭包参数当接收者（多行）：`.when(cond, |s| { s.on_click(…) })` 仍算手搓。
#[test]
fn detector_flags_hand_rolled_div_with_multiline_closure_receiver() {
    let code = "\
fn demo() {
    div()
        .when(active, |s| {
            s.on_click(|_, _, _| {})
        })
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}

/// 字符串里的 `.on_click(` 不算可点击元素。
#[test]
fn detector_ignores_on_click_inside_string_literal() {
    let code = "\
fn demo() {
    div()
        .id(\"a.on_click(b)\")
        .child(\"hi\")
}
";
    assert!(offenders_text(code).is_empty());
}

/// 显式豁免标记放行指针便利层。
#[test]
fn detector_accepts_explicit_skip_marker() {
    let code = "\
fn demo() {
    // keyboard-skip: 点画面暂停是鼠标便利层，键盘走控制栏播放/暂停钮与空格
    div()
        .id(\"stage-video\")
        .on_click(|_, _, _| {})
}
";
    assert!(offenders_text(code).is_empty());
}

/// 豁免标记必须带理由（空理由不算豁免）。
#[test]
fn detector_rejects_skip_marker_without_reason() {
    let code = "\
fn demo() {
    // keyboard-skip:
    div()
        .id(\"stage-video\")
        .on_click(|_, _, _| {})
}
";
    assert_eq!(offenders_text(code), vec!["div()"]);
}
