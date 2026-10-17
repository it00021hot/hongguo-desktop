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
| 7 | 竖屏播放：Tauri `enter/exit/fit_portrait_screen` + `set_portrait_full` 与 `use-portrait-window` 的竖条形态；GPUI 无 | `window_cmd.rs:230-370`、`use-portrait-window.ts`、`MiniScreenControls variant="portrait"` | ✅ 2026-10-17（`src/portrait/`：几何纯函数 + Win32 落窗；控制栏竖屏钮；竖屏紧凑条；侧栏藏/顶栏留；F→铺满、Esc 铺满→竖条；离开播放页先退竖屏）。真机验过 1280×840→388×840 与铺满 642×1392。macOS 侧只缩不挪（Apple FFI 后补，与小屏同缺口） |
| 8 | 退出确认：Tauri `CloseRequested` → 前端确认框（自绘 ×/Alt+F4/任务栏关闭都问一句）；GPUI 关窗直接退 | `crates/hongguo-tauri/src/lib.rs:107-113`、`app-shell.tsx:189-194` | ✅ 2026-10-17（`Shell::install_close_guard`：弹「确定要退出吗？/ 退出会中断正在进行的下载任务…」，取消复位闸门、弹过还来放行；真机投 WM_CLOSE 验过窗口未被关掉、文案与两键同 Tauri） |
| 9 | 单实例：Tauri `tauri-plugin-single-instance`（第二次启动前置已有窗口）；GPUI 第二次启动撞库锁 → 报错退出（用户视角「点图标没反应」） | `crates/hongguo-tauri/src/lib.rs:38-40,288-295` | ✅ 2026-10-17（`src/single_instance.rs`：命名互斥体 + 命名事件；第二进程置位即退、**早于开库**；首实例轮询消费 → 显示 + 还原最小化 + 聚焦。真机 A/B 双启验证） |
| 10 | 看完自动删除：设置页开关 `autoDeleteAfterPlay` 在 Tauri 播放到底时真删本地成品文件（`player-page.tsx:409-420` + `delete_episode`）；GPUI 的同一开关**无任何消费方**（死开关） | `crates/hongguo-gpui/src/home/playback.rs`（改前全仓仅设置页引用） | ✅ 2026-10-17（`advance_after_end` 顶部接上，同 Tauri 顺序：先删再连播；复用 service `cleanup::delete_episode`。真机：预置 completed 任务 + 探针文件，4 倍速播到底 → 文件被删 + 提示条 + 连播照常） |
| 11 | 本地成品播放：Tauri「本地成品优先，其次在线流」（`play_service::local::resolve_play`，已下载的集走 `hongguo-local://`，离线可播/秒开）；GPUI 起播链只走 `online::prepare`，全仓无一处读 `completed_path` | `crates/hongguo-service/src/play_service/local.rs` vs `crates/hongguo-gpui/src/home/playback.rs` | ✅ 2026-10-17（引擎加字节层 `ByteSource`（Online/Local）+ `PlaySource` 公开面；壳起播先查 `completed_path` 且文件确在 → 本地播放，档位表为空；陈旧记录回落在线流）。真机：640×360 本地 HEVC 样本 → 「头部就绪 0ms / 640x360 1ch」+ 彩条截图；播完连播回在线流同轮验证 |
| 12 | 同剧切集不闪封面：Tauri `use-frame-hold` 的显隐真值表（切集定格上一帧顶住、切剧立刻走封面）；GPUI 一切换就清帧 → 每次切集闪封面（Tauri 已修、GPUI 带着） | `src/features/player/hooks/use-frame-hold.ts`（含 `use-frame-hold.test.ts`）vs `home/stage.rs` video_ready | ✅ 2026-10-17（`hold_last_frame`：play_target 按同剧判定保留/清帧；新集首帧、失败、离开首页落下）。真机：切集间隙拍到定格末帧、切剧拍到新剧封面（`HONGGUO_SHOT_PANEL=next-series` 探针） |
| 13 | 缺 HEVC 扩展的安装引导：Tauri `HevcInstallCard` 两档（失败后的居中主卡 + 提前告知的底部悬浮卡，附 `hevc-session` 会话记忆与一键安装 service）；GPUI 选流虽已提前降档 H.264，但对用户完全沉默 | `hevc-install-card.tsx` + `hevc-session.ts` + `capability_service::install_hevc_extension` | ✅ 2026-10-17（`home/hevc_prompt.rs` 做 proactive 一档：底部悬浮卡 + 一键安装 + 暂不安装会话记忆；文案按 GPUI 实际形态改写——不是「有声音没画面」而是「当前是降档 H.264」）。真机 `HONGGUO_NO_HEVC=1` 拍到卡片与两键 |
| 14 | 顶部 2px 加载细条：Tauri `loadbar.tsx` + `hg-loadbar`（切换在途/首帧未出/缓冲中亮，30% 胶囊 1.1s 循环滑动）；GPUI 只有中央 spinner 与右上「缓冲中」角标 | `loadbar.tsx` + `index.css:252-273` | ✅ 2026-10-17（stage 顶部同款细条；用 gpui `Animation` 驱动循环滑动，不依赖播放帧率）。真机：对切换间隙逐帧扫顶部像素行，只有间隙那一帧命中亮条，间隙外 14 帧全无 |
| 15 | 暂停时的快捷键提示：Tauri `keyboard-hint.tsx`（38% 高度居中胶囊，小屏/竖屏/错误态不渲染）；GPUI 无 | `keyboard-hint.tsx:6-16` | ✅ 2026-10-17（stage 同款胶囊，文案取 Tauri 同一条静态提示）。真机 `HONGGUO_SHOT_PAUSE=1` 抓帧在位 |

