# 开发流程（环境 / 命令 / 调试 / 提交 / 发版）——两壳共用

Tauri 壳与 GPUI 壳共用的流程在本文件；壳专属部分：Tauri 的 WebView 调试与发版在本文件，GPUI 壳的全部（开发 / 界面规范 / 验收 / 已知坑）见 `references/gpui.md`。

## 环境准备

- Node ≥ 22（pnpm）+ Rust toolchain（MSRV 1.99，edition 2024）。依赖：`pnpm install`。
- 数据目录 `%APPDATA%/hongguo-downloader/hongguo.db`（SQLite，沿 Electron 版路径便于旧档迁移）；`HONGGUO_DATA_DIR` 环境变量可重定向（测试用）。多壳数据根分离：壳经 `paths::set_shell_data_root` 声明**与默认根平级**的独立数据目录（GPUI 壳 = `%APPDATA%/hongguo-gpui`），与 Tauri 根互不嵌套、互不共用——嵌子目录的形态已被否决，不得改回。
- 开发端口 **1420 固定**（Tauri 要求，strictPort）；加上 SQLite 单实例锁，**第二个 `tauri dev` 起不来**。用户活跃实例的自动化边界（一律不可调试，只针对自己启动的实例）见 dev-habits『不碰用户活跃环境』。

## 日常命令

Makefile 是入口（`make help` 看全量）：

| 命令                           | 作用                                                          |
| ------------------------------ | ------------------------------------------------------------- |
| `make dev`                     | `pnpm tauri dev`，Vite + Tauri 联调                           |
| `make gpui`                    | 启动 GPUI 壳（与 Tauri dev 并存，数据根平级分离不撞库）       |
| `make gpui-preview`            | GPUI 壳离屏预览矩阵出 PNG——界面改动的标准验收（见 `gpui.md`） |
| `make build`                   | 前端产物（`tsc -b && vite build`）                            |
| `make test` / `make test-rust` | vitest / cargo test                                           |
| `make lint`                    | **质量闸门、一票否决**：ESLint + Prettier + cargo clippy×3    |
| `make typecheck`               | `pnpm typecheck`（app + node 两个 tsconfig）                  |
| `make release`                 | `pnpm tauri build`（NSIS/DMG/APP，产出 updater 工件）         |
| `make assets`                  | 改名/换 logo 后重新生成图标与名称本地化（离线，产物提交入库） |
| `make nsis`                    | 升级 Tauri 后同步 Windows 安装器模板（需联网）                |
| `make fmt` / `make clean`      | cargo fmt / 清理 dist 与 target                               |

前端另有 `pnpm knip`（死代码检查，重构后必跑）。Rust 日志：`RUST_LOG=debug`（env_logger，默认 info）。

### lint 闸门的注意事项（Makefile 注释是真踩出来的）

- clippy 必须 `--all-targets`：否则 `#[cfg(test)]` 里的代码根本不进检查。
- macOS 双目标交叉 clippy（`--target aarch64-apple-darwin` / `x86_64-apple-darwin`）只守平台链正清单（Makefile `MAC_CROSS_CRATES`）：VT 等 cfg 门后的代码 Windows 宿主看不见，v0.0.1 就是它带着 125 条警告上了 CI；全图交叉会拖进 aws-lc-sys（要真 macOS SDK 的 Darwin libc 头，本地生不出来），全图覆盖在 CI 的 macOS 原生 runner。交叉的 C 工具链是真 clang（LLVM，前置 `winget install LLVM.LLVM`）：交叉链上有真编 C/C++ 的 build script，早年占位 `CC_*=true` 的「假成功」口径已失效；路径必须 `C:/` 短路径形态（cc 拆带空格路径）。
- `-D clippy::allow_attributes`：`#[allow]` 直接拒；确需断言用 `#[expect(reason)]`——lint 不再触发时编译失败，杜绝过期放行。
- 别用 `-` 前缀或 `|| true` 吞失败；每行独立 shell，`cd` 只对本行有效。

## 调试

### WebView 前端（Tauri 壳；首选 CDP，详见 docs/webview-cdp-testing.md）

```bash
# 环境变量必须在 app 进程启动前设置（WebView2 由 app 派生，继承 env）
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" pnpm tauri dev

node scripts/cdp.mjs pages          # 列 CDP targets
node scripts/cdp.mjs nav browse     # SPA 导航（路由参数不带前导斜杠！MSYS 会改写 /browse）
node scripts/cdp.mjs eval "<js>"    # 页面求值；可直接 invoke 后端命令验证返回
node scripts/cdp.mjs shot detail x.png
node scripts/cdp.mjs probe browse   # 裂图+网络失败体检，退出码可接 CI
```

- **回归惯例**：主要路由各跑一遍 probe：`for r in . browse rank new history collections; do node scripts/cdp.mjs probe "$r"; done`
- **probe 判死规则**：裂图（`img.complete && naturalWidth === 0`，即请求成功但解码失败）与图片请求 `Network.loadingFailed` → FAIL；懒加载未触发的图（`complete=false`）不算失败。退出码可直接接 CI。
- 直接调后端命令排查：`eval "window.__TAURI_INTERNALS__.invoke('related_series',{seriesId:'...'})"`。
- macOS 无 CDP（WKWebView），用 `node scripts/webdriver.mjs`（debug 构建内嵌 WebDriver，127.0.0.1:4445）。
- CDP 与 computer-use 分工：CDP 够用就 CDP（DOM/网络/求值可断言）；原生窗口装饰、系统对话框、无 CDP 的已运行实例才走 computer-use。
- 已知坑：纯数字 search 参数会被 JSON.parse 成 number——新路由照抄 `/detail` 的 `validateSearch` 兜底；视频帧进不了截图（WebView 合成层），播放验证用 `requestVideoFrameCallback`。

