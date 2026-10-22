# NSH 壳实施方案（hongguo-nsh）

（2026-10-21。前置事实：`hongguo-nsh-demo` 双平台闸门实测通过——macOS/AppKit 此前已过，Windows/MediaEngine+Direct2D 本日全过：起播走秒、Range 206 供数桥、切集/暂停/恢复/on_end 自动连播、播放中真实内存占用 ~200-250MB（私有工作集口径，切集零增长）、144Hz 屏弹幕 FPS 顶格。换壳总评估与工作量拆账见 `crates/hongguo-nsh-demo/docs/migration-plan.md`，本文是它的落地工程方案，**四项决策已拍板（§7）**。）

> 决策纪要：小窗 = 单窗口几何切换（对齐 Tauri，非新窗口）｜单实例 = 激活已开窗口后退出｜仓库节奏 = 与 GPUI 壳共存，功能对齐后再逐步移除｜NSH_SCRIPT 冒烟脚本 = 提交进仓库作验收门。

## 0. 总判断

**后端九域 service + 14 域 api + store/protocol/stream 零改动复用**（红线保证无 UI 依赖，demo 已验证）。新工作全部在壳层：GPUI 壳 41.4k 行的重写，NSH 声明式组件更短，预计新写 ~2.5-3 万行。原评估中"Windows 宿主"这唯一翻倍风险已解除，**维持主干 3-5 周、全功能对齐 6-9 周（单人量级）**，且 Windows 不再需要单列复制期，两端并行开发。

## 1. 目标与非目标

**目标**：新建 `crates/hongguo-nsh/` 第三壳，与 GPUI 壳**共存**开发；功能对齐并稳定后逐步移除 GPUI 侧（替值线，见 migration-plan §六）。

**非目标**：Tauri 官方壳不动；弹幕数据源重对齐（服务端只回引导条，需重抓 hgplayer）单列，不阻塞壳开发——渲染管道已通，数据修好即接上。

## 2. NSH 0.1.1 能力边界（选型的硬约束）

本日实测 + 源码核对（crates.io 发布版）：

- **单窗口模型**：`AppOptions` 只有一套 `title/size/appearance/script`，无内建多窗口、无托盘、无单实例、无内建导航栈——导航与这些壳能力全部自建。`AppView` 只给出原生句柄（Windows `hwnd()` / macOS `view()`），**没有 set_size / set_position**；几何改由壳内 platform 小件做（§4.5 已实测 Windows 可行）。
- `script`（`NSH_SCRIPT`）是内建验收通道：`wait/shot/tap/drag/key/text/resize/dark/light/screen/quit`，进程内截图（Windows 上连视频像素都能拍到）、进程内 `resize` 按客户区尺寸——壳级验收工具零成本，等价 GPUI 的 `make gpui-preview`。
- 纯 Rust 路线用 `default-features = false`（关 QuickJS）；API 与 GitHub main 有漂移（`Listener` 收 `&Event`、`VideoInfo` 平铺字段、`native_value()`、`NativeValue`/`ElementRef` 非 Copy），**以 demo 修好的写法为准，版本钉死 `0.1.1`**。
- 崩溃/生命周期：nsh 有 `background()`（自带 tokio 线程）与信号泵；demo 已验证 service async 桥 + `ServiceEvent` 转发模式。

## 3. 新壳 crate 结构（crates/hongguo-nsh/）

