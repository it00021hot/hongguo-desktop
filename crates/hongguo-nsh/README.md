# hongguo-nsh — 红果 NSH 壳

第三壳：NSH（纯 Rust 声明式 GUI）驱动整个界面，系统播放器出画面，hongguo-service
供业务。方案与分期见 [docs/nsh-shell-plan.md](../../docs/nsh-shell-plan.md)。

当前进度：**P1 主路径已通，播放页对齐 Tauri 壳**——沉浸式播放（选档 →
CENC 解密 → 本地供数桥 → `video` 元素）、chrome 显隐状态机（3 秒自动收 /
暂停常显 / 点控件续命）、全量控制栏（可拖进度条、倍速/清晰度菜单、选集
浮层（范围段+系列剧）、弹幕开关与设置面板、音量弹层、弹幕发送框）、
右侧互动栏（点赞/收藏/追更/分享，真实计数与乐观翻面）、评论面板（列表/
回复/发送/点赞）、弹幕渲染（进度驱动、四项显示设置）、连播锁定
（「观看完整短剧」横条 / 自动连播 / ↑↓ 语义切换 / Esc 退出）、小窗与
竖屏几何、置顶。排行榜 / 找剧 / 设置仍是占位面板，按方案 §5 逐期迁入；
下载、隐身/无边框/全屏、@提及（小果 AI）随 P2/P3 接。

## 与另外两壳的关系

| | Tauri 壳 | GPUI 壳 | NSH 壳（本 crate） |
|---|---|---|---|
| 界面 | React 19 + WebView | GPUI Kit 组件 | NSH 声明式组件 |
| 播放 | `<video>` + 协议供数 | 自研引擎 / 系统播放器 | `video` 元素（MediaEngine / AVPlayer） |
| 数据根 | `hongguo-downloader` | `hongguo-gpui` | `hongguo-nsh` |
| 后端 | hongguo-service（IPC） | hongguo-service（进程内） | hongguo-service（进程内） |

三壳数据根平级独立，互不锁库；**同时只应开一个壳**（单实例只管得住自己这一壳）。

## 界面结构（对齐 Tauri 壳）

- **侧栏** 224px：顶部是应用名，下面是四行页签（推荐 / 排行榜 / 找剧 / 设置），
  当前页用 `theme().accent` 圆角底块标出。
- **顶栏** 44px：首页时左侧是三枚分类 tab 胶囊（推荐 / 漫剧 / 真人），其他页
  换成页标题；右侧是运行状态文字 + 「小屏播放」按钮。
- **内容区** 无边距铺满。首页 = **沉浸式播放**（不是列表、不是 9:16 舞台）：
  视频铺满整块内容区，右侧互动栏、左下剧名/集数、底部进度条 + 控制条叠在画面上；
  大窗 `ResizeMode::Contain`（竖版源留黑边不裁内容）、小窗 `Cover`（480×270
  固定 16:9），与 Tauri `player-page.tsx` 的 `object-contain` / `object-cover`
  分档一致。悬浮层（chrome）交互：初始可见，**动鼠标即唤醒**（nsh 0.1.1
  没有指针移动事件，壳内 150ms 轮询 `platform_ext::cursor_in_window` 实现），
  播放中 3 秒无操作自动收、暂停与面板打开时常显。舞台手势与按键（nsh
  touch-and-gestures 全量接线）：单击=播放/暂停并唤醒；按下即唤醒；
  **长按画面=2 倍速快进**（松手恢复）；**纵向滑动=上一部/下一部剧**
  （连播中切集，横滑不误触）；`←→` 按住连发快退快进（`KeyOptions::repeat`）。
  纯展示层（弹幕带/键盘提示/加载细条/spinner/错误胶囊）`pointer_events`
  穿透，不挡舞台手势；信息层容器 `box-none`（「展开」可点、空白穿透）；
  进度条 `on_responder_terminate` 复位拖动态；小按钮 `hit_slop` 扩命中区。

## 运行

```sh
make nsh                      # = cargo run -p hongguo-nsh
NSH_SCRIPT='wait 3000; shot /tmp/a.png; quit' make nsh
```