### 已闭环的能力边界（原「已知边界」逐项清零）

- **H.264 播放（无 HEVC 扩展机器的在线降档流 / 合并产物 / 任意 avc1 文件）**
  —— ✅ 2026-10-17 引擎级补齐（`fdb4ed1` + `c9db36b`）。原边界：Tauri 的
  `<video>`（WebView2/Chromium）H.264 天然可播，缺扩展时选流会优先挑 H.264
  （`DecoderCapability::NoHevc`）并直接播；GPUI 自研引擎的解码器与轨道闸门
  都是 HEVC 专用（`HevcDecoder` + `open_stream` 的「只接 HEVC 轨」），
  H.264/合并产物在 GPUI 里播不了。现实现：
  - `hongguo-media::h264`：avcC 记录解析（SPS/PPS 裸 NAL + 长度前缀字节数 +
    整条 avcC 载荷），与 `hevc` 模块同构；
  - 平台契约：`DecoderInit` 带 `codec` 与 `sequence_header`（hvcC/avcC 归一），
    解码器类型 `HevcDecoder` → `VideoFrameDecoder`（两端同名，不再是 HEVC 专用）；
  - Windows MF：解码 subtype 按编码选（`MFVideoFormat_H264` / `_HEVC`），H.264
    走同一套 Annex-B 转换与带内参数集保险；**按 `MF_MT_GEOMETRIC_APERTURE`
    裁编码 padding**——实测 x264 把 360 宽码到 368，MF 按编码尺寸出帧，
    不裁时画面右缘一条绿边（本机 ffmpeg 造 avc1 样本复现并修掉）；
  - macOS VT：`CMVideoFormatDescriptionCreateFromH264ParameterSets` 建会话
    （转码链显式按 HEVC 建，行为不变）；本机无 Mac，双目标交叉 clippy 过，
    真机播放待测；
  - 诊断：`头部就绪` 与 `批量预取命中` 日志带编码（HEVC / H.264）。
  - 真机证据：ffmpeg 造 360×640 avc1 本地成品 → 「本地成品命中」→「选中解码器
    Microsoft H264 Video Decoder MFT（H.264）」→ 首帧 360x640、截图无绿边；
    在线 HEVC 路径同轮回归正常。proactive 安装引导（W4#13）保留——在线测试
    账号的 1080 档实测只有 bytevc1/HEVC 系流（`HONGGUO_NO_HEVC=1` 下选流
    走末选兜底），缺扩展机器的在线兜底仍要靠它。

