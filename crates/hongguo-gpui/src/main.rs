//! 红果桌面版 GPUI 壳（起步单壳，`docs/architecture-refactor.md` §4.3）。
//!
//! **操作与 Tauri 客户端一致**：装配序（开库 → 设备 adopt → 全局状态）、
//! 接口调用（`state.api_env()` + `hongguo_api`）与 Tauri 壳逐层同源——本壳
//! 不含任何业务逻辑，只做窗口、装配与渲染。UI 按 `.agents/skills/
//! gpui-ui-design` 的规范走 GPUI Kit（gpui-component 组件系统 + 主题令牌），
//! 持久侧栏导航对齐 Tauri 客户端的信息架构（首页 / 排行）。
//!
//! 异步桥：service 的 async 族假定 tokio 运行时设施（`tokio::time::sleep`、
//! `Semaphore`），而 GPUI 自带的是 smol 系执行器。本壳持一个常驻 tokio
//! 运行时线程，GPUI 任务经 `oneshot` 回传结果——`Receiver` 不依赖 tokio
//! 上下文，可在 GPUI 执行器上直接 await。
//!
//! 范围：浏览类操作（推荐流 / 排行榜，含封面、加载与失败态）。剧集详情与
//! 播放（帧流 → GPU 纹理）是 §5.3 spike，另行立项。

#[cfg(target_os = "macos")]
mod av_presenter;
#[cfg(target_os = "macos")]
mod av_spike;
mod boss_key;
mod bridge;
mod cover_img;
#[cfg(target_os = "macos")]
mod crash_log;
mod danmaku_emoji;
mod detail;
mod emoji_files;
mod emoji_scan;
mod events;
pub(crate) mod home;
mod http_client;
mod i18n;
mod image_upload;
#[cfg(target_os = "macos")]
mod macos;
mod mini;
mod pages;
mod pill_tab;
mod platform;
mod play_activity;
mod playback_prefs;
mod portrait;
mod preview;
#[cfg(feature = "screenshot")]
mod screenshot;
mod shell;
mod single_instance;
mod toast;
mod tokens;
mod ui_prefs;
mod video_backend;
#[cfg(target_os = "windows")]
mod win32;
mod window_restore;

use std::sync::Arc;
use std::sync::OnceLock;

use gpui_kit::prelude::*;
use hongguo_common::AppError;
use hongguo_service::app_state::AppStateInner;
use hongguo_store::Store;

/// 常驻 tokio 运行时（service 异步族的桥）。
static TOKIO: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub(crate) fn tokio() -> &'static tokio::runtime::Runtime {
    TOKIO.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio 运行时初始化失败")
    })
}

