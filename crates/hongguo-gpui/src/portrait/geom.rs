//! 竖屏窗口几何的纯函数（无副作用、无 crate 依赖——集成测试按源文件引入，
//! 见 `tests/portrait.rs`）。
//!
//! 逐行对齐 Tauri `window_cmd::fit_portrait` 与前端 `use-portrait-window`：
//! 宽高比带手机形地板、普通档高度最多吃工作区高的 70%、x 居中于**当前
//! 窗口**（铺满档居中于工作区）、宽溢出工作区时反推高度。
//!

/// 宽高比 ≥0.9（近方形/横屏）不算竖屏比：竖屏只对竖版内容有意义。
const PORTRAIT_ASPECT_MAX: f64 = 0.9;
/// 手机形比例地板（w/h）：9:16 内容在手机形窗口里上下留边，与真机观看
/// 一致；比手机更长的内容（9:21）仍按自身比例包住（Tauri 同值 9/19.5）。
const PHONE_LIKE_ASPECT: f64 = 9.0 / 19.5;
/// 竖屏下的最小宽度（Tauri PORTRAIT_MIN.0 同参；高度侧只作窗口约束用，
/// 不参与 fit 计算）。
pub(crate) const PORTRAIT_MIN_W: f64 = 360.0;
/// 普通档高度上限系数（Tauri PORTRAIT_DEFAULT_HEIGHT_RATIO 同参）。
const PORTRAIT_HEIGHT_RATIO: f64 = 0.7;

/// 视频尺寸（宽, 高）→ 竖屏窗口宽高比（Tauri `portraitAspectOf` 同款）。
pub(crate) fn aspect_of_video(width: usize, height: usize) -> f64 {
    let ar = if width > 0 && height > 0 {
        width as f64 / height as f64
    } else {
        0.0
    };
    if ar > 0.0 && ar < PORTRAIT_ASPECT_MAX {
        ar.min(PHONE_LIKE_ASPECT)
    } else {
        PHONE_LIKE_ASPECT
    }
}

/// 一次 fit 需要的输入（逻辑像素）。两端落窗路径共用。
#[derive(Debug, Clone, Copy)]
pub(crate) struct FitInput {
    /// 视频宽高比（w/h，已过 `aspect_of_video`）。
    pub(crate) aspect: f64,
    /// 铺满档：撑满工作区高、居中于工作区；普通档：保持当前高/位置。
    pub(crate) full: bool,
    /// 当前窗口内容矩形（x, y, w, h）。
    pub(crate) cur: (f64, f64, f64, f64),
    /// 工作区（left, top, right, bottom）。
    pub(crate) work: (f64, f64, f64, f64),
}

/// 算出竖屏窗口矩形（x, y, w, h）。两端落窗路径共用（macOS 在调用前
/// 把 AppKit 坐标翻成 top-left 语义）。
pub(crate) fn fit_rect(input: FitInput) -> (f64, f64, f64, f64) {
    let FitInput {
        aspect,
        full,
        cur: (cur_x, cur_y, cur_w, cur_h),
        work: (left, top, right, bottom),
    } = input;
    let wa_w = right - left;
    let wa_h = bottom - top;
    let mut h = if full {
        wa_h
    } else {
        cur_h.min((wa_h * PORTRAIT_HEIGHT_RATIO).round())
    };
    let mut w = (h * aspect).round();
    if w > wa_w {
        w = wa_w;
        h = (w / aspect).round();
    }
    w = w.max(PORTRAIT_MIN_W);
    // 手动 min/max 而非 clamp：极小工作区下 min>max 时 clamp 会 panic
    let x = if full {
        left + ((wa_w - w) / 2.0).round().max(0.0)
    } else {
        (cur_x + ((cur_w - w) / 2.0).round())
            .max(left)
            .min(right - w)
    };
    let y = if full { top } else { cur_y };
    (x, y, w, h)
}