### 下一步（本轮发现，尚未开工）

- `PlayRequest.file_path` 直传路径那条分支（Tauri `resolve_play` 的第一段：
 调用方直接给成品路径，合并产物/指定文件用）GPUI 尚无对应入口——先用
 `completed_path`（队列命中）覆盖了常规路径，等确认合并产物那条链谁在用再补。

## W8 并排对拍（真实 Tauri 壳 vs GPUI，2026-10-17）

验收口径「测试效果和 Tauri 一致、可同时启动验证」的落地动作：**不再只对代码，
直接对渲染结果**。

对拍方法（可复现）：

1. 本 worktree 编译 Tauri 壳：`cargo build -p hongguo-desktop`；
2. `pnpm dev`（Vite 1420）+ `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`
   启动 `target/debug/hongguo-desktop.exe`；
3. `node scripts/cdp.mjs shot <route> <png>` 抓 Tauri 各页；播放器状态用 CDP
   `Input.dispatchMouseEvent`（可信输入）点画面暂停后截图；
4. GPUI 用 `HONGGUO_PAGE=… / HONGGUO_DETAIL=… / HONGGUO_SHOT_PAUSE=1` 抓同位截图；
5. 同尺寸（1280×840）逐页目检 + 关键几何用 DOM `getBoundingClientRect` 对齐。

本轮到位的差异（分两提交：`5599d94`、`f0398d2`）：

| 页面 | 差异 | 处理 |
| --- | --- | --- |
| 顶栏 | tab 槽中心 646（绝对层按内容盒定位，bar 有 12px 左内边距） | 去掉 bar 左内边距 → 中心 639.5（窗口中线 640） |
| 找剧 | 搜索框被多余的 `flex_1` 占位块挤到 ~512px；卡片固定 196px | 删占位块（输入框回到 flex-1，~1010px）；按 Tauri `SeriesCardGrid` 断点算列数（2/3/4/5/8），1280 下卡宽 221（Tauri 220），封面 3:4 |
| 新剧 | 频道胶囊贴右 | 改贴左——Tauri 代码写 `justify-end` 但 ToggleGroup 是 inline-flex 宽度收缩、真机渲染贴左，按真机截图对齐 |
| 详情 | 播放钮「立即播放/继续播放 第N集」无图标；下半区 tab 是下划线样式 | 文案改 Tauri 原文「从第 1 集开始 / 继续看第 N 集」+ Play 图标；tab 改 Tauri `TabsList` 分段控件（muted 底槽 + active 抬起胶囊） |
| 设置 | 多一张「外观」卡（旧顶栏放不了控件时的替代入口）；登录钮非实心 | 删外观卡（顶栏外观菜单已就位，Tauri 设置页无此卡）；登录钮 primary |
| 控制栏 | 发送钮 ghost 纯文本；弹幕开关带常驻底色；全屏用斜向 icon | primary 胶囊；自绘透明底开关钮；Maximize/Minimize 四角 icon |
| 下载（`7de2324`） | **功能缺口**：GPUI 下载入口只有右侧任务面板 + 「下载全剧」；Tauri 是底边「下载到本地」选集浮层（逐集勾选/全选反选/前 N 集/区间语法），任务列表在 /tasks | 新增 `home/downloads_range.rs`（区间语法 1:1 + 5 条用例）+ 壳层整窗底边浮层：8 列集号网格、已选 N 集、区间输入即解析、格式化选集回填、开始下载 → 入队 → toast → 切任务页；原右侧任务面板退场（Tauri 无此面板） |
| 选集浮层（`19aab4e`） | 定位钉在舞台右缘（Tauri 贴「选集」按钮右缘向上弹）；无系列剧计数角标；多一行 Tauri 没有的提示 | 改挂按钮 wrapper（relative + deferred 置顶）、452×max440；系列剧红色计数角标；删提示行 |
| 剧评（`19aab4e`） | 单行输入框；评分摘要裸文字；条目「N分 + 相对时间」 | 3 行 Textarea（Enter 发表 / Shift+Enter 换行）；边框评分卡（30px 大字 + 五星 + 人数 + 标签统计胶囊）；条目五星 + 绝对日期 + 评分后缀「观看N小时后点评」 |
| 相关推荐（`43e6f91`） | 116px 卡（9 列）；单行标题；「全N集 / N次播放」；未上线卡无预约钮 | 145px（1280 下 7 列，Tauri 同宽）；两行标题；「共N集 / N万播放」；补预约钮（离线+在线预约表查初值、乐观翻转失败回滚、成功 toast、点击不跳详情） |
| 登录（`bff9f2a`） | 300 宽、无说明/字段标签/风控注记/关闭钮 | 512 宽 + 「登录红果账号」标题与说明 + 手机号/验证码字段标签 + 风控注记 + 右上 ✕；发码（手机号 11 位）与登录（手机号 11 位且验证码 ≥4 位）门槛同 Tauri |
| 弹幕设置（`90fc775`） | 四行滑杆结构一致；文案「字号」 | 改 Tauri 原文「字体大小」；新增 `HONGGUO_SHOT_PANEL=dm-settings` 取证开关 |
| 排行榜（`8137575`） | 根节点多一层 `p_4`（Tauri rank 页无内边距），整页左右各缩 16px | 去 `p_4`、gap 对齐 `gap-4`：子榜 x≈60（Tauri 56）、内容行 218..1276（Tauri 216..1270） |
| 列表页选中态（`1275aeb`） | 选中胶囊用 `secondary`（深色主题深灰底白字），Tauri 是 `bg-primary text-primary-foreground`（白底深字） | 统一改 `primary`：历史/收藏/预约筛选、排行榜与首页顶栏 tab、新剧视图/频道/日期胶囊 |
| 空态位置（`3da62ae`） | `flex-1` 垂直居中（内容落在页面中部） | Tauri `Empty` 是流内块（`py-16`，顶部对齐）；收藏/点赞/预约/历史（含筛选空）/下载任务/追更六页统一 |