```
crates/hongguo-nsh/
├── Cargo.toml              # nsh-core/nsh-platform-* = "0.1.1"（default-features=false）
├── nsh.json                # 应用清单（id/appearance/主题色）
└── src/
    ├── main.rs             # 入口：windows_subsystem、日志落盘、单实例抢先、AppOptions
    ├── app.rs              # 根组件：导航宿主 + 全局叠层（toast/确认框/更新条）
    ├── nav.rs              # 轻量 NavStack：信号驱动的页面栈 + 返回键/侧滑
    ├── state.rs            # AppStateInner 构造（真实 Store + 数据根）+ 常驻 tokio 运行时
    ├── pump.rs             # 信号泵：ServiceEvent 总线一个转发任务 → nsh 信号（替代 bridge.rs+events.rs 两件套）
    ├── supply/             # 本地解密供数桥（demo stream.rs 正式化）
    │   ├── mod.rs          #   tiny_http 常驻 + 端口生命周期 + pool
    │   └── cover.rs        #   封面三级转码阶梯供数（对齐 hongguo-protocol::cover 语义）
    ├── theme/
    │   ├── tokens.rs       # 语义令牌（对齐 GPUI 壳 tokens.rs 色板/字阶/间距）
    │   └── i18n.rs         # include_str! 复用 src/locales/{zh-CN,en-US}.json + 访问器
    ├── platform/           # 壳内唯一的平台代码（业务/视图层零 #[cfg]）
    │   ├── mod.rs          #   门面：set_window_mode(WindowMode) / hwnd/view 分发
    │   ├── windows.rs      #   SetWindowPos + AdjustWindowRectExForDpi + 单实例 mutex
    │   └── macos.rs        #   NSWindow setFrame:display: + 单实例 lock
    ├── components/         # 通用件：泛型分页列表（list_common+scroll_load 抽象）、sheet、
    │   │                   #   toast、空态、错误态、半屏登录、图片上传入口
    ├── player/             # 播放域（对齐 GPUI home/ 13.7k 的职责面）
    │   ├── stage.rs        #   video 元素舞台 + 弹幕带（demo 的 band/车道调度直接搬）
    │   ├── danmaku.rs      #   时间轴管道 + 车道占用 + 到期合批回收 + FPS 统计（demo 直搬）
    │   ├── controls.rs     #   叠层控制栏（进度/播放/切集/清晰度/倍速/横竖屏）
    │   ├── comments.rs     #   评论/剧评面板（commentapi，社交态 staleTime 0）
    │   └── mini.rs         #   小窗：只发 WindowMode 信号 + 切控制栏紧凑形态（几何在 platform/）
    └── pages/              # 内容域（对齐 pages/ 13.1k + detail/ 3.4k）
        ├── home.rs         #   推荐流（分页列表）
        ├── detail.rs       #   详情 + 分集 + 主演/出品方跳创作者
        ├── search.rs rank.rs category.rs history.rs favorites.rs follow.rs
        ├── creator.rs      #   创作者主页（profile 一张响应带全作品）
        ├── settings.rs login.rs tasks.rs
        └── ...
```

分层规则沿用仓库既有判定：**壳不复制业务**——操作与 service 契约一一对应。平台分支只允许出现在 `platform/` 门面模块（NSH 已抹平渲染/输入两端，业务与视图层出现 `#[cfg(target_os)]` 即返工）。

## 4. 关键设计

### 4.1 状态与事件

- 壳直接构造 `AppStateInner`（**正式壳接真实 Store**：`hongguo-store` 持久库 + `paths::ScopedDataDir`；demo 的 `default()` 内存库只留在 demo）。数据根与 Tauri 壳平级分离（`set_shell_data_root` 惯例，测试锁死），不嵌套。
- async service 族走 nsh `background()`（自带 tokio 上下文），oneshot 回 UI 信号——demo 已验证的模式，替代 GPUI 的 bridge.rs。
- `ServiceEvent` 用**一个转发任务**把 broadcast 总线泵进 nsh 信号（pump.rs），替代 bridge.rs + events.rs 两件套。

### 4.2 供数桥正式化

demo 的 `supply` 雏形（tiny_http + `hongguo-protocol::respond_stream` + 进程级 stream_pool）直接提升，补三件事：端口生命周期随壳（含异常退出后的端口清理）、封面/HEIC 阶梯供数并入同一桥、URL 形态统一 `{scheme}://{host}/{kind}/{档位}/{vid}`（对齐 hongguo-protocol 现有语义）。MediaEngine/AVPlayer 对该桥的容忍度已双平台实测。

### 4.3 导航（自建 nav.rs）

信号驱动的 `Vec<Page>` 栈 + 转场；返回键接 nsh `on_back_press`（Android/鼠标侧键）。不引路由库——页面数量 ~15，栈语义自足。

### 4.4 单实例（自建，直接移植 GPUI 壳已验证实现）

行为：第二实例**激活已开窗口后退出**（对齐 Tauri `bring_back_active_window` 与 GPUI 现状）。

实现直接搬 `crates/hongguo-gpui/src/single_instance.rs` + `win32.rs::claim_single_instance`（两平台都已实测）：
- Windows：命名互斥体 `HongguoNsh.SingleInstance` + 命名事件 `.Focus`；第二进程 `SetEvent` 后退出，首实例轮询 `take_focus_request()` 时 show/还原/聚焦。
- macOS：数据目录下 Unix 域套接字 `nsh-shell.sock`；第二进程 `connect` 成功即写一字节退出；崩溃残留按「connect 失败即陈档」清理。

