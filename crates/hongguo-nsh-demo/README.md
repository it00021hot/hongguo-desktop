# hongguo-nsh-demo — 红果短剧 × NSH 纯 Rust 闸门 demo

验证「GPUI 壳 → NSH Rust 壳」换壳方案：系统播放器（nsh `video` 元素）+ 本地解密供数桥 + 弹幕时间轴渲染。**纯 Rust，全程无 QuickJS**。评估结论与分期路线见 [docs/migration-plan.md](docs/migration-plan.md)，Windows 测试见 [docs/windows-testing.md](docs/windows-testing.md)。

## 状态（2026-10-21）

| 闸门 | 结果 |
|---|---|
| 播放 / 切集 / 叠层控件 | ✅ macOS 实测通过（Mac mini / AppKit 后端） |
| 弹幕渲染 | ✅ 时间轴管道 + 车道占用调度 + FPS 顶格 |
| 弹幕数据源 | ⚠️ 服务端弹幕接口只回引导条（对照组评论区正常），需重抓 hgplayer 请求重对齐 `hongguo-api` 的 `danmaku_payload`；诊断工具 `src/bin/dmprobe.rs` |
| Windows | ⬜ 待验（唯一大未知数：MediaEngine 对 Range 等待的容忍度） |

## 运行

前置：**NSH checkout 与本仓库并排 clone**（crate 以 `../../../nsh` 相对路径引用，Windows/Linux 同理）：

```
projects/
├── hongguo-desktop/   ← 本仓库
└── nsh/               ← github.com/longbridge/nsh（或内部镜像）
```

```sh
cargo run              # 本机后端直接跑（macOS=AppKit，Windows=Win32/MediaEngine）
RUST_LOG=info cargo run
# 脚本化验收（应用内合成输入 + 进程内截图，免屏幕录制权限）：
NSH_SCRIPT='wait 16000; shot /tmp/a.png; tap 185 658; wait 6000; shot /tmp/b.png' cargo run
```

启动后自动：推荐流选一部多集短剧 → 拉分集 → 起播推荐位 → 拉该集真实弹幕。

## 结构

- `src/lib.rs` — 全部 UI：舞台（视频 + 弹幕层 + 叠层控制栏）、弹幕时间轴游标、车道占用调度、FPS 统计
- `src/stream.rs` — 取流与供数：推荐流/分集/弹幕（hongguo-api）→ `online::prepare`（CENC 解密计划）→ tiny_http + `respond_stream` 本地供数
- `src/bin/dmprobe.rs` — 弹幕接口诊断（拉取条数 + 评论区对照）
- `docs/` — 换壳评估与 Windows 测试指南

## 工程注记

- **独立 workspace，不入主 workspace 成员/CI**：依赖仓库外的 NSH checkout（相对路径 `../../../nsh`），CI runner 无该源；与 `vendor/*` 的排除同理。本 crate 自行保持 `cargo clippy`/`cargo fmt` 干净。
- `[patch.crates-io]` 只镜像了 `muxide`（patch 段不跨 workspace 继承）。
- 服务面用 `AppStateInner::default()`（内存库 + 静态设备档案），免登录；后端九域 service 零改动复用。