同轮目检一致的页面：排行榜 / 下载任务 / 存储 / 合并 / 首页沉浸播放器
（信息块、互动栏、连播横条、快捷键提示、控制栏顺序与几何）。

形态目检：竖屏（388×840，顶栏只留右侧钮、无 tab 槽/侧栏、视频居中留黑边）与
小屏（480×270 纯播放器 + 紧凑条）与 Tauri 同构。

已知非差异：倍速/主题/筛选折叠等**持久化状态**两边各自保存，截图可能不同
（如 Tauri 存了 2x、GPUI 默认 1x）；语言入口仍缺（W1#5 待裁决）。

### 顺带修（本轮验证中发现）

- **全部确认框缺「取消」键**：组件库 `AlertDialog` 的取消钮默认不显示
 （`show_cancel=false`），而全仓 10 处只设了 `cancel_text` —— 确认框实际只有
  确认键，Esc 是唯一取消通道（与 Tauri 的「取消 + 确认」两键不一致）。逐处
  补 `show_cancel(true)`：退出登录/退出应用/删除剧评回复/批量取消收藏/批量删
  历史·点赞·预约·追更/删合并记录/删全部下载文件。

附注（本轮顺带修）：`ShowWindow` 在实体更新里调用会同步派发 WM_*，gpui 的窗口
过程要更新窗口状态 → 撞借用（实测日志刷「RefCell already borrowed」六连）。
老板键轮询与隐身轮询（`incognito_tick` 改返回 `IncognitoStep`）都已把窗口藏显
挪到实体更新之外；`mini`/`home/controls` 里同类「监听器内直接 SetWindowPos/
ShowWindow」的既有写法未动（已实测可用，属另一批次）。

### 顶栏结构（原「留待裁决」项，2026-10-17 已自绘落地，`aa54522`）

自绘 44px 顶栏（Tauri `app-shell` header 逐项对齐）：

