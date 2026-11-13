# NSH 非播放器菜单差异审查（2026-11-13）

只读审查以 Tauri 生产源码与 NSH 生产装配为准；下表行号为修复前审查位置。固定数据预览、缺少真实账号操作记录，不据此判定功能未实现。

| 差异 | 源码证据 | 用户影响 | 状态/归属 |
| --- | --- | --- | --- |
| 个人片库播放没有使用统一续播入口 | NSH `library/actions.rs:79-116` 仅取本地 `series_last_position`，点赞还强制 `liked_vid`；Tauri `hooks/use-play-series.ts:16` 只点名剧，`home-page.tsx:316` 按本地/云端更新时间决定起点 | 收藏/点赞/追更/预约的播放可落到旧本地集或第一集；点赞卡可能强制回到以前赞过的集 | 已修：统一 `player::resolve_start`；内存库验证云端较新/本地较新、稀疏集号、删除过的点赞视频及一次性续播提示 |
| 预约搜索只覆盖已加载页 | NSH `library/mod.rs:131-147` 只滤当前合并记录，`287-301` 只提供手动加载；Tauri `reservation-page.tsx:52-56` 在搜索态主动补齐剩余页 | 在后续页的预约会暂时显示为“无匹配”，用户必须手工逐页补载 | 已修：搜索态自动补页，普通列表滚近底部续载；空中间页、失败停止/显式重试、游标与账号归属已定向验证 |
| 匿名预约页隐藏了整个工具栏 | NSH `library/mod.rs:89-106` 将内容放进登录门禁；Tauri `reservation-page.tsx:94-128` 保留两个标签和搜索，仅主体显示登录墙 | 匿名页缺少已上线/待上线选择、搜索框与原有布局 | 已修：Tabs/搜索在登录门禁外，管理仅登录可见；匿名读取0、退出隐藏旧行、换账号分页从0开始已验证；原生明暗2/2与四张图目检通过 |
| 移动端子榜没有横向滚动 owner | NSH `rank/presentation.rs:93` 的 Scroll 始终默认纵向，只将子 View 改为横排；Tauri `rank-page.tsx:106` 使用 `overflow-x-auto`，子项 `shrink-0/nowrap` | 窄窗长子榜不能按原交互横向滚到末项 | 已修：受约束的原生横/纵Scroll，轴key防残留偏移；三平台Headless及macOS1280/393实际首末、两向resize通过；另补focus-only分组键盘与可见焦点 |
| 点赞批量取消遇首错就结束 | NSH `library/actions.rs:65-69` 首次失败直接 return；Tauri `liked-page.tsx:60-72` allSettled 尝试全部 | 首次失败之后的已选条目没有尝试取消，用户难以确认哪些已经执行 | 已修：按既有顺序尝试全部项，成功项移除、失败项保留选择并汇总错误；fake 回归覆盖首错之后仍执行 |
| 多个页面缺少会话状态记忆 | NSH History `history.rs:23-24`、Browse `mod.rs:31`、Rank `mod.rs:22`、New `mod.rs:20`、Library `mod.rs:110-115` 使用页面局部 Signal；Tauri 对应 useSessionState | 页面被 Back 移除后重新进入/Forward，筛选、关键词或频道会重置；不是所有 retained route 场景都会丢失 | 已接入值快照、session-owned 数据模型和真实 Scroll/List 恢复；个人库30项、新剧9项及 Back/Forward 定向回归通过，Browse/Rank 实际原生返回流程通过 |
| 应用内更新安装未实现 | NSH `update.rs:48-54` 发现新版后只外开 Releases；Tauri `features/update/update-dialog.tsx:37-53` 有下载/重试/安装动作 | NSH 用户无法在应用内下载与安装更新 | 明确未实现；需要 NSH 资产 feed，不能直接安装 Tauri 资产，也不能标为“缺账号待验” |

## 已接线，不能误报成硬禁用