日志落 `<数据根>/nsh-shell.log`（Windows：`%APPDATA%\hongguo-nsh\`），GUI 子系统
没有控制台，这是唯一观测面。

## 冒烟验收

```sh
make nsh-smoke
```

逐个执行 `scripts/*.txt`（每条是一整条链路：启动 → 交互 → 截图 → `quit`），
截图落 `target/nsh-smoke/`，**任一截图缺失即失败**。加新页面时同时加脚本。
现有三条：

- `smoke-pages` — 首页（沉浸播放有画面）→ 点侧栏三个占位页；
- `smoke-mini` — 点「小屏播放」缩到 480×270 → `key Escape` 还原；
- `smoke-play` — 首页起播（chrome 收起态）→ `key space` 暂停亮出全套
  chrome（控制栏/信息层/连播横条/键盘提示）→ `key Down` 换下一部剧再起播。

判据不止「文件存在」：起播链路看日志里两行 `[Player] 起播 vid=… 档位=…`，
画面看截图内容区（非纯色、有纹理）。

脚本动词（nsh 0.1.1）：`wait <ms>`、`tap <x> <y>`、`drag`、`touch`、`key <组合>`、
`selectall`、`text <内容>`、`resize <w> <h>`（**按客户区**）、`dark` / `light`、
`shot <路径>`（进程内渲染，Windows 连视频像素一起拍）、`screen <路径>`
（`PrintWindow`，连 DirectComposition 层一起拍）、`quit`；macOS 另有
`zoom` / `fullscreen`。

坐标是**窗口内容左上原点**、点为单位（1280×840 窗口、dpi=96 下的实测值）：

| 目标 | 坐标 |
|---|---|
| 侧栏 排行榜 / 找剧 / 设置 | (112, 101) / (112, 139) / (112, 177) |
| 顶栏「小屏播放」 | (1236, 22) |
| 顶栏分类 tab（推荐 / 漫剧 / 真人） | (266, 22) / (311, 22) / (372, 22) |
| 退出小屏 | `key Escape`（控制条上也有「退出小屏」胶囊） |

## 结构

- `main.rs` — 入口：数据根、日志、panic 留痕、单实例抢占、`run_app`
- `state.rs` — 开库 → 设备 adopt → 设置装载 → `AppStateInner`（与另两壳同源）
- `pump.rs` — `ServiceEvent` 总线一个转发任务泵进 nsh 信号
- `nav.rs` — `Nav`（侧栏页栈 + 返回键弹栈）、`FeedNav`（顶栏分类 tab 信号）、
  `MiniMode`（小屏态信号 + 几何切换）
- `platform_ext.rs` — **壳内唯一的平台代码**：单实例门闩 + 窗口几何
  （小屏 / 竖屏）+ 置顶
- `tokens.rs` — 语义令牌，收口到 Tauri 壳的 CSS 变量取值
- `i18n.rs` — 复用 `src/locales/*.json`，编译期嵌入
- `supply.rs` — 本地 HTTP 供数桥（`/s/{档位}/{vid}` 在线流含 Range、
  `/c/{base64url}` 封面 HEIC 转码），nsh 的 `video` / `image` 只认 URL，
  这是唯一一条供数通路
- `player/` — 播放域（对齐 `src/features/player/` 的职责面）：
  - `mod.rs` — `Stage` 状态束 + 舞台装配（视频 / 悬浮层叠层）+ chrome
    状态机 + 键盘
  - `actions.rs` — 编排动作：换剧/切集/seek/发弹幕/档案装载/推荐流分页
  - `feed.rs` — 接口封装（推荐流 / 分集 / 简介 / 弹幕 / 评论 / 互动回显）
  - `controls.rs` — 全量控制栏 + 倍速/清晰度/弹幕设置/音量浮层
  - `mini.rs` — 小窗 / 竖屏紧凑控制条
  - `picker.rs` — 选集浮层（范围段网格 + 系列剧卡片）
  - `rail.rs` — 右侧互动栏（真实计数 + 点赞/收藏/追更/分享）
  - `info.rs` — 左下信息层 / 连播指示与横条 / 加载细条 / 错误条 / 加载面板
  - `danmaku.rs` — 弹幕层（进度驱动 + 泳道调度 + 四项显示设置）
  - `comments.rs` / `comments_view.rs` — 评论面板（状态与数据 / 渲染件）
  - `widgets.rs` / `icons.rs` — 可拖滑条等通用件 / canvas SVG 图标
- `app.rs` — 根组件：侧栏 / 顶栏 / 内容区

## 关于 `platform_ext` 的手写部分

nsh 0.1.1 的 Rust 面**没有窗口几何 API**：`DesktopCommand::SetFrame` 只在
`nsh/desktop` 脚本模块内部（`runtime::desktop::Window::set_frame` 是
`pub(crate)`），`AppView` 只交出 `hwnd()` / `view()`。而三壳的小窗语义是
「同一个窗口缩成 480×270 落到工作区右下角」，这只能原生做一次，所以这一段留在
壳里。

**但它不自己养 Win32 声明**：类型、常量、函数签名全部用 nsh 自己转出的
`nsh_platform_windows::windows`（windows 0.62，feature 由 nsh 开），nsh 升级换代
这里不改。等上游把 `setFrame` 开放给 Rust（或 `AppView::set_frame`），整个
`imp` 模块删掉即可——`enter_mini` / `exit_mini` 两个门面函数保持不动。

## 已知坑

- **起播前必须等渐进流注册**（`hongguo_stream::total_len` 非空）：播放器先到
  会吃 404，MediaEngine 判加载失败后画面定格且不恢复。
- 信号只能在引擎线程写（别的线程写会 `no reactive runtime` panic），所以事件
  泵用 nsh 的 `spawn` 而不是 service 的 runtime。
- `Image::resize_mode` 收的是**非响应式**值，`video` 的 `resize_mode` 才收
  `impl IntoReactive`——小窗切 Cover 只能挂在 `video` 上，封面背板固定 Cover。
- 窗口句柄靠平台反查（Windows 按类名 `NshWindow` + 本进程 pid，macOS 取
  `NSApplication.windows()` 第一扇），因为 `run_app` 不交出 `AppView` 所有权。
- nsh 0.1.1 是发布版，与 GitHub main 有 API 漂移（`Listener` 收 `&Event`、
  `VideoInfo` 平铺字段、`native_value()`）。升级要逐层改，别一次跳。
- `NSH_SCRIPT` 的值是**脚本文本**，不是文件路径——跑仓库里的脚本用
  `NSH_SCRIPT="$(cat crates/hongguo-nsh/scripts/smoke-play.txt)"`（`make
  nsh-smoke` 就是这么跑的）。传成路径不会报错，只是什么都不执行。
- 窗口关掉后进程才退出；`quit` 脚本动词是唯一的无头退出方式。