- 分类 tab 槽：首页/排行榜/新剧的分类 tab 从舞台悬浮层与页面上移入顶栏
  （Tauri `TopBarTabsPortal` 同位；竖屏隐藏槽，Tauri 4376496 同款）；
- 右端：置顶（与小屏紧凑条共用状态）、外观三档菜单（跟随系统/亮色/暗色，
  与设置页同一段 `ThemePref` 实现）、窗口钮（Min/Max/Close 走
  WindowControlArea 交给 OS 原生处理——关闭照旧过 WM_CLOSE 退出确认守卫；
  macOS 不画窗口钮、给原生红绿灯留 76px）；
- 拖窗/双击：gpui 的 `start_window_move` 在 Windows 是 trait 默认空实现
  （gpui-pre-windows 未覆写），只能走 win32 `ReleaseCapture +
  WM_NCLBUTTONDOWN(HTCAPTION)`，且必须挪出实体更新执行（事件当口同步派发
  WM_* 会撞 gpui 借用——隐身 ShowWindow 同款坑）；双击走 win32 切最大化；
- 控制栏里的置顶钮随之退场（Tauri 正常播放控制栏没有这一项）。

为什么组件库 TitleBar 做不到：它为 Windows 把整条 bar 标成
`WindowControlArea::Drag`（OS 级 HTCAPTION），而 gpui 的 window-control
命中回调按插入（绘制）序返回第一个命中——父级 Drag 永远压过子控件；自绘后
不给 bar 标 Drag、拖拽走手动路径，控件点击才进得来（`tests/toolbar_click.rs`
扫描式回归钉住）。

真机取证（真实系统点击，逐项）：切 tab（源切换日志 + 截图）、置顶（日志）、
外观菜单开合与暗色生效（截图）、窗口钮最小化/最大化（IsIconic/IsZoomed）、
拖窗（窗口位移 122,79）、双击最大化、关闭钮弹退出确认框。

有意保留的差异：顶栏左侧不画应用名（Tauri 顶栏同样为空）；设置页「外观卡」
作为主题第二入口保留（Tauri 只在顶栏）；语言切换（W1#5）仍是待裁决项。

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
- ✅ 2026-10-17 匿名历史源切换（对齐 Tauri `88f5edf`）：首页的观看历史源
  按登录态取——登录读云端（跨端续播），匿名读**本地记录**（此前匿名也发
  云端查询，拿回设备级历史行当续播依据/推荐种子）。真机两轮：首轮跑完本地
  落 1 条，次轮日志「匿名：历史源取本地记录（1 条）」。
- ✅ 2026-10-17 小屏铺满裁切（对齐 Tauri `eadb9fe`「小屏去黑边」）：小屏
  形态视频 `cover` 拉满 480×270（竖屏剧在小窗不留黑边），其余形态仍
  `contain`；转换端加 `cover_fit`、显示端按形态选 `ObjectFit`。真机：本地
  360×640 样本进小屏，彩条贴满窗口。
- ✅ 2026-10-17 相关推荐跳详情补 prefill（对齐 Tauri `a21ddf2` /
  `relatedDetailSearch`）：`open_related` 必带 title/cover/tags/desc 快照，
  未上线剧走「即将上线 + 预约」降级视图而不是整页报错；`HONGGUO_SHOT_PANEL=
  related-upcoming` 真机拍到降级视图。
- 🔍 2026-10-17 浏览位置保留（Tauri `use-scroll-restore` sessionStorage 口径：
  去播放再回列表保留位置）——GPUI 用 `HONGGUO_SHOT_PANEL=scroll-return`
  （滚一把 → 往返首页 → 再抓帧）核对：前后两张截图逐像素同位置，
  gpui 的元素状态按 ElementId 跨挂载保留，**无需对齐动作**。
- 顶栏分类 tab：Tauri = 固定顶栏 portal 常显；GPUI = 舞台顶部悬浮、随悬浮层显隐
  （`home/stage.rs` 注释自述为适配 TitleBar 拖拽区的有意形态）。**待用户裁决**是否改为
  常显顶栏（需要自绘标题栏拖拽区）。
- 其余按双壳并存逐页对拍补充；发现即登记。
