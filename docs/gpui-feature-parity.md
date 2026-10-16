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

## W2 创作者主页（Tauri 有整页，GPUI 缺）—— ✅ 2026-10-17 全部落地

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 详情页缺「出品方 / 演职员」行与点入 | `series-detail-page.tsx:298-312` | ✅ `detail/header.rs` render_creators（头像/名字/角色 pill；无 uid 仅展示不可点；头像随 meta 预取） |
| 2 | `/creator` 整页未建 | `src/pages/creator/components/creator-home-page.tsx` | ✅ `pages/creator.rs`（资料头独立降级 / 作品网格 3:4 卡 / 游标翻页 / 空态 / 失败重试；点击起播回首页沉浸流；返回回来源详情页） |

附注：`hongguo-api::detail` 补导出 `SeriesCreator`（纯导出，无行为变更——GPUI 预览 fixtures 需要构造该类型）。

## W3 详情 / 播放语义缺口（代码自述）—— ✅ 2026-10-17 全部落地

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 详情页「云端观看历史兜底」未接 | 原 `detail/mod.rs` 自述 | ✅ 三段决策（本地>3s → 云端>3s → 本地≤3s 仍算看到）与 Tauri continueIndex 同口径；列表页 (1,0) 起点经首页云端复核 |
| 2 | 剧终接力跨季 / 换批语义未对齐 | 原 `playback.rs` 自述 | ✅ `home/season.rs`（季号解析 + 下一季挑选）+ 接力链路（下一季 → 信息流下一条）；下载面板闸与滚过末集同款；`tests/season_relay.rs` 覆盖 |
| 3 | MFA 风控自动轮询未移植 | `gpui-review-checklist.md`「已知边界」（旧） | ✅ 复核确认**已闭环**：轮询在 service（`login_service/flow.rs:83` 起 `spawn_mfa_polling`），壳经事件泵接 Waiting/Failed/Success 三态（`home/auth.rs` on_login_mfa_event）；旧清单描述已过期 |

## W4 壳级行为

| # | 差距 | 证据 | 状态 |
| - | ---- | ---- | ---- |
| 1 | 侧栏账户区：Tauri 已登录点头像进设置页（退出在设置页账户卡）；GPUI 曾直接「退出登录」、无头像/昵称 | `app-sidebar.tsx:160-210` vs `shell.rs` render_sidebar | ✅ 2026-10-17（头像/昵称/点心进设置页；登录墙统一 toast+切设置页） |
| 2 | UI 偏好持久化：Tauri 持久化 `sidebarCollapsed`（默认折叠）/`aibotPopupOn`/`browseFiltersCollapsed` | `src/stores/ui.ts` | ✅ 2026-10-17（`ui_prefs.rs` 三项齐） |
| 3 | 更新通道：GPUI 设置页自述「暂未接入更新通道」 | `settings.rs:1196` | 待定（GPUI 未入发布管线，先确认是否要求对齐） |
| 4 | 主题切换：Tauri 顶栏三档（`theme-switch.tsx`：跟随系统/亮色/暗色，persist `hongguo-theme`，默认 auto）；GPUI 原为固定深色 | `src/stores/theme.ts`、`src/components/layout/theme-switch.tsx` | ✅ 2026-10-17（`ui_prefs.theme` + 设置页「外观」卡；auto 观察窗口外观实时跟随；显式档位钉住窗口外观）。**入口位置是有意偏差**：Tauri 在顶栏，GPUI 顶栏是平台拖拽区（自绘可点控件会被原生命中测试判成拖拽吞掉，见 `shell.rs` render_title_bar 注释） |
| 5 | 界面语言切换：Tauri 顶栏中/英切换（`ThemeSwitch` 同排的 Languages 菜单，`src/locales/` 两份文案） | `theme-switch.tsx:44-57` | ⏳ 待裁决：GPUI 全量文案是硬编码中文，做 i18n 是全仓文案扫一遍的大工程；先确认是否要求对齐 |
| 6 | 老板键：Tauri `boss_key_cmd.rs` 自述「GPUI 壳无此层（全局快捷键插件是 Tauri 侧能力）」——设置页无卡片、无注册 | `crates/hongguo-tauri/src/commands/boss_key_cmd.rs` 头注释；`boss-key-card.tsx` | ✅ 2026-10-17（`src/boss_key/`：accelerator 解析/捕获/冲突纯函数 + win32 自建消息窗口接 WM_HOTKEY；设置页「全局快捷键」卡；先注册后落库 + 冲突拦截）。真机验过保存/启动注册/合成按键藏显/冲突拦截四条路径。macOS 侧仅缺 FFI（设置时会明确报错，不做假生效） |