**顺序红线**：`claim()` 必须早于窗口创建与数据库打开（否则二次启动会撞库锁被误报「数据损坏」，与 Tauri 壳第 8 条红线同源）。轮询挂进 nsh 的 `set_interval(100ms)`。

### 4.5 小窗（= 单窗口几何切换，对齐 Tauri）

**不做第二个窗口、不做窗口内悬浮层**。现有两壳（Tauri / GPUI）的小窗语义本来就是「同一个窗口缩成 480×270 落到工作区右下角，视频元素原地不动、播放零中断」——`crates/hongguo-tauri/src/commands/window_cmd.rs::enter_mini_screen` 的注释明确否掉了独立小窗方案（两路声音叠放、窗口管理混乱）。NSH 壳照抄这个语义即可，不需要在「悬浮层 / 嵌入子窗口」之间选型。

实现路径：nsh 的 `AppView` 只公开 `hwnd()`（Windows）/ `view() -> Retained<NSView>`（macOS），**没有内建的 set_size / set_position**，所以几何操作由壳内 `platform/` 小件用原生句柄完成：

- Windows：`SetWindowPos` + `AdjustWindowRectExForDpi`（外框 ↔ 内容尺寸换算，`WM_SIZE` 由 nsh 自己接住并重排重绘）；
- macOS：`view().window()` 取回 `NSWindow` 后 `setContentSize:`/`setFrame:display:`。源码级依据：nsh 自己的 `resize` 脚本动词就是 `window.setContentSize(NSSize::new(w, h))`，说明「内容尺寸」语义在 macOS 后端成立；`NSView::window()` 由 objc2-app-kit 0.3.2 提供（nsh-platform-macos 已 `pub use objc2_app_kit`）。

平台分支集中在这一个模块，业务与视图层零 `#[cfg]`。

**已实测（Windows，2026-10-21）**：
- nsh 内建的 `resize` 脚本动词（`NSH_SCRIPT='resize 480 270'`）把**客户区**精确设成 480×270（外框 496×309），再 `resize 430 800` 完整还原——说明 nsh 的 `WM_SIZE → surface.resized → 重排重绘` 链路对程序化改尺寸完全成立，视频播放不中断；
- 壳层自己 `SetWindowPos` 同样生效（走的是同一个 WM_SIZE 路径）；
- nsh 的 `WM_GETMINMAXINFO` 只把最小跟踪尺寸钳在 200×150（点），360×180 的 `MINI_MIN` 不会被拦。

**待验（macOS）**：本机只有 Windows，`NSWindow.setFrame` 路径按 API 形态可行但未实测，P1 首个双平台冒烟脚本里加一条 `resize` 断言。

三个窗态槽位（Normal / Mini / Portrait）与几何记忆沿用 Tauri 壳 `window_state` 的行为契约：快照用内容尺寸、`MINI_MIN = 360×180`、`MINI_MARGIN = 16`、小工作区压到放得下为止、手动 min/max 而非 clamp。

### 4.6 其余壳能力对齐清单

GPUI 壳根散文件 → NSH 壳归宿：`crash_log.rs`→panic 钩子+落盘；`toast.rs`→components/toast；`tokens.rs`→theme/tokens；`i18n.rs`→theme/i18n；`playback_prefs.rs`/`ui_prefs.rs`→settings service 直读；`window_restore.rs`+`mini.rs`+`platform/`→platform 小件（窗口几何与三窗态，业务层只读一个 `WindowMode` 信号）；`boss_key/`→全局热键（nsh 无内建热键，P3 用平台 API 小件）；`screenshot.rs`/`image_upload.rs`/`emoji_*`/`danmaku_emoji`→对应页随 P2/P3 迁；`cover_*`三件套→删除（`image` 元素原生加载供数桥 URL，平台自带缓存——已验证）；`video_backend.rs`/`av_spike/`/`preview/`→删除（随替值线）。

## 5. 分期路线（每期结束都可用、可验收）