fn main() {
    // 日志先于一切：装配失败的原因只在这里（与 Tauri 壳同一约定）。预览
    // 模式同样要日志与 panic 留痕，所以这两样在数据根声明之前就位。
    // 毫秒时间戳：卡顿诊断看的就是亚秒级间隔（2x 下一帧 16.7ms），
    // 默认的秒级精度会把「停顿 120ms」和「正常」印成同一秒。
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    // Windows 默认定时器精度 ~15.6ms：播放循环的毫秒级轮询会被拉长成一整个
    // tick，2x 下变成「两帧一起到 → 33ms 空档」的节拍抖动（日志里
    // `[Playback][video]` 的 ≤20/≤33ms 分档能直接看到）。浏览器/播放器都会
    // 在播放期把精度提到 1ms，这里同样做；进程退出系统自动回收。
    #[cfg(target_os = "windows")]
    {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn timeBeginPeriod(period: u32) -> u32;
        }
        let result = unsafe { timeBeginPeriod(1) };
        if result == 0 {
            log::info!("[Shell] Windows 定时器精度已提升到 1ms");
        } else {
            log::warn!(
                "[Shell] timeBeginPeriod(1) 失败（{result}），播放轮询可能受 15.6ms 粒度影响"
            );
        }
    }
    // panic 留痕：音频设备回调等非主线程的 panic 会静默带走整条线程
    // （实测：回调线程一死，声音原地消失、时钟冻结、界面无限「缓冲中」，
    // 日志里却一片安静）。钩子让它至少留下现场，然后 abort 显性化。
    std::panic::set_hook(Box::new(|info| {
        log::error!(
            "[Panic] 线程 {:?} panic：{info}",
            std::thread::current().name().unwrap_or("<unnamed>")
        );
        std::process::abort();
    }));
    // Phase 0 AVPlayer spike：HONGGUO_AV_SPIKE=1 时整个进程走独立诊断路径，
    // 不声明数据根、不抢单实例、不碰任何业务状态（默认启动路径零改动）。
    #[cfg(target_os = "macos")]
    if crate::av_spike::requested() {
        crate::av_spike::run();
        return;
    }
    // 离屏预览：--render-preview <目录> 直接接管进程。必须在下方声明
    // 交互壳数据根之前分发——preview::run 要把数据根指到预览专用目录
    //（set_shell_data_root 是 OnceLock，只有第一次生效）。管线编在
    // screenshot feature 里，裸构建给不出图，明确报错而不是静默忽略。
    let preview_dir = preview::requested_dir();
    #[cfg(feature = "screenshot")]
    if let Some(dir) = preview_dir {
        preview::run(&dir).expect("预览渲染失败");
        return;
    }
    #[cfg(not(feature = "screenshot"))]
    if preview_dir.is_some() {
        eprintln!(
            "--render-preview 需要 screenshot feature：\
             cargo run -p hongguo-gpui --features screenshot -- --render-preview <目录>"
        );
        std::process::exit(2);
    }
    // 独立数据根最先声明：GPUI 壳用与 Tauri 客户端平级的独立目录
    // （%APPDATA%\hongguo-gpui，与 hongguo-downloader 无任何交集），
    // 互不锁库（实测并存时后启动者因库锁直接退出）。
    hongguo_store::paths::set_shell_data_root("hongguo-gpui");
    // 崩溃取证：数据目录就位后装（信号处理器要把调用栈写进数据目录）。
    #[cfg(target_os = "macos")]
    crash_log::install();
    // 单实例：已有实例在跑就请它把窗口带到前台，自己安静退出（Tauri
    // single-instance 插件同语义）。必须早于开库——上一个实例可能正持库锁
    //（隐身藏窗/小屏/竖屏形态都在跑），撞锁退出会让用户看到「点图标没反应」。
    if !single_instance::claim() {
        log::info!("[Shell] 已有实例在运行：已请求它前置窗口，本进程退出");
        return;
    }
    let state = build_app_state();

    // 资产源必须接上：gpui 的 SvgRenderer 从 App 的 AssetSource 读 SVG——
    // 不接的话**所有纯图标按钮都是空壳**（按钮在、图标没了，实测控制条
    // 只剩文字按钮可见）。注意用 AllAssets（完整 Lucide 目录）而不是
    // Assets（那只是组件兼容默认子集 106 个，音量/弹幕/小窗/全屏等
    // 图标不在其中，缺的就是它们）。
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            // 老板键：按持久化设置注册（Tauri boss_key_cmd::init 同款）。
            // 必须在 App 跑起来之后注册：Carbon 的 application event target
            // 要等 NSApplication 就位，开跑前注册会拿到空 target、把热键
            // 装到一个永远不会派发事件的地方。失败只记日志——快捷键被占用
            // 不该拦启动，用户进设置换一个即可。
            if let Err(e) = boss_key::apply(&state.settings().boss_key) {
                log::warn!("[BossKey] {e}（进设置换一个快捷键）");
            }
            // Windows 非打包应用的 Toast 需要稳定的 AppUserModelID；macOS
            // 通知中心也用同一身份展示。必须在开窗前设好。
            cx.set_app_identity("com.hongguo.desktop", "红果");
            // 语言与主题一样在开窗前就位：所有首帧文案都必须已经是用户上次
            // 选择的语言，不能先画中文再闪成英文。
            let ui_prefs = crate::ui_prefs::load();
            crate::i18n::set_locale(ui_prefs.locale);
            // 封面是 CDN 签名 URL：gpui 图片缓存需要一个 HTTP 客户端才能拉远程资源。
            http_client::ReqwestClient::install(cx);
            // 主题偏好（Tauri `hongguo-theme` store 同款三档）：auto 跟随系统
            // 外观（开窗前就能从平台拿到），light/dark 为显式覆盖——显式档位
            // 同时把窗口外观钉给平台（macOS 窗口边框跟随；Windows 无操作），
            // auto 则保持不钉，系统切换时观察者会重算（见 Shell::new）。
            let theme_pref = ui_prefs.theme;
            let theme_mode = theme_pref.resolve(cx.window_appearance());
            crate::tokens::apply(theme_mode, cx);
            if theme_pref != crate::ui_prefs::ThemePref::Auto {
                cx.set_window_appearance(Some(match theme_mode {
                    gpui_kit::component::ThemeMode::Light => gpui_kit::WindowAppearance::Light,
                    gpui_kit::component::ThemeMode::Dark => gpui_kit::WindowAppearance::Dark,
                }));
            }
            // 默认窗口与 Tauri conf 同参（1280×840，可恢复下限 480×270，
            // 与 mini 同尺寸）。优先级：取证开关 HONGGUO_WINDOW=宽x高（中置）>
            // 上次窗口几何（落在当前任一显示器内才恢复，issue #11；Tauri 壳
            // window_state 同语义）> 中置默认。取证开关要求整页入镜，不恢复
            // 旧位置。
            let forensic = std::env::var("HONGGUO_WINDOW").ok().and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
            });
            let saved = if forensic.is_some() {
                None
            } else {
                crate::window_restore::restore_rect(
                    ui_prefs.win_x,
                    ui_prefs.win_y,
                    ui_prefs.win_w,
                    ui_prefs.win_h,
                    &crate::platform::memory_display_rects(cx),
                    crate::window_restore::MIN_RESTORE_W,
                    crate::window_restore::MIN_RESTORE_H,
                )
                .map(|(x, y, w, h)| {
                    crate::platform::OuterRect::new(
                        f64::from(x),
                        f64::from(y),
                        f64::from(w),
                        f64::from(h),
                    )
                })
            };
            let bounds = if let Some((w, h)) = forensic {
                gpui_kit::Bounds::centered(
                    None,
                    gpui_kit::size(gpui_kit::px(w), gpui_kit::px(h)),
                    cx,
                )
            } else if crate::platform::RESTORE_AFTER_OPEN {
                // macOS：开窗用默认尺寸，创建后按 AppKit 外框精确恢复。
                match saved {
                    Some(rect) => log::info!(
                        "[Window] 恢复上次几何（平台坐标）x={} y={} {}×{}",
                        rect.x,
                        rect.y,
                        rect.w,
                        rect.h
                    ),
                    None => log::info!("[Window] 无可用几何记忆，按默认 1280×840 中置"),
                }
                gpui_kit::Bounds::centered(
                    None,
                    gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(840.)),
                    cx,
                )
            } else {
                match saved {
                    Some(rect) => {
                        log::info!(
                            "[Window] 恢复上次几何 x={} y={} {}×{}",
                            rect.x,
                            rect.y,
                            rect.w,
                            rect.h
                        );
                        gpui_kit::Bounds {
                            origin: gpui_kit::point(
                                gpui_kit::px(rect.x as f32),
                                gpui_kit::px(rect.y as f32),
                            ),
                            size: gpui_kit::size(
                                gpui_kit::px(rect.w as f32),
                                gpui_kit::px(rect.h as f32),
                            ),
                        }
                    }
                    None => {
                        log::info!("[Window] 无可用几何记忆，按默认 1280×840 中置");
                        gpui_kit::Bounds::centered(
                            None,
                            gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(840.)),
                            cx,
                        )
                    }
                }
            };
            // 句柄两边都用：默认构建挂退出确认守卫、screenshot 构建再交给抓帧
            let (shell_window, shell) = gpui_kit::open_window(
                // 无边框（Tauri decorations:false 同款）：以组件库 TitleBar
                // 的窗口选项为基底——标题栏自绘、拖拽与窗口钮走平台原生命中
                // 测试（WindowControlArea → HTCAPTION/HTMINBUTTON/…）。
                {
                    let mut options = gpui_kit::component::TitleBar::window_options();
                    options.window_bounds = Some(gpui_kit::WindowBounds::Windowed(bounds));
                    // 标题空着的话任务栏是一条空白条目，用户根本认不出这个窗口
                    options.titlebar = Some(gpui_kit::TitlebarOptions {
                        title: Some("红果".into()),
                        ..gpui_kit::component::TitleBar::title_bar_options()
                    });
                    options
                },
                cx,
                |window, cx| {
                    // 组件库管不到的最后一寸：WS_THICKFRAME 引出的 1px 系统
                    // 边框线，按 DWM 边框色关掉（缩放/贴靠不受影响）
                    #[cfg(target_os = "windows")]
                    win32::hide_border(window);
                    cx.new(|cx| shell::Shell::new(state.clone(), window, cx))
                },
            )
            .expect("窗口创建失败");

            // macOS：窗口创建后按平台坐标精确恢复（gpui 的 WindowBounds 只认
            // 它自己的坐标空间，多屏下会丢显示器原点）
            if crate::platform::RESTORE_AFTER_OPEN
                && let Some(rect) = saved
            {
                let _ = shell_window.update(cx, |_, window, cx| {
                    crate::platform::memory_restore_frame(window, cx, rect);
                });
            }

            // 落框取证：延迟一拍打印「记忆坐标」里的实际外框（与恢复日志同口径）
            let actual_window = shell_window;
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                cx.update(|app| {
                    let _ = actual_window.update(app, |_, window, _| {
                        if let Some(rect) = crate::platform::memory_rect(window) {
                            log::info!(
                                "[Window] 实际外框 x={} y={} {}×{}",
                                rect.x,
                                rect.y,
                                rect.w,
                                rect.h
                            );
                        }
                    });
                });
            })
            .detach();

            // 点击系统通知（追更更新）→ 激活已有窗口；Tauri Web Notification
            // 的 `window.focus()` 同语义。
            let notification_window = shell_window;
            cx.on_system_notification_response(move |_response, cx| {
                let _ = notification_window.update(cx, |_, window, _| {
                    window.activate_window();
                });
            });
            shell.update(cx, |shell, cx| shell.activate(cx));
            // 退出确认：窗口关闭先问一句（Tauri CloseRequested → 确认框同款）。
            // 挂在实体就位之后——守卫回调要用壳的弱引用。
            let _ = shell_window.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.install_close_guard(window, cx));
            });
            // 图形链路取证：HONGGUO_SHOT=… 时进程内抓帧存 PNG（见截图模块头注释）
            #[cfg(feature = "screenshot")]
            screenshot::install(shell_window, shell.clone(), cx);
        });
}

