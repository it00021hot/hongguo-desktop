# hongguo-desktop 前后端重构计划

> 分支：dev ｜ 状态：待执行
> 原则：功能保持不变、纯移动与逻辑拆分分提交、每步编译绿、bug 修复独立提交
> 方案来源：GPT 最终版方案（参考 Skyroc 取舍后的 pages + features + service + components）结合本项目实际（Tauri 2 + React 19 + TanStack Router/Query + Zustand + Tailwind v4；Rust 后端分层已规范）；后端按「API 层全面目录化」执行。不做 monorepo、不搞每域五件套、不引入转发层等新抽象。

## 一、前端目标结构（src/，行数为现状）

```
src/
├── main.tsx                     入口（保持）
│
├── pages/                       # TanStack 文件路由(薄壳) + 页面实现就近放置
│   ├── __root.tsx               AppShell/UpdateProvider/全局下载订阅(现 routes/__root)
│   ├── index.tsx                / → pages/home/
│   ├── browse.tsx  rank.tsx  new.tsx  detail.tsx  player.tsx
│   ├── history.tsx  liked.tsx  collections.tsx  reservations.tsx
│   ├── merge.tsx  tasks.tsx  storage.tsx  settings.tsx
│   ├── routeTree.gen.ts         生成物随目录迁移(插件配置同步改)
│   ├── home/                    # 各页面实现: *-page.tsx + components/ + hooks/(复杂页面才有)
│   ├── browse/  rank/  new-drama/
│   ├── series/                  # 原 features/series(详情页 8 组件拆分)
│   ├── player/                  # 薄壳,实现全在 features/player
│   ├── history/  liked/  collection/  reservation/
│   ├── merge/  download/        # 原 features/merge、features/download(tasks 页)
│   ├── storage/  settings/
│
├── features/                    # 只放复杂、可复用能力(不按页面建)
│   ├── player/                  # 播放引擎(重点分解)
│   │   ├── components/
│   │   │   ├── controls/        # 原 player-controls.tsx(886 行 7 组件)拆出:
│   │   │   │   player-controls.tsx  danmaku-send-box.tsx  scrub-bar.tsx
│   │   │   │   sliders.tsx  volume-popup.tsx  icon-button.tsx
│   │   │   ├── episode-picker.tsx  comment-panel/  incognito.tsx ...
│   │   ├── hooks/               # 从 player-page.tsx(1366 行)抽出的职责:
│   │   │   ├── use-playback-source.ts      # 取流/起播/清晰度切换/断供重试/缓冲事件
│   │   │   ├── use-transcode-fallback.ts   # videoWidth==0 探测 + H.264 转码轮询
│   │   │   ├── use-playback-progress.ts    # 5s 节流落盘 + 云端上报 5s/60s + 卸载补写
│   │   │   ├── use-binge-relay.ts          # 剧终三级接力(下一季→推荐流→猜你喜欢)/连播锁定
│   │   │   ├── use-player-overlay.ts       # 悬浮层显隐状态机(3s 倒计时/暂停常显/悬停)
│   │   │   ├── use-player-interactions.ts  # 滚轮/点击/键盘三套裁决
│   │   │   ├── use-mini-window.ts          # 小屏进出/置顶
│   │   │   └── use-danmaku-settings.ts     # 弹幕设置读写
│   │   ├── player-page.tsx      # 收敛为装配层(目标 <300 行)
│   │   └── types.ts
│   └── update/                  # 现有 provider/dialog 结构保留
│
├── service/                     # 与 Rust 交互 + 服务端数据
│   ├── tauri/                   # invoke.ts  events.ts  types.ts (原 lib/ipc)
│   ├── commands/                # 原 391 行 commands.ts 按域拆: app/settings/series/discover/
│   │                            #   rank/danmaku/interact/download/merge/play/history/storage...
│   ├── queries/                 # 原 1159 行 queries.ts 按域拆(与 commands 对齐),
│   │                            #   useInfiniteStream 通用封装放 common.ts, keys 随域走
│   ├── schema/                  # 原 688 行 schema.ts 按域拆 + common.ts(跨域类型)
│   └── query-client.ts
│
├── components/
│   ├── ui/                      # shadcn 23 件(不动)
│   ├── layout/                  # app-shell  app-sidebar  top-bar-tabs  theme-switch  window-controls
│   └── common/                  # series-cover  skeletons  list-search  refresh-shade  resolving-pill
│
├── hooks/                       # 跨页面通用: use-play-series、use-pin-window(togglePinned 去重)
├── stores/                      # 原 lib/stores: player  ui  theme  locale
├── locales/                     # 原 i18n(index.ts + zh-CN/en-US.json)
├── utils/                       # 原 lib 根: format  range  platform  list-filter
│                                #   playback-prefs  danmaku-emoji(+test)  cover(从 queries 抽出的封面代理)
├── lib/utils.ts                 # 仅保留 cn()(shadcn 约定,避免动 23 个 ui 组件)
├── assets/  styles/             # 不动
└── types/                       # 仅真正全局类型;无则不建
```

