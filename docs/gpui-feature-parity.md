# GPUI 壳 · 功能对齐清单（Tauri 基准）

> 2026-10-17 建立。前情：2026-10-14 布局对齐（`gpui-layout-parity-plan.md`，已实施）；
> 10-15/16 逐页对齐（排行榜/找剧/新剧/详情/收藏/点赞/历史/预约/下载/设置/合并/存储）。
> 本文只记**当时的剩余差距**与处理状态，逐项清零；每项都带代码证据，结论以此为准。
>
> 验收方式：`make dev`（Tauri）与 `make gpui` 可同时启动（数据根平级分离，
> `%APPDATA%/hongguo-downloader` vs `%APPDATA%/hongguo-gpui`，不撞库锁），逐页对照
> 页面布局 / 交互 / 状态（空态、加载、失败、未登录）。

## W1 追更（Tauri 有整页，GPUI 缺）—— ✅ 2026-10-17 全部落地（除 #6 选型）

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 侧栏缺「追更」入口（Tauri 13 项） | `app-sidebar.tsx:45` vs `shell.rs` | ✅ `pages/follows.rs` + `shell.rs` Dest::Follows |
| 2 | `/follows` 整页未建 | `home/mod.rs:745` 自述 | ✅ 双 tab / 批量取消 / 检查更新 / 清空 / 未登录态 / 空态 / 错误态 |
| 3 | 详情页「追更」按钮（仅连载中显示） | `series-detail-page.tsx:390` | ✅ `detail/header.rs`（series_status==0 才显示；乐观翻转+回滚） |
| 4 | 播放页互动栏「追更」项（仅连载中显示） | `interaction-rail.tsx:231-239` | ✅ `home/interact.rs`（meta series_status 判定；未登录 toast+去设置页） |
| 5 | 侧栏追更未读角标 | `app-sidebar.tsx:60,76` | ✅ 折叠/展开两种形态；FollowNotices 事件 + 已读变化同步 |
| 6 | 更新提醒 OS 弹窗 | `app-shell.tsx:53-64`（Web Notification） | ⏳ 选型待定（可能要新依赖，先报告再动）；当前：事件已进壳，追更页列表/角标实时刷新 |

## W2 创作者主页（Tauri 有整页，GPUI 缺）

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 详情页缺「出品方 / 演职员」行与点入 | `series-detail-page.tsx:298-312`；GPUI `detail/` 无 creator 字样 | 待做 |
| 2 | `/creator` 整页未建 | `src/pages/creator/components/creator-home-page.tsx`；api 函数在 `hongguo-api/src/danmaku/`（profile/works） | 待做 |

## W3 详情 / 播放语义缺口（代码自述）

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 详情页「云端观看历史兜底」未接 | `detail/mod.rs:237`、`liked.rs:385`（同族） | 待做 |
| 2 | 剧终接力跨季 / 换批语义未对齐 | `home/mod.rs:12`、`home/playback.rs:619` | 待做 |
| 3 | MFA 风控自动轮询未移植 | `gpui-review-checklist.md`「已知边界」 | 待做 |

## W4 壳级行为

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 侧栏账户区：Tauri 已登录点头像进设置页（退出在设置页账户卡）；GPUI 曾直接「退出登录」、无头像/昵称 | `app-sidebar.tsx:160-210` vs `shell.rs` render_sidebar | ✅ 2026-10-17（头像/昵称/点心进设置页；登录墙统一 toast+切设置页） |
| 2 | UI 偏好持久化：Tauri 持久化 `sidebarCollapsed`（默认折叠）/`aibotPopupOn` | `src/stores/ui.ts` | ✅ 2026-10-17（`ui_prefs.rs`）；`browseFiltersCollapsed` 仍为内存态（待补） |
| 3 | 更新通道：GPUI 设置页自述「暂未接入更新通道」 | `settings.rs:1196` | 待定（GPUI 未入发布管线，先确认是否要求对齐） |

### 本轮顺带修复（验证中发现）

- `screenshot` feature 构建破损：`b588e92` 给 `RankSubList` 加了 `background` 字段，
  `preview/fixtures.rs` 的构造没跟上 → `cargo check -p hongguo-gpui --features screenshot`
  直接报错（预览矩阵整条链断）。已补 `background: Vec::new()`。
- 播放页互动栏点赞/收藏/分享无回执：补齐 toast（已点赞/已收藏/分享链接已复制），
  登录墙统一为「toast + 切设置页」（Tauri requireLogin 同款）。

## W5 macOS 壳（Windows 已做，macOS 缺）

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 小屏落位 / 置顶只有 Win32 | `mini.rs:210` | 待做 |
| 2 | 隐身模式窗口藏显只有 Win32 | `home/incognito.rs`（非 Windows 分支降级） | 待做 |

## W6 播放器性能（对照 gpui-video-player）

- 参照 `https://github.com/cijiugechu/gpui-video-player` 提取可借鉴点（取流节奏 / 解码管线 /
  帧上屏路径 / 音频 / 倍速变调），出「优化项 + 收益 + 风险」清单后按序实施。
- 红线：不得引入需要外部安装的运行时（一个安装包直接可用）；新依赖先报告后动。
- 已知未结：倍速切换卡顿（hongguo-playback；历史遗留项）。

## W7 全量交互 / 布局对拍（持续）

- 顶栏分类 tab：Tauri = 固定顶栏 portal 常显；GPUI = 舞台顶部悬浮、随悬浮层显隐
  （`home/stage.rs` 注释自述为适配 TitleBar 拖拽区的有意形态）。**待用户裁决**是否改为
  常显顶栏（需要自绘标题栏拖拽区）。
- 其余按双壳并存逐页对拍补充；发现即登记。
