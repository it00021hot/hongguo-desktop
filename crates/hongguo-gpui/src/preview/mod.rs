//! 离屏 UI 预览流水线：无头窗口渲染**真实壳代码** + 假数据快照 → PNG。
//!
//! 窗口从头到尾不显示（`show: false`），画面不经过窗口服务器，直接从
//! 渲染目标读回——验收各功能菜单的视觉时不依赖屏幕、不依赖系统授权。
//! 与 `screenshot.rs`（对**运行中**窗口进程内抓帧）互补：那边验真实数据
//! 与播放链路，这边验布局/主题/面板的全矩阵分支，且完全离线确定。
//!
//! 机制（gpui 自带测试基建 + 本仓库的编排）：
//! 1. `--render-preview <目录>` 进预览模式（`screenshot` feature 编入管线，
//!    它已连带开启 gpui 的 test-support 与 Metal 无头渲染器）；
//! 2. 数据根指到 `hongguo-gpui-preview-<pid>` 临时目录（与真实数据平级、
//!    互不接触），跑完即删；
//! 3. 每个场景一个 [`gpui_kit::HeadlessAppContext`]（独立主题/字号全局），
//!    `gpui_kit::open_window` 开 `show:false` 窗口挂真实 `Shell`，种子注入
//!    （`home/preview_seed.rs` + `preview/fixtures.rs`）后 `draw` 两帧，
//!    `capture_screenshot` 存 `<场景名>.png`。
//!
//! 已知边界（预览图的预期形态，不是 bug）：
//! - 无播放引擎：进度条 00:00/00:00、无已播填充；弹幕无在飞条目；
//! - 封面/头像为合成渐变图，评论头像走首字占位；
//! - gpui 的无头渲染器只有 Metal（macOS）/ wgpu（Linux）实现，Windows 上
//!   `current_headless_renderer` 返回 `None`，截图会报错退出。
//!
//! 用法：`make gpui-preview`（= `cargo run -p hongguo-gpui --features
//! screenshot -- --render-preview <目录>`）。

// 场景矩阵与假数据只被管线消费：默认构建（无 screenshot）连模块都不编入，
// 免得 pub(crate) 未用触发 dead_code；覆盖率测试经 #[path] 自行引入 scenes。
#[cfg(feature = "screenshot")]
pub(crate) mod fixtures;
#[cfg(feature = "screenshot")]
mod scenes;

use std::sync::atomic::{AtomicBool, Ordering};

static PREVIEW_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 预览进程标记：`HomeView`/`Shell` 据此跳过启动网络加载（预览只吃
/// fixtures 的假快照，不发任何请求）。
pub(crate) fn is_active() -> bool {
    PREVIEW_ACTIVE.load(Ordering::Relaxed)
}

/// 扫描命令行的 `--render-preview <目录>`；命中即进预览模式，缺目录参数
/// 直接报错退出（开发工具，宁可显性失败）。
pub(crate) fn requested_dir() -> Option<std::path::PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--render-preview" {
            return Some(match args.next() {
                Some(dir) => dir.into(),
                None => {
                    eprintln!("--render-preview 需要一个输出目录参数");
                    std::process::exit(2);
                }
            });
        }
    }
    None
}

/// 跑完整场景矩阵。任何一张渲染失败即中止（后续产物已落盘保留）。
#[cfg(feature = "screenshot")]
pub(crate) fn run(output: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output)?;
    // 数据根最先声明（与 main 的交互壳同一纪律）：预览专用目录与真实
    // 数据平级，账号/设备档案/数据库完全不碰，跑完删除。
    hongguo_store::paths::set_shell_data_root(&format!(
        "hongguo-gpui-preview-{}",
        std::process::id()
    ));
    PREVIEW_ACTIVE.store(true, Ordering::Relaxed);
    let state = crate::build_app_state();
    for case in scenes::cases() {
        if let Err(e) = render_case(&case, &state, output) {
            anyhow::bail!("场景 {} 渲染失败: {e:#}", case.name);
        }
        // mini 场景翻的是进程级全局（crate::mini 的静态），跑完复位，
        // 不让后续场景继承小屏形态。
        crate::mini::set_active(false);
    }
    // 出图之后跑交互探针：点击是出图验不了的另一半
    interaction_probe(&state)?;
    drop(state);
    let data_dir = hongguo_store::paths::data_dir();
    if let Err(e) = std::fs::remove_dir_all(&data_dir) {
        log::warn!("[Preview] 临时数据目录清理失败（不影响产物）: {e}");
    }
    log::info!("[Preview] 全部场景完成 → {}", output.display());
    Ok(())
}