## 二、后端目标结构（src-tauri/src/，行数为现状）

分层职责一览：`commands`(IPC 命令层) → `service`(业务编排层) → `domain/api`(外部接口客户端层) + `store`(数据访问层) + `domain/model`(领域模型)；`utils`(工具层,新增) 独立被各层引用；`signer/media/protocol` 为专项能力层；`bootstrap` 只做启动装配。

utils 准入标准：无业务语义、不依赖 AppState/领域类型、纯函数可单测；域内部细节（如 mp4 的 `u32_at/u64_at` 字节读取）留在域内共享，不上提。

```
src-tauri/src/
├── lib.rs(325) main.rs app_state.rs error.rs diagnostics.rs   # 不动(装配/横切)
│
├── commands/            # 不动: 18 个 *_cmd.rs 薄层 + 静态检查测试(只同步 import 路径)
├── service/             # 不动: download/merge/play/series/settings/storage/transcode 七服务
├── bootstrap/           # 不动
├── signer/              # 不动(含自动生成 tt_hash 1138 行)
├── media/               # 不动(含 mf 1649 / vt 1105 —— COM/VideoToolbox 平台代码,拆分高风险低收益)
│
├── domain/
│   ├── api/             # ★ 全面目录化: 每域一目录 = mod.rs(端点函数+tests) + model.rs(struct/serde)
│   │   │                #   + parse.rs(复杂 JSON 解析才有,不强制)
│   │   ├── mod.rs
│   │   ├── client.rs    # 保持文件(759 行,共享 HTTP 客户端,基础设施非端点域)
│   │   ├── params.rs    # 保持文件(96 行,参数构造)
│   │   ├── play_url.rs  # 保持文件(89 行)
│   │   ├── rank/        # ← rank.rs(2522 行)大拆,7 域拆成 5 个顶层域目录:
│   │   ├── recommend/   #      排行榜 | 推荐流(+tab 配置缓存) | 新剧 | 预约(reserve/finalize)
│   │   ├── new_drama/   #      | 日历(含 beijing_date 时区) 各自 mod.rs+model.rs(+parse.rs)
│   │   ├── reservation/ #      原 1193 行内嵌 tests 随所属域走
│   │   ├── calendar/
│   │   ├── login/       # ← login.rs(1124): mod + model (+ mfa.rs 拆 MFA 上下行,若边界清晰)
│   │   ├── register/    # ← register.rs(1012): mod + model
│   │   ├── detail/      # ← detail.rs(986): mod + model + parse
│   │   ├── discover/    # ← discover.rs(926): mod + model + parse
│   │   ├── danmaku/     # ← danmaku.rs(745): mod + model
│   │   ├── interact/    # ← interact.rs(564): mod + model
│   │   ├── search/      # ← search.rs(524): mod + model
│   │   ├── stream_pick/ # ← stream_pick.rs(460): mod + model
│   │   └── history/     # ← history.rs(327): mod + model
│   ├── model/           # 不动(5 聚合已拆好)
│   ├── crypto/  mp4/    # 不动
│
├── protocol/
│   ├── register.rs(343) local.rs(223) range.rs(184) cover.rs(170) mod.rs  # 不动
│   └── stream/          # ← stream.rs(1366) 三职责分家:
│       ├── mod.rs       #   装配 + 对外 re-export(调用方零改动)
│       ├── progressive.rs  # ProgressiveStream + serve/HTTP Range 响应 + 窗口策略
│       └── cache.rs        # StreamCache 全局 LRU(淘汰权重/内存预算)
│
├── store/
│   ├── db.rs(455) bridge.rs(313) json_migrate.rs(239) paths.rs recover.rs mod.rs  # 不动
│   └── entity/          # ← entity.rs(689) 按聚合拆(与 domain/model 对齐):
│       ├── mod.rs       #   traits/re-export
│       └── settings.rs  task.rs  series.rs  playback.rs  merge.rs
│
└── utils/               # ★ 新增工具层: 无业务语义纯函数、零业务依赖,只被引用不引用业务层
    ├── mod.rs
    ├── json.rs          # ← 收敛四处重复的 JSON 取值: str_field(detail:460/login:677/discover:353/
    │                    #   history:149 各写一份)、int_field(discover:361/detail:468 重复)、
    │                    #   num_field、pick(detail:296) → 统一一套签名并补单测
    ├── time.rs          # ← beijing_date(rank.rs:1098)、now_ms(bootstrap/device.rs:73)、
    │                    #   now_millis(signer/ticket.rs:208) 三处时间函数收敛
    ├── hex.rs           # ← hex(protocol/cover.rs:60) + bytes_from_hex(login.rs:917) 成对 encode/decode
    └── url.rs           # ← urlencode_component(login.rs:536)、urlencode(register.rs:972) 统一 percent 编码
```