附注（本轮顺带修）：`ShowWindow` 在实体更新里调用会同步派发 WM_*，gpui 的窗口
过程要更新窗口状态 → 撞借用（实测日志刷「RefCell already borrowed」六连）。
老板键轮询与隐身轮询（`incognito_tick` 改返回 `IncognitoStep`）都已把窗口藏显
挪到实体更新之外；`mini`/`home/controls` 里同类「监听器内直接 SetWindowPos/
ShowWindow」的既有写法未动（已实测可用，属另一批次）。

附注（顶栏结构偏差，一并留给用户裁决）：Tauri 顶栏 = 固定常显的分类 tab 槽 +
置顶钮 + 主题/语言 + 窗口钮；GPUI 顶栏是组件库 TitleBar（整条为平台拖拽区），
只能放不可点内容——分类 tab 因此仍在舞台顶部悬浮层、置顶/主题入口分散在
舞台/设置页。要完全对齐需要上游支持「拖拽区内的可点控件」（gpui-kit
`TitleBar` 目前只会把整条标 Drag），或自绘标题栏并自己实现拖拽/双击最大化。

附注（主题默认变化）：本次后 GPUI 默认「跟随系统」（= Tauri 默认 auto 口径）；
本机 Windows 当前为浅色应用外观，所以 GPUI 壳开箱即浅色，想要深色在设置页
选「暗色」即可（此前是硬编码深色）。

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

## W6 播放器性能（对照 gpui-video-player）—— 2026-10-17 对照完成（见下）

