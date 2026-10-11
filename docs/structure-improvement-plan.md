# 目录结构改进计划（2026-10-11）

> 分支：dev ｜ 状态：**待确认，未经确认不动代码**
> 依据：`AGENTS.md` + `.agents/skills/hongguo-dev`（含 `dev-habits`）的目录结构规范逐条核对现状
> 原则：纯移动/重命名与行为修改分开提交（硬红线 10、13）；不为目录整齐制造转发文件；
> 新代码单一归属，同一段逻辑只留一个明确归属；`signer/`、`media/platform`、`player/` 渲染结构是禁改区。
> 目标：把「已定型但只落实了一半的约定」补齐，消掉唯一的真实逻辑双实现。

## 0. 结论与健康基线

结构大方向是干净的，问题集中在**「约定只落实了一半」**和**一处真实的逻辑双实现**。
不存在死代码、不存在循环依赖、i18n 双语完全对齐——已裸跑核实的基线：

| 检查项 | 命令 | 结果 |
| --- | --- | --- |
| 死导出 / 死文件 / 未用依赖 | `pnpm knip` | 零输出 |
| i18n 键对齐 | zh-CN.json vs en-US.json | 505 : 505，零 only-zh / only-en |
| 分层纪律 | `lib.rs` 通读 | 331 行纯装配，模块全私有，命令注册按注释分组 |
| 元测试守护 | `commands/mod.rs` 扫源码测试 | command 层禁 tokio 约定在守 |

规模基线：前端 197 个 `.ts/.tsx` 共 18,104 行；后端 189 个 `.rs` 共 44,357 行。

---

## 1. P0 —— 评论/剧评组件双实现（唯一的真实逻辑重复）

skill 反面清单原文：「同一业务规则不许两份实现」。这是全仓唯一一处硬违反。

### 1.1 位置与现状

| 文件 | 行数 | 职责 |
| --- | --- | --- |
| `src/features/player/components/comment-panel.tsx` | 645 | 沉浸流评论面板（hgplayer 红皮肤） |
| `src/pages/series/components/review-list.tsx` | 579 | 详情页剧评列表（语义令牌皮肤） |

逐行比对结果——不是「相似的两层」，是同一套东西写了两遍：

| comment-panel.tsx | review-list.tsx | 一致性 |
| --- | --- | --- |
| `ReplyRow` L51-128 | `ReviewReplyRow` L64-136 | **props 签名完全相同**（`reply/myUserId/onDigg/onReplyTo/onDelete`）；DOM 结构、`relativeTime`、`EmojiText`、删除按钮、♥ 点赞逐字同构 |
| `ReplySection` L129-216 | `ReviewReplySection` L137-223 | 同为「展开才拉首页 + hasMore 续拉 + 发送后强制展开」 |

两处差异仅在于：

1. **皮肤**：player 侧用 `neutral-*` 硬编码（红线 11 的 hgplayer 例外），series 侧用语义令牌；
2. **i18n key 前缀**：`player.comments.*` vs `detail.*`。

i18n 层已留下指纹——以下 8 个键值在两侧**逐字重复**：

```
reply / replyToComment / replyPlaceholder / replyToPrefix
expandReplies / collapseReplies / moreReplies / repliesLoadFailed
```

另 `detail.like` 重复 `player.interact.like`。`src/service/queries/danmaku.ts`（401 行 / 14 个 hook）
同时管弹幕、评论、剧评三件事，是同一问题在查询层的同源表现。

反证：后端**已经收敛**——`src-tauri/src/domain/api/danmaku/mod.rs:576 fetch_replies(path, body, env)`
就是评论与剧评共用的取数路径。前端没跟上。

### 1.2 改动

1. 新建 `src/components/common/comment/`，落三个共享件：
   - `reply-row.tsx`（合并 `ReplyRow` / `ReviewReplyRow`）
   - `reply-section.tsx`（合并 `ReplySection` / `ReviewReplySection`）
   - `comment-row.tsx`（两层共用的主列表行，若比对后确认同构则合并）