## 三、阶段划分（每步编译绿、独立 commit，预计 19~23 个提交）

- **P0 基线**（不提交）：pnpm typecheck/test/lint/knip + cargo check/test/clippy 记录存量告警；快照 routeTree 路由集合用于迁移后 diff。
- **P1 service 层**：C1 lib/ipc→service/tauri、commands/schema 按域拆；C2 queries.ts 按域拆入 service/queries/，coverProxyUrl 等纯函数移 utils/cover.ts，删旧入口。
- **P2 基础平移**：C3 stores/locales/utils/hooks 平移 + components.json、eslint、knip 配置更新；C4 路由并入 pages/（vite.config.ts 改 routesDirectory 与 generatedRouteTree），纯页面域 features 迁 pages/，routeTree 重新生成并 diff 确认路由集合不变。
- **P3 后端**：C5 rank.rs 大拆(5 域目录)；C6 login/register/detail/discover 目录化；C7 danmaku/interact/search/stream_pick/history 目录化；C8 protocol/stream 与 store/entity 拆分；C9 抽取 utils 工具层(json/time/hex/url 收敛各处重复实现并补单测，删除域内旧副本)；C10 clippy 存量清理。mod.rs re-export 保持公共路径尽量稳定，commands/service 引用同步更新。
- **P4 player 分解**（最高风险，小步走）：C10 player-controls 拆一组件一文件；C11~C14 player-page.tsx 抽 hooks（结构见 features/player 树），渲染结构不动、只做逻辑搬家；togglePinned 去重为 hooks/use-pin-window。
- **P5 其他大文件**（每页一 commit）：series-detail(706/8 组件)、browse(546)、rank(513)、new-drama(481)、home(470)、comment-panel(401)、rich-emoji-input(400)、settings(384)、tasks(384)；同名 episode-picker.tsx 冲突随迁移自然消解。
- **P6 收尾**：knip 清死代码、lint/prettier 全绿、pnpm build + cargo test/clippy 全量通过；更新 README/docs 结构说明；`pnpm tauri:dev` GUI 冒烟（首页→详情→起播→切集/清晰度→弹幕→下载/合并→设置→历史/收藏），播放体验最终请用户验收。

## 四、Bug 顺手修策略

重构中发现的任何行为 bug（重点：player 竞态防御代码、eslint react-hooks 的 effect 依赖问题、clippy 告警、knip 死代码）一律**独立 fix commit**，最终报告列清单；存量与新增严格区分，不与纯移动混提。

## 五、明确不做

不改业务行为/UI/文案；不动 mf/vt、signer、vendor、domain/{model,crypto,mp4}、service、bootstrap、store 其余文件、lib.rs 装配与 commands 薄层约定；不 monorepo 化；不做性能优化类改动。

## 六、验证门禁

前端每 commit：pnpm typecheck && test && lint；后端每 commit：cargo check && test；阶段末：pnpm build；终验：GUI 冒烟 + 全量测试。全程 dev 分支提交，main 不动。