- 找剧：筛选、折叠、真实输入联想、回填或进详情、搜索/退出搜索、续载均有生产回调。
- 排行榜：主分类、子榜、总榜筛选、月榜历史、重置、续载均有生产回调；移动横滚差异单独列在上表。
- 新剧：推荐/日历、频道、日期、分页、详情、在线播放与登录后预约均有真实 API/统一播放调用。
- 历史：匿名本地/登录云端读取、三标签、搜索、续播、批量删除和确认已接真实服务。
- 收藏/点赞/预约：加载、筛选/搜索、选择、单项/批量取消、详情与播放已接线；上表记录具体语义差异。
- 下载：启动/暂停、重试、选失败、扫描、停止、删除（含文件确认）、桌面定位/手机导出均有真实服务/宿主回调。
- 合并：候选、输出名、模式/预检、启动、任务删除与产物定位/导出已接线。Quick 的禁用来自实际预检，不是占位。
- 存储：容量读取、按剧搜索与删除、全部删除确认均已接共享服务。
- 设置：目录（桌面）/私有目录（手机）、命名、并发、代理测试、播放开关、转码检测、快捷键/老板键（桌面）、缓存、登录/退出和保存均有真实回调。

检索到的 `disabled(true)`、空回调多数位于测试/固定数据预览；生产禁用主要来自 busy、空列表、预检和真实能力，不能据检索命中直接报缺陷。已实现功能在 Windows 的真实运行，以及真实账号写操作是否验收，需由对应平台/账号验证记录判断，本次没有执行这些操作。

本轮个人片库定向验证为 `cargo test --lib library::` 26/26，all-targets strict 0；
原生 `ReservationsFollowup.test.tsx` 2/2（28.6秒），JUnit 为
`target/nsh-followup-20261113/reservations-keyboard-native-results.xml`。
匿名和过滤结果明暗四图均在同目录，未执行真实账号登录、删除或播放写操作。
前两轮在搜索框仍是原生 firstResponder 时触发了被过滤的 fixture 热键；失败日志保留，
最终用真实 Tab 移出编辑框，未修改生产输入快捷键策略或放宽功能断言。
旧影视帧缺失产生的 image 警告不影响匿名页面；本次不把带灰色封面占位的过滤图当影视封面验收。

Rank 定向再发现两项真实原生问题并修正：普通按钮无分组方向键；方向键可移焦点后焦点仍在滚动视口外。
现用公开 ToggleGroup/Toggle 和 wrapper `ElementRef.on_focus_in`，只按实际bounds揭示焦点，不改SDK。
resize空白红例由trace确认同原生Scroll623在横纵切换时保留另一轴偏移；按轴key重建修复。
原生 `view memory finishing` 五例5/5包含Rank三例与Browse明暗两例，
并实际验证Rank主榜400滚位返回后第7项可点击并进入其详情；灰封面的完整外观待恢复ignored帧后同用例补拍。
恢复ignored测试帧后，同五例纯截图复跑再次5/5（1m39s）通过，
`view-memory-visual-native.log` / `view-memory-visual-native-results.xml` 保留第二轮结果；
实际加载图片的窄横栏/桌面纵栏首末、可见02焦点和主榜返回同滚位已目检。
这两轮均未操作真实账号；Mac测试实例在结束后全部退出。

## Tauri 会话字段清单

- Browse：filters 的 genre/theme/role/epoch/sort/gender/onlineTime/duration/creationStatus、keyword、submitted；另 browseFiltersCollapsed 属于 UiStore 跨启动持久化，默认 false。
- Rank：selected=`all`、sub=`ranklist_hot_sc`、panel/background 为空。
- New：gender=2、view=`recommend`、Calendar date 为空。
- History：tab=`all`、query 为空。
- Collections：query 为空、typeTab=`all`；Liked：query 为空。
- Reservations：online=true、query 为空；Follows 标签已在本轮提醒功能中由 Session 持有。
- 滚动：browse/history/collections/liked/reservation；rank 在切换 sub/panel/background 时归零；new 按 gender 分开保存。
- Tasks 选择、Merge 草稿、Storage 搜索、Settings 未保存草稿是 Tauri 页面本地 state，不应当被误做会话持久化。

会话值快照只保存筛选/草稿/滚动等值，不保存 reactive handle、请求响应或账号数据。
列表的数据模型按固定页面名惰性创建于窗口会话 owner，关闭会话即销毁；
请求依旧由实际路由、身份和业务 revision 驱动，身份变更清分页并检查结果归属。
页面元素、原生引用、管理/选择状态及操作闭包仍归当前页面 owner，禁止放入模型表。
滚动绑定真实 Scroll/virtual_list，挂载时恢复；筛选/身份 scope 变化即时回顶，分页不重置位置。