2. 皮肤差异用 props 传入 class（player 侧沿用既有红皮例外，series 侧传语义令牌）；
   或在 player 侧包一层红皮壳，共享件只出结构——按 1.1 比对结果哪种更少改动走哪种。
3. i18n 的 8 个重复键收敛到 `common.reply.*`，两侧改引；`detail.like` 改引 `player.interact.like` 后删除。
4. `src/service/queries/danmaku.ts` 按域拆成 `danmaku.ts` / `comments.ts` / `reviews.ts`，
   `queries/index.ts` 出口不变。

### 1.3 性质与验证

性质：**逻辑搬家不重写**，不动交互语义（player 区 skill 原文约束）。
预期副作用：两个文件自然回落到 300 行线内（`dev-habits #21`）。

验证：`pnpm typecheck` + `pnpm lint` + `pnpm test` + `pnpm knip`；
CDP probe 走 `/player` 评论面板与 `/detail` 剧评 tab 两条链路（回复展开/发送/点赞/删除）。

### 1.4 不作

不借机重构 `player-page.tsx`(683) / `player-controls.tsx`(589) / `home-page.tsx`(441) /
`series-detail-page.tsx`(422) / `browse-page.tsx`(382) / `settings-page.tsx`(363) /
`rich-emoji-input.tsx`(363)。`player/` 是敏感区，只做上面这一项搬家。

---

## 2. P1 —— `domain/api` 的 `mod+model[+parse]` 约定只落实了 2/14

### 2.1 位置与现状

`backend.md` 写明每域目录化 `mod+model[+parse]`，实际只有 2 个域有 `parse.rs`：

| 域 | mod.rs 行数 | fn 数 | parse.rs |
| --- | --- | --- | --- |
| danmaku | 984 | 16 | ❌ |
| rank | 983 | 3 | ❌ |
| register | 975 | 8 | ❌ |
| login | 888 | 15 | ❌ |
| interact | 860 | 19 | ❌ |
| recommend | 726 | 9 | ❌ |
| search | 477 | 6 | ❌ |
| calendar | 472 | 5 | ❌ |
| stream_pick | 421 | 8 | ❌ |
| history | 306 | 6 | ❌ |
| detail | 460 | 3 | ✅ |
| discover | 537 | 2 | ✅ |

`danmaku/mod.rs` 单文件混了 4 类职责：5 个 payload 构造器（`danmaku_payload`/`comments_payload`/
`series_comments_payload`/`comment_replies_payload`/`review_replies_payload`）、6 个 fetcher、
3 个 parser（`check_comment_code`/`parse_comment_page`/`parse_reply_page`）、1 个发送器。

未核实项：`rank/mod.rs` 983 行只有 3 个 fn，疑似内联数据表/常量——拆它之前必须先读，不在本批范围。

### 2.2 改动

以 `danmaku` 为样板，按 `detail`/`discover` 已有的命名对齐：

```
domain/api/danmaku/
├── mod.rs       # 只留对外 pub async fn + 模块声明
├── model.rs     # 已有，不动
├── payload.rs   # 5 个 *_payload 搬入
└── parse.rs     # 3 个 parse/check 函数搬入
```

其余域（`interact`/`login`/`register`/`recommend`/`search`/`calendar`/`stream_pick`/`history`）
按同一形状逐个推进，**一个域一个 commit**。

### 2.3 验证

`cargo test --lib`（该域测试随文件走 `#[path]` 挂回，模式见 `backend.md` 测试规范）+
`make lint`。纯搬迁，无行为变更。

---

## 3. P2 —— 测试数据住在生产源码树，且构成反向依赖

### 3.1 位置与现状

```
src-tauri/src/domain/api/testdata/
├── py_enc_accepted.hex
├── py_gz.hex
├── register_req_real.bin
└── tt_v5_golden.hex
```

引用方只有两处 `#[cfg(test)]`：`domain/api/register/mod.rs:513,545`、
`signer/tt_hash.rs:1127`。