对照对象：[cijiugechu/gpui-video-player](https://github.com/cijiugechu/gpui-video-player)
（已 clone 到本机临时目录逐层阅读）。**结论先行**：该项目的可迁移技法我们
已全部具备；其底座（GStreamer）需要系统安装运行时，按「一个安装包直接可用」
红线**不可引入**。

| 参考项目技法 | 对应证据 | 本项目现状 |
| --- | --- | --- |
| 逐帧 `RenderImage` 必须显式 `cx.drop_image` 回收，否则图集（GPU）只增不减（其 `docs/video-memory.md`：数百 MB→GB） | 它 `element.rs` 的 `paint_render_image` | ✅ 已实现：`home/mod.rs` 回收队列 + `stage.rs`「gpui 纹理只增不删」注释 + render 内 `window.drop_image` |
| macOS 走 `CVPixelBuffer` + `paint_surface` 绕开图集（IOSurface/Metal 零拷贝） | 其 `element.rs` macOS 分支 | ✅ 已实现：`FrameBuffer::CVPixelBuffer` 直通 surface（2026-10-13 实测定案） |
| 帧缓冲有界（`VecDeque` 裁到 capacity，防 UI 停滞时堆积） | 其 `video.rs` frame_buffer | ✅ 更优：`latest_frame()` 最新帧覆盖式握手（不堆积、天然丢旧帧）；解码侧另有各自有界队列 |
| 转换按“显示需要”出图（其 `yuv` crate 软件转换 + encode 面） | 其 `video.rs` 转换段 | ✅ 已实现：`nv12_to_rgba_bt709_scaled` 按舞台尺寸最近邻缩放 + 行并行 + 定点整数数学（`hongguo-media/nv12_rgba.rs`） |
| 倍速改采样/变速 | GStreamer `set_speed`（无 scaletempo 时音调会走样） | ✅ 更优：WSOLA 变速不变调（`wsola.rs`）+ 分级换速时钟（`audio.rs`：旧速率内容放完再换轴，防音画错位） |

**真机实测（本机，临时数据根，2.0 倍速，2026-10-17）**：

- `gpui-playback-prefs` 预置 `rate=2.0` → 实际生效：107.7s 的集 54s 播完（日志时间线核对）；
- 全程无「缓冲低水位」（仅起播预热 1 次）、无同步偏差告警（仅每集「第 0 帧」启动一拍 -0.4~-0.56s，为起播瞬态）；
- 帧转换 2.4–5.7ms/帧（1224×689 输出，1920×1080 源），每秒约 58 帧转换+重绘（满 60fps 节奏）；
- 连播切集（epoch=2/3）正常，无卡死。

**剩余候选优化（证据在手，收益/风险已评，暂不动）**：

1. 起播首帧"画面落后"瞬态（-0.4~-0.56s，仅第 0 帧）：可在首帧出画时把时钟 base 重锚到首帧 pts；影响面小、收益有限（用户感知约半秒声先画后），留待有实感反馈再做。
2. NV12→RGBA 的 SIMD 化（当前 2.4–5.7ms/帧 @1224×689）：仅在 4K 舞台/超大窗口下可能逼近 16.6ms 预算；届时可上 `safe_arch`/手写 SSE2；现在无收益场景，不加依赖。
3. 倍速卡顿（历史遗留项）：本次 2 倍速 100s 实测未见欠载/漂移复现；疑似已被「分级换速 + 帧转换下沉 + WSOLA」一组修复覆盖。**需要用户耳朵复核**：若仍卡，`RUST_LOG=debug` 抓 `[Playback][feeder]` 与 `[Playback][sync]` 日志定位。

## W7 全量交互 / 布局对拍（持续）

- ✅ 2026-10-17 Esc 分层补评论面板：面板开着时 Esc 先关面板、不退连播
  （Tauri comment-panel 自带 Esc 监听同序）。
- ✅ 2026-10-17 「回首页过期换一批」补齐：流数据 >5 分钟重开会话（FEED_STALE），
  在播剧 pin_hold 钉住继续播（Tauri FEED_STALE_MS / pinHold 同语义）。
- ✅ 2026-10-17 键盘语义核对：空格/←→(±5s)/↑↓/Esc 与 Tauri 同表（↑↓ 在连播
  锁定下切集、未锁定切剧，`step()` 内部分流）；GPUI 另有 F=沉浸全屏（Tauri 无此键，
  为真沉浸形态的配套入口，非分叉）。
- ✅ 2026-10-17 观看历史双模式（Tauri issue #10）：未登录 = 本地观看记录
  （本地必记 + 提示行 + 按剧批量删）；**并补齐播放期的历史记录链路**——
  此前 GPUI 只写播放档案、从不调 `history_service::record`（本地历史表恒空、
  登录态也永不上报云端；Tauri 由 `record_watch_history` 命令在上报节拍里写）。
  节奏按 Tauri 实际行为对齐：起播 ≥5 秒首报 + 暂停/切集/离开 force（其注释
  所称「每 60 秒一拍」的计数分支实际不可达，按实测行为对齐）。
- ✅ 2026-10-17 连播入口「观看完整短剧 · 全N集」横条 + 自动连播进连播态
  （Tauri player-page 同款；此前 GPUI 自然连播后滚轮/↑↓ 仍换剧、且沉浸流
  没有进入连播态的手动入口）。真机 4x 探针取证：左上「第2集」胶囊 + 横条退场。
- ✅ 2026-10-17 评论 @提及全链（候选弹层/插入提及段/rich_text tp=15 发送 +
  aibot_mention_context/AI 回复 5s 轮询与「思考中…」/正文提及染蓝 + 表情内联）。
  顺带修两处既有问题：评论输入框发送后从不清空（标志位无人消费）、评论面板
  铺到舞台底导致输入行被控制条压住点不到（按 Tauri bottom-56 + h-68% 几何）。
- ✅ 2026-10-17 详情页剧评发评框（Tauri ReviewComposer 同款：我的评分五星
  （悬停高亮/未选禁发）+ 表情网格 + 文本 + 发表剧评；十分制 = 星 ×2；
  成功回执 + 清空正文 + 重拉首页置顶）。取证开关新增 `HONGGUO_DETAIL_TAB`
  （剧评/相关推荐的无头截图入口），真机目检通过。
- ✅ 2026-10-17 剧评列表互动（Tauri ReviewList 同款）：♥点赞（剧评形态
  `digg_review`，乐观 ±1 失败回滚）/ 回复线程（悬停展开懒加载 + 展开更多
  回复 + 行内回复框；发送走剧评形态 `send_review_reply` commit_source=13，
  回复「回复」带 reply_to_reply_id，成功后本地追加 + 父剧评 replyCount+1
  并强制展开）/ 回复点赞（评论形态端点，Tauri 实测口径）/ 删除（AlertDialog
  确认；剧评 service_id=2、回复 service_id=4，本地摘除免重取）；正文按
  `[表情]` 内联渲染。真机数据目检：♥计数 + 回复钮渲染正确。
- ✅ 2026-10-17 找剧页搜索联想对齐 Tauri：
  - 键盘导航（Tauri 输入框 onKeyDown 同款）：↑↓ 高亮循环 / Enter 选中高亮条
    （无联想时提交搜索）/ Esc 收起 / 悬停改高亮；聚焦重开、失焦收起。
    派发语义已用进程内断言钉死（`tests/browse_suggest_keys.rs`）：gpui 的
    「绑定（action）先、key listener 后」顺序下，祖先的冒泡 `on_key_down`
    仍能收到这四个键，且单行输入里 ↑↓ 不挪光标（无 Tauri 的 preventDefault
    也能等价）。**打字输入走平台 WM_CHAR**，不受这条拦截影响。
  - 下拉形态对齐：绝对定位锚在输入框正下方（Tauri Popover
    side=bottom/sideOffset=6/锚宽同款）、`deferred` 提到卡片网格之上绘制、
    max-h 320 + 滚动、popover 底色。行内容补全：有封面的行前置 40×54 竖版
    缩略图（走既有 HEIC 转码封面链）、命中片段上 warm 令牌、带剧集的条目
    加粗、摘要行保留。
  - 顺带收编业务语义色：新增 `tokens.rs`（对齐 CSS 的 `--warm`，暗色档 =
    orange-400），找剧卡片热度行与榜单推荐语从 muted 灰改为 warm——Tauri 这两处
    用的是 `text-warm`，此前 GPUI 少了一层色。`hot`/`star` 待同法收编。
- ✅ 2026-10-17 业务语义色收编完成（`tokens.rs`：warm/hot/star，均按主题档
  取亮/暗档值）：详情页收藏/点赞/热度胶囊、剧评评分星与评分胶囊、榜单前三
  名次色、新剧卡片评分与日历行人气（原 `theme.primary` 白字）、预约评分星、
  点赞页红心。相关推荐卡同时补齐右下评分角标（星色 + 黑底半透明，Tauri
  `related-card` 同位），统计行改回「全N集 / 即将上线 · N次播放」。
  评论面板（评论/回复点赞）保持 hgplayer 皮肤固定色——该面板不随主题走，
  走令牌反而错档。
- 顶栏分类 tab：Tauri = 固定顶栏 portal 常显；GPUI = 舞台顶部悬浮、随悬浮层显隐
  （`home/stage.rs` 注释自述为适配 TitleBar 拖拽区的有意形态）。**待用户裁决**是否改为
  常显顶栏（需要自绘标题栏拖拽区）。
- 其余按双壳并存逐页对拍补充；发现即登记。
