# 已知存量问题清单

> 来源：2026-10 重构（docs/refactor-plan.md P0~P6）过程中逐文件排查时记录。按纪律「只记录不修」的项都在这里；
> 重构中确认的行为 bug 已当场修复，见文末「已修复」。行号会漂移，定位一律用 符号名。

## 一、行为类疑点（改不改需要线上/产品口径，别盲改）

| #   | 位置                                                                    | 现象                                                                              | 影响                                                                         |
| --- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| B1  | `domain/api/reservation` `fetch_reservations` 翻页循环                  | 只查 `has_more/next_offset>0/非空页`，不校验 offset 是否前进、不按 series_id 去重 | 服务端翻页异常回重复页时最多堆叠 19 页重复条目，靠 finalize 收口             |
| B2  | `domain/api/rank/mod.rs` `parse_cell_selector`                          | 用中文文案 `show_name != "总榜"` 决定是否保留空 id 选项（前端清除筛选依赖它）     | 服务端改文案该判定即静默失效；代码注释自知                                   |
| B3  | `domain/api/recommend/mod.rs` `resolve_tab_config`                      | tab 配置进程缓存无失效路径，cell_id/bookstore_id 硬编码假设                       | 服务端运营调整 tab 配置时只能重启恢复（known_tab_config 兜底同为硬编码）     |
| B4  | `domain/api/rank/mod.rs` `fetch_rank_ex`                                | 相邻页重叠 10 条，后端不去重，去重职责全在前端                                    | 任一调用点遗漏去重即出现重复条目                                             |
| B5  | `domain/api/interact/mod.rs` `collect_series`                           | 请求体 `"is_cancelled": false` 恒定，不随取消收藏联动                             | 若服务端认这个字段，取消收藏可能不生效（注释称照抄抓包，可能服务端本就如此） |
| B6  | `domain/api/interact/mod.rs` `fetch_bookshelf`                          | 时间字段候选序 `["collect_time","collect_time_ms","create_time"]`                 | 秒/毫秒字段并存时会先取到秒值被当毫秒用（排序错乱）                          |
| B7  | `utils/json.rs` `int_field`                                             | 超 i64 大数静默归 0（discover/detail/history 原三处行为，C9 收敛后口径统一保留）  | 字段数值异常时拿到 0 而非报错                                                |
| B8  | `features/player/hooks/use-mini-window.ts` + `use-playback-progress.ts` | 信息流进小屏会双写进度：enterMini 强落一次 + 卸载补写 effect 再落一次             | 幂等无害，多一次冗余 IPC                                                     |
| B9  | `pages/rank/components/normalize-tabs.ts`                               | 严格说非纯函数：内部调 `t('rank.filter.title')`（随语言变）                       | 无 hooks/状态，可接受；i18n 切换后需重挂载才刷新面板标题                     |

## 二、测试质量（形同虚设 / 名不副实，修了能提升置信度）

| #   | 位置                                                                                                     | 现象                                                     |
| --- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| T1  | `domain/api/interact/mod.rs` 测试 `digg_action_types_match_capture`、`shelf_operate_types_match_capture` | 用 `if true {3} else {4}` 断言字面量，不触达真实代码路径 |
| T2  | `domain/api/login/mod.rs` 测试 `parses_upsms_states`                                                     | 名字说测 upsms 状态解析，实际只重新解析一段 JSON 字面量  |
| T3  | 登录链路（SMS/MFA）无任何自动化覆盖                                                                      | 需真实手机验证码，CDP 冒烟无法覆盖；回归靠手测           |

## 三、注释 / 文档腐烂（零风险，顺手可清）

| #   | 位置                                            | 现象                                                                                |
| --- | ----------------------------------------------- | ----------------------------------------------------------------------------------- |
| C1  | `domain/api/register/model.rs` `RegisterResult` | 3 行 tt_decrypt 备注错挂在其文档上（拆分前就错位，原样保留）                        |
| C2  | `protocol/cover.rs` `cache_path`                | 注释称「sha256 前 16 字节 hex」，实际落盘用完整 32 字节 digest                      |
| C3  | `signer/ticket.rs` `encode_component`           | 注释称「再对 `!'()` 额外转义」，代码实际保留 `!'()` 不转义（signer 禁改区，只记录） |
| C4  | `store/mod.rs` 模块文档                         | 说「五个实体」，device_profile（+bootstrap 元数据）实为第六组                       |
| C5  | `domain/api/rank/mod.rs` `fetch_rank_ex` 文档   | 自述「相邻页重叠 10 条客户端去重」——这是行为描述也是 B4 的坑位说明                  |

## 四、工程 / 构建提示（存量，非阻塞）

| #   | 位置                                         | 现象                                                                                                                                                        |
| --- | -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1  | `pnpm build`                                 | 主 chunk >500kB 警告（TanStack 全家桶在同一 chunk）；有 manualChunks 拆 react/tanstack，可再细化                                                            |
| E2  | vite TanStack Router 插件                    | 对 `pages/storage.tsx` 提示「未导出 Route」（路由壳写法与插件期望不一致，功能正常）                                                                         |
| E3  | rustfmt                                      | 仓库部分文件与本地 rustfmt 版本存在存量格式漂移（如 `commands/app_cmd.rs`、rank probe 两处），`make lint` 不含 fmt 故不阻塞                                 |
| E4  | `features/player/components/player-page.tsx` | 748 行，高于计划 <300 目标——剩余为 PlayerView JSX 本体；再降需抽沉浸流信息叠加/缓冲面板等子组件，会动组件树边界（条件挂载语义需逐一核对），建议单独立项评审 |

## 已修复（重构中当场修掉，含 commit）

| commit    | 内容                                                                                                                         |
| --------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `28b84dd` | `HONGGUO_STREAM_WINDOW=0` 开放 Range u64 下溢 panic（渐进窗口 0 对齐整集模式「给到末尾」，带回归测试）                       |
| `b3770a5` | 键盘快进在 metadata 未加载时给 currentTime 赋 NaN 抛 TypeError                                                               |
| `e298326` | cdp.mjs nav 落点断言按 pathname+包含 query 判准（validateSearch 补省缺参误判）                                               |
| `f1bfe01` | 三个死文件（resolving-pill / series-panel / feed-card-grid）、四处悬空重复注释、knip/prettier 存量清零                       |
| C9 重命名 | detail `pick` 注释与行为不符——查明「空串也算命中」是真实线上行为，是注释错：已改名 `str_field_any` 并写准注释 + 行为锁定测试 |