/// 渲染单个场景：无头 App + 隐藏窗口 + 种子注入 + 两帧绘制 + 截图落盘。
#[cfg(feature = "screenshot")]
fn render_case(
    case: &scenes::PreviewCase,
    state: &std::sync::Arc<hongguo_service::app_state::AppStateInner>,
    output: &std::path::Path,
) -> anyhow::Result<()> {
    // open_window 的 build 闭包里要 cx.new 造根视图（AppContext trait 方法）
    use gpui_kit::AppContext as _;
    let mut cx = gpui_kit::HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        std::sync::Arc::new(gpui_kit::assets::AllAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tokens::apply(case.mode, cx);
        if case.font_scale != 1. {
            gpui_kit::component::theme::Theme::update(cx, |theme| {
                theme.font_size *= case.font_scale
            });
        }
    });
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                    origin: Default::default(),
                    size: gpui_kit::size(gpui_kit::px(case.width), gpui_kit::px(case.height)),
                })),
                focus: false,
                show: false,
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| crate::shell::Shell::new(state.clone(), window, cx)),
        )
    })?;
    // 首帧：种子注入后立即布局一次，面板/叠层的位置计算才有基准。
    // Result 层次：Entity::update（实体已释放/场景键名写错）一层，闭包
    // 把它带出后 update_window（窗口已关）再包一层，故内 `?` 外 `??`。
    cx.update_window(handle, |_, window, cx| -> anyhow::Result<()> {
        shell.update(cx, |shell, cx| {
            shell.debug_preview_seed(case.scene, window, cx)
        })?;
        let _ = window.draw(cx);
        Ok(())
    })??;
    cx.run_until_parked();
    cx.advance_clock(std::time::Duration::from_millis(200));
    // 二帧：run_until_parked 期间落定的 notify 收进这一帧再取图
    cx.update_window(handle, |_, window, cx| {
        let _ = window.draw(cx);
    })?;
    let image = cx.capture_screenshot(handle)?;
    let path = output.join(format!("{}.png", case.name));
    image.save(&path)?;
    log::info!(
        "[Preview] {} → {}（{}×{}）",
        case.name,
        path.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

/// 交互探针：无头 Shell 上派**进程内**鼠标事件点真实的舞台 tab 按钮，
/// 断言切源生效。与出图矩阵同一套构建与种子；不猜按钮几何——沿 tab 条
/// 所在高度横扫点击，源离开「推荐」即证明 on_click 可达。等价真实点击
/// 路径（move → down → up 全走 gpui 事件分发），`make gpui-preview` 每次
/// 必跑，tab 点击失效会让流水线显性失败。
#[cfg(feature = "screenshot")]
fn interaction_probe(
    state: &std::sync::Arc<hongguo_service::app_state::AppStateInner>,
) -> anyhow::Result<()> {
    use gpui_kit::AppContext as _;
    use gpui_kit::{MouseButton, PlatformInput, Point, px};

    let mut cx = gpui_kit::HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        std::sync::Arc::new(gpui_kit::assets::AllAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tokens::apply(gpui_kit::component::theme::ThemeMode::Dark, cx);
    });
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                    origin: Default::default(),
                    size: gpui_kit::size(gpui_kit::px(1024.), gpui_kit::px(680.)),
                })),
                focus: false,
                show: false,
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| crate::shell::Shell::new(state.clone(), window, cx)),
        )
    })?;
    cx.update_window(handle, |_, window, cx| -> anyhow::Result<()> {
        shell.update(cx, |shell, cx| shell.debug_preview_seed("feed", window, cx))?;
        let _ = window.draw(cx);
        Ok(())
    })??;

    // tab 条带：舞台顶部 top_3(12px) 起的一小条；横跨内容区全宽（x 取
    // 240..1010，避开侧栏 224），多档高度每 8px 点一次，直到源离开
    // 「推荐」。点击推荐本身 = 换一批（源不变），漫剧是第一个会翻源的点。
    let switched_at = cx.update_window(handle, |_, window, cx| -> Option<(f32, f32)> {
        for y in [54.0, 60.0, 66.0, 72.0, 78.0] {
            for x in (240..1010).step_by(8) {
                let pos = Point::new(px(x as f32), px(y));
                for input in [
                    PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                        position: pos,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }),
                    PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                        button: MouseButton::Left,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                        button: MouseButton::Left,
                        position: pos,
                        modifiers: Default::default(),
                        click_count: 1,
                    }),
                ] {
                    window.dispatch_event(input, cx);
                }
                let _ = window.draw(cx);
                if shell.update(cx, |shell, cx| shell.active_source(cx))
                    != crate::home::Source::recommend()
                {
                    return Some((x as f32, y));
                }
            }
        }
        None
    })?;
    anyhow::ensure!(
        switched_at.is_some(),
        "交互探针：沿 tab 条横扫未触发任何切源——舞台 tab 点击失效"
    );
    let active = cx.update_window(handle, |_, _, cx| {
        shell.update(cx, |s, cx| s.active_source(cx))
    })?;
    anyhow::ensure!(
        active == crate::home::Source::new("36"),
        "交互探针：首个翻源的 tab 应是「漫剧」（实际 {active:?}）"
    );
    log::info!(
        "[Preview] 交互探针通过：舞台 tab 点击切源生效（落点 {:?}）",
        switched_at
    );
    Ok(())
}