/// 开库并组装全局状态。与 Tauri 壳的装配逐层同源：
/// 开库（锁冲突短重试一次）→ [`hongguo_service::device::adopt_sync`] →
/// [`AppStateInner::with_device`]。
fn build_app_state() -> Arc<AppStateInner> {
    let db = match open_store_with_lock_retry() {
        Ok(db) => db,
        Err(AppError::StoreLocked(e)) => {
            log::error!(
                "[Store] 数据库被占用，终止启动: {e}\n\
                 另一个 GPUI 壳实例可能还在运行（隐身时窗口整个藏掉，注意杀进程）。"
            );
            std::process::exit(1);
        }
        Err(e) => {
            log::error!("[Store] 数据库打开失败，终止启动: {e}");
            std::process::exit(1);
        }
    };
    let (device, device_source) = hongguo_service::device::adopt_sync(&db);
    log::info!(
        "[Device] 现役档案 iid={}（{device_source}）",
        device.get("iid")
    );
    let state = Arc::new(AppStateInner::with_device(db, device));
    // 设置装载（Tauri bootstrap store.rs 步骤 2 同款）：db 里存过就装载并
    // 归一化——登录账号（评论/回复的 isMine、发送、点赞全靠它）、下载根、
    // 并发数都从这里来；gpui 壳此前没装载，这些一直是默认值。
    match state.store.settings() {
        Ok(Some(stored)) => {
            let settings = hongguo_service::settings_service::normalize(stored).unwrap_or_default();
            log::info!(
                "[Store] 设置装载完成（账号={}）",
                settings
                    .account
                    .as_ref()
                    .map(|a| a.user_id.as_str())
                    .unwrap_or("未登录")
            );
            hongguo_service::settings_service::apply_cache_root(&settings);
            state.replace_settings(settings);
        }
        Ok(None) => log::info!("[Store] 无存量设置，使用默认值"),
        Err(e) => log::warn!("[Store] 设置装载失败，使用默认值: {e}"),
    }
    // 追更更新检查定时器（Tauri 壳 lib.rs setup 同款：启动查一轮 + 20 分钟周期）
    hongguo_service::follow_service::start_notice_loop(state.clone());
    state
}

/// 开库；被锁时稍等重试一次（与 Tauri 壳同一口径）。
fn open_store_with_lock_retry() -> hongguo_common::AppResult<Store> {
    match Store::open(hongguo_store::paths::db_file()) {
        Ok(db) => Ok(db),
        Err(AppError::StoreLocked(first)) => {
            log::warn!("[Store] 数据库暂时被锁（{first}），1s 后重试一次");
            std::thread::sleep(std::time::Duration::from_secs(1));
            Store::open(hongguo_store::paths::db_file())
        }
        Err(e) => Err(e),
    }
}