### GPUI 壳

见 `references/gpui.md`（GPUI 壳规范的唯一归属）：`make gpui-preview` 离屏预览矩阵是界面改动的标准验收，运行中窗口取证走 `screenshot.rs`（HONGGUO_SHOT）。

### Rust 后端

- `RUST_LOG=debug` 看日志；诊断快照见 `crates/hongguo-tauri/src/diagnostics.rs`。
- 单测过滤：`cargo test --lib <模块路径>`（在仓库根跑）；e2e：`HONGGUO_E2E_DIR=<目录> cargo test --lib testing::media_pipeline_e2e -- --nocapture`。
- 网络请求问题先读 `docs/hongguo-api-endpoints.md` 的坑清单（签名失效返回 200+0 字节、send_code type=1 vs 3731 等）。

## 提交纪律

- 开发在 **dev 分支**；`main` 只进验收过的内容。
- 提交信息按 dev-habits『commit message 规范』（Conventional Commits）+ 中文描述：`feat(player): …`、`fix(feed): …`、`refactor(service): …`、`docs: …`。
- 纯移动/结构拆分与行为修改**分开提交**；bug 修复独立 `fix:` 并写明根因。
- 改代码用精确编辑（dev-habits『改代码用精确编辑』；本仓库多次事故）。Python 照常可以写辅助工具（范例：`captures/addon.py` 抓包、`captures/audit.py` 参数审计，放工具目录正常提交）；一次性分析的临时脚本用完即删。
- **提交前的门禁命令必须裸跑确认退出码**——`pnpm x | tail && commit` 这类管道会吞掉失败，曾把坏状态带进提交。Windows 下写文件后有可见性延迟：写完等 1 秒再验证。
- 每 commit 编译绿：前端 `pnpm typecheck && pnpm test && pnpm lint`；后端 `cargo check && cargo test`。阶段收尾跑 `make lint` 全闸门 + `pnpm build`。
- 播放器（features/player）改动收尾做冒烟：首页→详情→起播→切集/清晰度→弹幕。
- `crates/hongguo-gpui` 界面改动收尾：`make gpui-preview` 目检受影响场景（规范见 `references/gpui.md`）；动了场景矩阵/种子分发再跑 `cargo test -p hongguo-gpui --test preview_scenes`。

## 发版（仅 Tauri 壳；GPUI 壳暂未入发布管线）

1. **发版走 GitHub CI（release.yml），本地不跑 `make release`**——推 `vX.Y.Z` tag 即触发：三平台矩阵构建 + 上传安装包与 `.sig`，收尾组装 updater `latest.json` 与 SHA256SUMS。
2. **每次发版先改版本号文件再提交、再打 tag**：`package.json` / `crates/hongguo-tauri-app/tauri.conf.json` / `crates/hongguo-tauri-app/Cargo.toml` 三处同值，跑 `cargo update -p hongguo-desktop` 同步 Cargo.lock 一并提交（`chore(release): vX.Y.Z`）。CI 构建前还会把 tag 号写进 tauri.conf.json，版本号唯一来源是 tag。
3. **发版提交必须先补 `CHANGELOG.md` 对应 `## [x.y.z]` 段落**（✨ 新增 / ⚡ 优化 / 🐛 修复，格式见文件头）：CI 从这里提取该版本段落作 GitHub Release 说明和应用内更新弹窗内容，段落缺失流水线直接失败——更新内容必填，不写「优化体验若干」式空话。
4. 流程：dev 验收 → 合入 main（--no-ff）→ 推 main → 在 main 打 tag 推送。
5. updater 走 GitHub Releases（`endpoints` 指向 `latest.json`），Windows `installMode: quiet`，NSIS 用仓库自带模板 + installerHooks（`make nsis` 保持模板同步）。
6. 改应用名/图标后：`make assets` 重新生成并**提交产物**（构建不依赖本机 Python/网络）。
7. capabilities 是最小权限集（自绘标题栏的逐条 window 动作放行），新增窗口操作要同步 `crates/hongguo-tauri-app/capabilities/default.json`。
8. **issue 回复在发版后再回**：修复/功能合入 dev ≠ 用户可用——回复「已实现/已修复」时用户更新不到就会追问。顺序：合入 dev → 发版（tag + CHANGELOG）→ 再到 issue 附版本号回复（2026-10-15 教训：@小果AI 未完成时提前回了「其余已实现」，用户实测发现与回复不符）。

## 工程背景（改构建配置前必知）

- release profile `opt-level=3 + lto="thin" + strip`：fat LTO 曾让 rustc 段 17-18 分钟，thin 砍半以上。**禁加 `panic="abort"`**（见 SKILL.md 红线 6）。
- `[profile.dev.package.rusty_h265-accel] debug-assertions = false`：只为该依赖关断言（其去块滤波 debug_assert 在 1080p 真码流必触发），别推广到别处。
- vendor/ 四个 patch（muxide B帧 stco、rusty_alloc 2.2.2 顶替 1.1.6、rusty_h264-common 摘全局分配器、tauri-plugin-webdriver 抬版本）各有回收约定，改这些目录前看**根 `Cargo.toml`** 的 `[patch.crates-io]` 注释（workspace 化后 patch/profile 都在根 manifest）。`vendor/` 已被 `[workspace] exclude` 排除，不参与 lint/test。
- `lib.rs` 里的 `alloc_stress` 压力测试：只要它崩就是分配器的锅，别在业务代码里找。