两个问题：① 纯测试专用数据却编译进 `src/` 生产树；② **`signer` 伸手 include
`../domain/api/testdata/`**，形成 `signer → domain` 的反向依赖（规定的单向是 commands→service→domain）。

仓库已有正确落点：`src-tauri/tests/fixtures/`（`heic-cover-sample.heic` 就在那）。

### 3.2 改动

整体移到 `src-tauri/tests/fixtures/`，两处 `include_bytes!`/`include_str!` 路径跟着改。

### 3.3 验证

`cargo test --lib register` + `cargo test --lib signer::tt_hash`。

---

## 4. P3 —— `components/` 两级散装，约定第 5 条只落实了 emoji

### 4.1 位置与现状

`frontend.md` 目录归属第 5 条：「shadcn 件 → `ui/`；布局壳 → `layout/`；跨页展示件 → `common/`」。
实际 `common/` 里只有 `emoji/`，4 个跨页展示件散在 `components/` 根：

| 文件 | 行数 | 使用处 |
| --- | --- | --- |
| `src/components/refresh-shade.tsx` | 34 | 6 处 |
| `src/components/series-cover.tsx` | 44 | 5 处 |
| `src/components/list-search.tsx` | 32 | 4 处 |
| `src/components/skeletons.tsx` | 32 | 4 处 |

结果是 `components/` 同时存在「文件」与「目录」两个层级，归属判定只能靠记忆。

### 4.2 改动

4 个文件移入 `src/components/common/`，`components/` 收敛成 `ui` / `common` / `layout`
三个子目录、零散 file。**与 P0 的 `common/comment/` 合并为一次提交**——两次 import
改写一次做完，比分开多做一轮全量引用改动划算。

### 4.3 验证

`pnpm typecheck` + `pnpm lint` + `pnpm knip` + CDP 全路由 probe。

---

## 5. P4 —— 两个同名不同物的 `EpisodePicker`

### 5.1 位置与现状

| 文件 | 行数 | 职责 | props |
| --- | --- | --- | --- |
| `src/features/player/components/episode-picker.tsx` | 333 | 播放器选集浮层（双 tab + 数字网格） | `seriesId/currentIndex/hint/onSelect/onClose` |
| `src/pages/series/components/episode-picker.tsx` | 148 | 下载批量选区（区间语法 + 反选） | `episodes/selected/onChange` |

同名、同在 components 约定下、职责与 props 完全不同。违反 `frontend.md`「命名准确」。

### 5.2 改动

`src/pages/series/components/episode-picker.tsx` → `episode-range-picker.tsx`
（组件导出名同步 `EpisodePicker` → `EpisodeRangePicker`）；player 侧保留 `EpisodePicker` 不动。

### 5.3 验证

`pnpm typecheck` + `pnpm lint` + `pnpm knip` + CDP probe `/tasks` 下载批量选择链路。

---

## 6. P5 —— 前后端域粒度错位，「同名同构」断了两处

### 6.1 位置与现状

`domain/api` 有 17 个域，前端 `commands/` 15 个、`queries/` 16 个，且错位：

- **四处合一**：`rank` + `new_drama` + `reservation` + 搜索 四个 API 域 → 前端只有
  `commands/rank.ts`；`rank_cmd.rs`(182) 里也确实混着七个命令：
  `rank_list` / `new_drama_list` / `search_series_cmd` / `search_suggest_cmd` /
  `reservation_list` / `reservation_reserve` / `new_drama_calendar`
- **二合一**：`discover` + `recommend` → `commands/discover.ts`
- 而前端 `pages/` 是 `rank/` `new-drama/` `reservation/` 三个独立页面目录

`dev-habits #23` 的核心是「看目录就能定位对应关系」，这里断了。

### 6.2 改动

按 `pages/` 的域粒度对齐：

- 前端：`commands/rank.ts` → `commands/{rank,new-drama,reservation,search}.ts`；
  `queries/rank.ts` 同步拆（`useRank` / `useNewDrama`+`useNewCalendar` / `useReservations`+`useReserveSeries`
  / `useSeriesSearchApp`+`useSearchSuggest`）；`commands/index.ts`、`queries/index.ts` 出口保持不变。
