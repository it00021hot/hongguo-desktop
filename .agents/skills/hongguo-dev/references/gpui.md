# GPUI 壳开发规范（crates/hongguo-gpui）

本文件是 GPUI 壳相关规范的唯一归属：壳形态、界面规范、验收流程、已知坑。后端 service/store 等两壳共用的规范见 `backend.md`；Tauri 壳专属的 IPC / React 前端规范见 `ipc.md` / `frontend.md`。

## 第 0 步：动可见界面前先过设计 skill（强制）

改任何**可见界面**（布局 / 主题 / 组件 / 交互）之前，必须先经 Skill 工具调用 `gpui-ui-design` skill，并按其指引读 design-guides——本文件只管壳内约定，不回答「界面该长什么样」。纯引擎、装配、桥接、取证类改动不需要。

本壳基于 GPUI Kit（longbridge/gpui-kit，gpui-base + gpui-component 聚合）：组件与主题令牌都从 `gpui_kit` 出，不自造组件原语；GPUI Kit 专属的设计 / 编码规则以 gpui-ui-design 的 guides 为准。

## 壳形态与边界（改装配前必读）

- **不复制业务**：设备引导、`AppStateInner` 装配、接口调用（`state.api_env()` + `hongguo_api`）与 Tauri 壳走同一段服务面；壳内只做窗口、装配与渲染。需要新业务能力时进 service（发布 `ServiceEvent`），壳只消费——不许把业务写进视图。
- **数据根平级分离**：壳经 `paths::set_shell_data_root` 声明与默认根平级的独立数据目录（GPUI 壳 = `%APPDATA%/hongguo-gpui`），与 Tauri 根互不嵌套、互不共用——嵌子目录的形态已被否决，不得改回。因此 `make gpui` 可与 `make dev` 并存、不撞库锁。
- **异步桥**（`bridge.rs`）：service 的 async 族假定 tokio 设施（`sleep` / `Semaphore`），而 GPUI 执行器是 smol 系——壳持一个常驻 tokio 运行时线程（`main.rs` 的 `TOKIO`），`bridge::spawn` 投递 + oneshot 回传；`Receiver` 不依赖 tokio 上下文，可在 GPUI 任务里直接 await。`rx.await.expect("tokio 桥不会丢弃发送端")` 是不变量断言（运行时常驻到进程结束），新增调用沿用同一措辞，别加兜底。
- **事件泵**（`events.rs`）：service 业务事件经 broadcast 发布，泵转发进 mpsc 无界通道（同步原语，GPUI 执行器可直接 await）；消费方在视图构造时起一条 `cx.spawn` 循环把事件落进视图状态——与 Tauri 壳消费同一批事件（一份业务实现，两种 UI 适配）。
- **封面与 HTTP**：HEIC 封面走 `hongguo-protocol` 的转码阶梯 + `spawn_blocking`（与 Tauri 的 `hongguo-cover://` 同源）；远程拉封面走 `http_client.rs` 的 HttpClient 桥，请求进常驻 tokio 运行时（hyper 的 DNS 解析依赖 tokio 反应器，GPUI 执行器没有）。
- **模块归属**：`shell.rs`（侧栏导航 / 目的地切换，对齐 Tauri 客户端信息架构）、`home/`（首页沉浸流，按域拆子模块：feed / playback / danmaku / comments / interact / auth / controls / mini_panel / picker / downloads / stage / fmt）、`rank.rs`、`mini.rs`（小屏）、`preview/`、`screenshot.rs`（feature 门控）。新文件按域落位，参照 `home/` 的模块树；文件规模红线与 `backend.md` 同款。

## 界面规范

