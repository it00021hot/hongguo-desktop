# GPUI 壳 → NSH Rust 壳：工作量评估与模块复用清单

（2026-10-21 评估；前提：系统播放器方向已定、弹幕数据源问题单列不计入。依据：hongguo-gpui 41.4k 行逐目录拆账 + hg-nsh / hongguo-nsh-demo 两个 demo 的实测结论。）

## 总判断

**后端零行改动，播放主链路已通（demo 即证据），新工作集中在 UI 层约 2.3 万行的重写**。NSH Rust 声明式组件比 GPUI 命令式视图短，实际新写量小于被替换量。单人量级估算：**主干可用 3-5 周，全功能对齐 6-9 周**。最大不确定项：Windows 宿主（第二技术闸门）、悬浮小窗模型。

## 一、原样复用，零改动

红线保证 service 无 UI 依赖，换壳不伤内核（demo 已在用）：

- hongguo-service 九域（`AppStateInner::default()` 内存库 + 静态设备免登录；正式壳接真实 Store 即可）
- hongguo-api（推荐/详情/搜索/弹幕/评论/登录…14 域）、hongguo-stream、hongguo-protocol
- hongguo-store、hongguo-common、hongguo-signer、hongguo-video、hongguo-media*

## 二、demo 已建成，直接搬

| 资产 | 出处 |
|---|---|
| 本地解密供数桥（tiny_http + `respond_stream` + `stream_pool`） | hongguo-nsh-demo `src/stream.rs` |
| `background()` + 信号桥接模式（替代 bridge.rs/events.rs 两件套） | hongguo-nsh-demo |
| 车道占用调度 + 到期合批回收 + FPS 统计 | hongguo-nsh-demo |
| `NSH_SCRIPT` 验收法（tap/shot，替代 make gpui-preview） | 两 demo 均已用 |

## 三、换实现（更薄）

| GPUI 壳现有 | NSH 方案 |
|---|---|
| cover_image_cache/cover_asset/cover_img 自建图片缓存 | `image` 元素原生加载 URL，平台自带缓存 |
| video_backend.rs + 播放引擎集成 | `video` 元素（系统播放器） |
| i18n.rs | zh-CN/en-US JSON 资源复用，访问器重写 |
| tokens.rs | nsh theme/design tokens |
| bridge.rs（oneshot 桥）+ events.rs（事件泵） | `background()` + 信号泵（`ServiceEvent` 总线一个转发任务） |

## 四、直接删（随系统播放器方向退场）

hongguo-playback（3.9k）、hongguo-player-*（3.2k）、player-api、vendor/gpui-pre-apple、vendor/gpui-pre-windows、壳内 av_spike（0.9k）、video_backend.rs、preview/（1.9k）。

## 五、要新写的：41.4k 行壳的构成

| 块 | 现量 | 说明 | 估时 |
|---|---|---|---|
| 壳骨架：窗口/导航/事件泵/i18n/令牌 | shell 2k + 根散文件 ~2k | nsh 单窗 + NavStack + 信号泵；**小窗/单实例待验证**（nsh 独立应用单窗，悬浮小窗需 AppView 嵌出自管 NSWindow/Win32 窗口） | 1-1.5 周 |
| 播放域：controls/playback/stage/comment_view/弹幕 | home 13.7k | 播放/进度/切集/叠层控制栏/弹幕渲染已通；剩评论面板、清晰度、倍速、横竖屏、小窗 | 1.5-2.5 周 |
| 内容页 13 域 + detail | pages 13.1k + detail 3.4k | `list_common`+`scroll_load` 已抽象「列表+分页」，NSH 做一个泛型列表组件后每域变薄 | 2-3 周 |
| 设置 | 1.9k | 表单类直接翻 | 0.5 周 |
| 登录 | service 现成 | QR（image + 轮询信号）+ 短信 UI 新写 | 0.5 周 |
| Windows 复制 | — | nsh Windows 宿主 + MediaEngine + 供数桥，**未验证** | 0.5-1 周 |

## 六、分期路线（每期结束都可用）

- **P0（已完成）**：demo = 播放 + 控件 + 弹幕渲染，闸门全过
- **P1 主路径**：推荐流 + 详情 + 搜索 + 播放页完整化——无账号可用的「电视机」
- **P2 账号域**：登录、历史、收藏、追更（service 现成，纯 UI）
- **P3 长尾 + Windows**：设置/任务/创作者/合集/小窗；Windows 复制
- **替值线**：P3 完成 + 一个发版周期稳定 → 删 GPUI 壳与播放引擎、回收 vendor patch

## 七、先手棋

1. **Windows spike 优先**（唯一能让总量翻倍的未验证项，见 [windows-testing.md](windows-testing.md)）
2. **小窗选型放 P1 前**：嵌出多窗 vs 窗口内悬浮层，影响壳骨架窗口管理设计