- 后端：`rank_cmd.rs` 按同样边界拆成 `rank_cmd.rs` / `new_drama_cmd.rs` / `reservation_cmd.rs`，
  `lib.rs` 的 `generate_handler!` 同步更新。**若 §7.2 的 `SOURCES` 自动发现已先落地**，
  拆出的新文件自动纳入守护；否则 `commands/mod.rs` 的清单要手动补三条。

优先级低于 P0~P3：现在还能忍，**下次加预约相关功能时顺带做**，不单独排期。

### 6.3 验证

`pnpm typecheck` + `pnpm lint` + `pnpm knip` + `cargo test` + `make lint`；
CDP probe `/rank` `/new` `/reservations` 三页。

---

## 7. P6/P7 —— 零散项（可随手带走，不单独排期）

### 7.1 `gen_tt_hash.py` 放错了根

`src-tauri/gen_tt_hash.py` 是生成器，应在仓库根 `scripts/`（那里已有 `make-icons.py`、
`build-updater-manifest.py` 等 6 个同类脚本）。纯移动。

### 7.2 meta-test 的 `SOURCES` 清单是手工登记的坑

`src-tauri/src/commands/mod.rs` 的 `commands_do_not_touch_tokio_context_directly` 用硬编码
`SOURCES` 数组列文件。**新增 command 文件不加进这个列表，就静默逃过守护**，而清单里
没有写这条说明。改成测试时用 `env!("CARGO_MANIFEST_DIR")` + `read_dir` 自动发现 `.rs`，
零依赖、自维护。

### 7.3 响应式栅格断点三处逐字重复

`grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 2xl:grid-cols-8` 出现在：

- `src/components/skeletons.tsx:11`
- `src/pages/series/components/series-card-grid.tsx:28`
- `src/pages/new-drama/components/new-drama-recommends.tsx:73`

按 `src/styles/index.css` 已有 `hg-loadbar` / `scrollbar-thin` 的先例定义成 `hg-cover-grid` 全局类。

### 7.4 目录名与路由名不一致且无说明

`/tasks` 的实现在 `pages/download/`；`pages/series/` 根本没有 series 路由（承载 `/browse` + `/detail`）。
这是**有意的按域分组**，但 `frontend.md` 目录归属一节没写，新人会找不到实现。补一句话说明即可。

### 7.5 `utils/playback-prefs.ts` 兼职管了隐身开关

`readIncognito`/`writeIncognito` 被 `src/features/player/components/incognito.ts` 从这导入。
隐身是窗口行为不是播放偏好。拆成 `playback-prefs.ts` + `incognito.ts` 两个小文件。

---

## 8. 推进顺序与提交切分

按「纯移动/重命名」与「行为可见改动」分开的原则分批：

| 批次 | 内容 | 性质 | commit 切分 |
| --- | --- | --- | --- |
| 第一批 | §3 P2 testdata 搬迁、§5 P4 重命名、§7.1 生成器归位 | 纯移动 | 三个独立 commit（一个动作一个） |
| 第二批 | §1 P0 共享评论件 + §4 P3 `common/` 归位 | 搬家 + 一处删除合并 | 先 `refactor:` 搬家，后 `refactor:` 收敛 i18n 与拆分 queries（若 i18n 键值有变更则单独一个） |
| 第三批 | §2 P1 danmaku 拆 `payload.rs`/`parse.rs` 做样板 | 纯拆分 | 一个域一个 commit |
| 第四批 | §6 P5 命令域对齐 | 纯搬迁 | 前端、后端分开 |
| 随时 | §7 零散项 | 各自独立 | 随手带走 |

每批交付前裸跑确认退出码（不用管道吞失败）：`pnpm typecheck`、`pnpm lint`、`pnpm test`、
`pnpm knip`、`cargo test`、`make lint`。

**本计划未经确认不动代码。** 确认后从第一批开始执行。
