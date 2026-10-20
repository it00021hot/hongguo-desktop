# GPUI 壳文件拆分计划与结果（单文件 ≤ 800 行）

日期：2026-10-20｜状态：**已完成**｜触发：`scripts/check-file-size.sh` 的 800 行硬闸 +
用户要求「全拆」

## 规则（每一批都照这个做）

1. **纯结构移动**：只搬 `impl` 块/函数，不改逻辑、不改可见性语义、不删注释；
   必要时只补 `use`。
2. 子模块用 `use super::*;` 取父模块的上下文（私有字段对子模块可见——Rust
   的字段隐私覆盖整个子树），不为此给字段加 `pub`。
3. 目录形态：`xxx.rs` → `xxx/mod.rs` + `xxx/<域>.rs`；`mod.rs` 只做模块声明
   + 结构体字段 + `new()` + 跨子模块共享的小 helper。
4. 每个文件目标 ≤ 300 行（提醒线），**硬上限 800**。
5. 跨子模块调用要求的最小可见性放宽（`pub(super)` / `pub(in crate::home)`）
   允许，但必须与原有效范围等价，且交付时逐条列出。
6. 验证：`cargo check -p hongguo-gpui` → `cargo clippy -p hongguo-gpui
   --all-targets -- -D warnings` → `cargo fmt --all -- --check` →
   `cargo test -p hongguo-gpui` → `make gpui` 起一次真机。
7. 一批一提交：`refactor(gpui): <文件> 拆 <目录>/<子模块>`。

## 结果（2026-10-20 全部落地）

| 原文件（行数） | 拆分后 | 文件数 | 最大文件 |
| --- | --- | --- | --- |
| `home/playback.rs` 1554 | `playback/{mod,open,switch,tail,preload,progress}.rs` | 6 | 698 |
| `home/stage.rs` 1003 | `stage/{mod,render,letterbox,overlay}.rs` | 4 | 777（`render_stage` 单方法 751） |
| `home/controls.rs` 1486 | `controls/{mod,bar,progress,menus,volume,danmaku_bar,mini,behavior}.rs` | 8 | 284 |
| `home/mod.rs` 1429 | `home/{mod,view,events,state,window}.rs` | 5 | 753（结构体 349 + `new()` 272） |
| `home/comment_view.rs` 1012 | `comment_view/{mod,list,composer,card,mention,delete_dialog,debug}.rs` | 7 | 253 |
| `shell.rs` 1951 | `shell/{mod,nav,sidebar,topbar,topbar_menu,account,events,window,debug,debug_preview,render}.rs` | 11 | 497（`Shell::new` 386） |
| `preview/fixtures.rs` 885 | `fixtures/{mod,feed,library,browse,rank,player}.rs` | 6 | 219 |
| `pages/browse.rs` 1365 | `browse/{mod,feed,search,suggest,filter,cards,render,preview}.rs` | 8 | 277 |
| `pages/new_drama.rs` 1264 | `new_drama/{mod,feed,filter,cards,calendar,view}.rs` | 6 | 427 |
| `pages/settings.rs` 1737 | `settings/{mod,account,general,network,playback,save,shortcuts,storage,transcode,load,view}.rs` | 11 | 279 |
| `pages/reservations.rs` 1098 | `reservations/{mod,list,actions,cards,render}.rs` | 5 | 296 |
| `pages/rank.rs` 1001 | `rank/{mod,list,tabs,rows,render}.rs` | 5 | 297 |
| `pages/history.rs` 858 | `history/{mod,list,actions,rows,render}.rs` | 5 | 238 |
| `detail/mod.rs` 916 | `detail/{mod,load,actions,render,preview_seed}.rs` | 5 | 275 |

结论：**GPUI 壳已无 > 800 行文件**；`scripts/check-file-size.sh` 的销号清单清空。
剩余 > 300 行的都是「单个渲染方法」造成的（`stage/render.rs` 777、`home/mod.rs`
753 含结构体与 `new()`、`shell/mod.rs` 497 含 `Shell::new`、`new_drama/view.rs`
427 含 338 行 `Render::render`、`detail/header.rs` 712、`detail/reviews/view.rs` 784）：
再往下拆必须**抽方法**（把大函数的一段子树提成 helper），那是行为保持的重构而不是
纯搬迁，需要单独立项。

## 不属于本次

- 上述「抽方法」式二次拆分（`render_stage` / `Render::render` / `Shell::new` 等）。
- vendored `gpui-pre-windows` 上游文件（`window.rs` 1740 行等）**不拆**：
  它是上游快照，补丁面越小越好。
- 壳外仍 > 800 的存量（本次未动，按需另立项）：`hongguo-api` 的 `rank/mod.rs`
  1530、`interact/mod.rs` 1125、`register/mod.rs` 903、`client.rs` 858、
  `recommend/mod.rs` 847、`login/mod.rs` 836；`hongguo-signer` 的 `tt_hash.rs`
  1150；`hongguo-stream` 的 `progressive.rs` 991。