- **P1 主路径（~2 周）**：壳骨架（状态/事件泵/导航/令牌/i18n/单实例/日志/崩溃取证）+ 供数桥正式化 + 推荐流 + 详情 + 搜索 + 播放页完整化（控制栏/清晰度/倍速/评论面板/自动连播）+ 小窗（单窗口几何切换）。**产出：无账号可用的"电视机"**。验收：`crates/hongguo-nsh/scripts/` 的全页冒烟脚本（wait/tap/resize/shot/quit）双平台绿 + 关键页截图目检。
- **P2 账号域（~1.5 周）**：登录（QR image + 轮询信号 / 短信）、历史、收藏、追更、弹幕数据源重对齐接入（依赖抓包工作流，可与本期并行）。验收：真账号双平台走通"登录→看→收藏→历史→追更"。
- **P3 长尾（~2-3 周）**：设置/任务中心/创作者主页/榜单/分类/老板键/截图/表情/性能打磨（长列表 scroll、弹幕峰值）。随后**替值线**：一个发版周期稳定 → 移除 GPUI 壳与播放引擎（hongguo-gpui、hongguo-playback 3.9k、hongguo-player-* 3.2k、player-api、vendor 双 patch、av_spike），回收 vendor。

## 6. 风险与对策

| 风险 | 对策 |
|---|---|
| nsh 上游 API 漂移（0.1.1 与 main 已有差异） | 钉死 0.1.1；壳层 API 触点集中在 components/player，升级一次改一层 |
| 长列表 scroll 性能未验证（demo 只有弹幕帧率证据） | P1 首周做推荐流 500 项压测；nsh 有 virtual_list 兜底 |
| 单实例/热键等壳能力自建引入平台坑 | 单实例与窗口几何直接移植 GPUI 壳已验证实现（§4.4/§4.5）；热键沿用 `boss_key/accel.rs` 的键位映射 |
| macOS 的窗口几何（`NSWindow.setContentSize`）未在本机实测 | 代码路径同 nsh 自己的 `resize` 动词（源码已核）；P1 冒烟脚本加一条 `resize` 断言，Mac 机上一跑即验 |
| 弹幕数据源修复依赖抓包节奏 | 渲染管道已通，数据源单列不阻塞；未修复期间弹幕显示引导条 |
| 双端一致性（字体/输入法/窗口装饰） | P1 起每期跑双平台 NSH_SCRIPT 冒烟，Windows 侧已验证的坑（日志落 cwd、`--bin` 指名）写进壳 README |

## 7. 已定决策

1. **小窗**：单窗口几何切换（Tauri 语义），不做第二窗口、不做悬浮层——见 §4.5，Windows 已实测可行。
2. **单实例**：第二实例激活已开窗口后退出（对齐 Tauri `bring_back_active_window` / GPUI 现状）。nsh 无内建单实例，壳内自建（Windows named mutex + 全局 lock 文件），且必须**早于窗口创建与数据库打开**，否则二次启动会撞库锁被误报「数据损坏」。
3. **仓库节奏**：`crates/hongguo-nsh` 与 GPUI 壳**共存**至替值线；`make nsh` 新目标并行存在，等 NSH 壳功能对齐后再慢慢移除 GPUI 侧（hongguo-gpui / hongguo-playback / hongguo-player-* / vendor 双 patch）。
4. **验收工具**：NSH_SCRIPT 冒烟脚本**提交进仓库**作为每期验收门，放 `crates/hongguo-nsh/scripts/`；每个脚本文件 = 一条完整链路（启动→等起播→交互→截图→quit），双平台各跑一遍，截图归档到 `target/nsh-smoke/`。

   脚本可用动词（0.1.1 实测/源码确认）：`wait <ms>`、`tap <x> <y>`、`drag <x0> <y0> <x1> <y1> [ms]`、`touch`、`key <组合>`、`selectall`、`text <内容>`、`resize <w> <h>`（**按客户区尺寸**，Windows 已实测精确生效）、`dark` / `light`、`shot <路径>`（进程内渲染，Windows 连视频像素一起拍）、`screen <路径>`（`PrintWindow` 连 DirectComposition 层一起拍）、`quit`。macOS 另有 `zoom` / `fullscreen`。

   `make nsh-smoke` 目标：按顺序跑 `scripts/*.txt`，任一截图缺失即失败。

## 8. 首个提交怎么落

1. `crates/hongguo-nsh/` 骨架：Cargo.toml + nsh.json + main.rs + app.rs + 空 `nav/state/pump/supply/theme/platform`，能起空窗口、双平台跑通；`platform/` 里先只放 `set_window_mode` 与单实例抢占两件。
2. demo 资产提升：danmaku 车道调度、供数桥、信号泵模式原样搬并去 demo 化命名。
3. `scripts/` 首批冒烟脚本（`smoke-home.txt` / `smoke-play.txt` / `smoke-mini.txt`）+ `make nsh-smoke` 目标 + 壳 README（构建/运行/脚本验收/已知坑）。

（純结构提交与行为提交分开，Conventional Commits 中文描述，开发在 dev 分支。）