- **语义令牌禁硬编码色值**（硬红线 11 的 GPUI 对应条）：颜色一律走 gpui_kit `ActiveTheme` 主题令牌，亮暗主题都不得写死；唯一例外是播放器控制栏 / 互动栏 / 评论面板的 hgplayer 红色皮肤。
- **图标两分**：通用组件图标用 gpui_kit `Icon` + `IconName`（assets 集合）；自绘图标（播放器控制栏的 B 站形态弹幕/网页全屏/小窗）走 `home/player_icons.rs` 模式——几何内嵌 SVG 字节（`assets/player/*.svg`）1:1 移植 Tauri 版、`fill/stroke` 全 `currentColor`、激活态由外层 `text_color` 接管、不经 AssetSource；含汉字的图标（「弹」字方框）用 div 拼文本，不吃 SVG `<text>` 的系统字体回退。
- **点击穿透**：重叠 bounds 的层（悬浮层 / 面板 / 控制条）必须 `stop_propagation`，否则点击透到下层。
- **组件优先 gpui_kit**（Sidebar / Button / Input / Slider…）；确需新原语先读 gpui-ui-design 的 coding guides。Tauri 端已有的交互形态（信息架构、面板行为、皮肤）是默认对齐基准，不另起炉灶。
- **视图字段私有**：跨模块给视图种数据只能长在 home 树内（`impl HomeView` 才够得着），不为此开 pub 字段。

## 验收（改界面必跑）

- **`make gpui-preview` 离屏预览矩阵是标准验收**：无头渲染真实 `Shell` + 假数据快照（`preview/fixtures.rs` + `home/preview_seed.rs`），逐场景出 PNG 逐张目检。与真实壳同一套视图 / 主题 / 布局代码，只有数据源不同；离线、确定、不碰真实数据根（`hongguo-gpui-preview-<pid>` 临时目录跑完即删）。机制细节见 `crates/hongguo-gpui/src/preview/mod.rs` 头注释。
- **预览边界**（图里的预期形态，不是 bug）：进度条恒 00:00、弹幕无在飞条目、封面为合成渐变、评论头像走首字占位；gpui 无头渲染器仅 Metal（macOS）/ wgpu（Linux），Windows 上截图报错退出。
- **扩展矩阵三步**（新页面 / 新面板必做）：`preview/scenes.rs` 加场景（键名唯一）→ `home/preview_seed.rs` 加种子分支 → `tests/preview_scenes.rs` 的 `SCENE_KEYS` 同步补键（覆盖率测试盯着失同步）。下载假任务灌**真实队列**（`state.replace_queue`），直接种 `self.downloads` 会被 Render 每帧的 `tasks_of` 刷新覆盖。动过场景矩阵 / 种子分发就跑 `cargo test -p hongguo-gpui --test preview_scenes`。
- **运行中窗口取证用 `screenshot.rs`**（与预览矩阵互补不互替）：预览验布局 / 主题 / 面板全分支（离线、无引擎）；`HONGGUO_SHOT` 对运行中窗口进程内抓帧，验真实数据与播放链路。macOS 上 `screencapture` 抓别的进程要屏幕录制授权（抓到的是壁纸），`render_to_image` 不经窗口服务器、无需授权，是图形链路唯一可靠的取证手段。

## 已知坑（每条锁着一条纪律）

- **cargo test 与本壳 bin**：`[[bin]] test = false` 是铁的——cargo test 会把无测试框架的 bin 当用例**直接执行**，本壳一跑就开窗口，把全仓测试挂死。要测的东西放 `tests/`（集成测试）：现状 `preview_scenes`（纯数据覆盖率，默认构建可跑）与 `emoji_insert`（需 `screenshot` feature）。bin 无 lib 目标，集成测试进不了 crate 内部，唯一通路是 `#[path]` 源文件级引入（`scenes.rs` 自洽、不引 `super::*` 就是为这个）；`#[cfg(test)]` 单测在这 crate 里永远跑不了。
- **RenderImage 纪律**：造 `RenderImage` 全壳只许 `image_upload.rs` 一处，且必须过 RGBA→BGRA（源码守卫测试盯着）。
- **交叉 clippy 排除本壳与 hongguo-playback**：gpui 系依赖与 cpal 的 build script 要 bindgen Apple 框架头，Windows 宿主没有 macOS SDK 生不出来；macOS 覆盖由 Mac 本机与 CI 承担（见 Makefile lint 注释）。别为了让交叉 clippy 过去改依赖。
- **screenshot feature 的依赖口径**：不启用 gpui-kit 的 test-support（会连带拉起 gpui-fast 加速栈，与 gpui-pre 在 unicode-properties 上版本打架解不开）；开 gpui-pre / gpui-pre-platform 两家**同版本** test-support 即可（`render_to_image` 实现门控在这两处）。
