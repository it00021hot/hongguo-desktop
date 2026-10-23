# Windows 测试指南

目标：验证 nsh Windows 宿主（Win32/Direct2D + MediaEngine）+ 本地解密供数桥的组合。Mac 侧一切已实测通过，以它为基准对照。

> **结论（2026-10-21 实测通过，Win11）**：编译零错、起播走秒、Range 206（`....ftyp`）、下一集/暂停/恢复/on_end 自动连播全过；渐进流整段秒下，未观察到 Range 等待卡点；144Hz 屏 FPS ~142；`shot` 能拍到视频像素（Windows 与 Mac 不同，画面直接进进程内截图）。以下流程保留作回归用。

## 前置

- Windows 10 1809+，Rust toolchain（MSVC）
- NSH 依赖走 crates.io（`nsh-core` / `nsh-platform-windows` 0.1.1），无需额外 checkout
- 无需改任何路径；`cd <仓库>/crates/hongguo-nsh-demo` 直接构建

## 构建与运行

```powershell
cargo run --bin hongguo-nsh-demo
```

- 首次编译较长（nsh-platform-windows + hongguo 全后端）
- **日志自动落运行目录 `hg-nsh-demo.log`**（GUI 子系统无控制台，`cargo run` 时即 crate 目录，这是唯一观测面）
- 启动后自动：推荐流选剧 → 拉分集 → 起播（免登录，静态设备档案）

## 验收清单（按序，任何一步失败即记录现场）

| # | 项 | 通过标准 |
|---|---|---|
| 1 | 编译 | `cargo build` 零 error（nsh Windows 宿主 + hongguo 后端全量） |
| 2 | 起播 | 窗口出现；日志有 `[HgNshRs] 选中《…》` 与 `[Online] … 渐进流就绪`；本机出现 127.0.0.1 LISTEN 端口（`netstat -ano | findstr <pid>`） |
| 3 | 供数桥 | `curl -H "Range: bytes=0-15" http://127.0.0.1:<port>/s/1080/<vid>` → **HTTP 206**，body 以 `....ftyp` 开头（MediaEngine 能不能吃我们的 Range 应答，核心验证点） |
| 4 | 播放推进 | 控制栏时间走秒（`0:0x / 1:20`）；画面出现（对照官方 App 同一刻） |
| 5 | 弹幕渲染 | 头部 `FPS n · 弹幕 n`；车道分布正常。**数据源目前只有 1 条引导弹幕是已知服务端问题，不计入失败**；FPS 应顶格 |
| 6 | 叠层控件点击 | 播放/暂停（时间停走）、下一集（集数 +1、时长变化、画面重载） |
| 7 | 脚本验收 | 见下节（Windows 同样支持 wait/shot/tap） |

## 脚本化验收（免手点、出证据）

```powershell
$env:NSH_SCRIPT = 'wait 16000; shot C:/temp/w1.png; tap 185 658; wait 6000; shot C:/temp/w2.png'
cargo run --bin hongguo-nsh-demo
```

- `shot` 是进程内渲染截图（不需要屏幕录制权限）；Windows 上**能拍到 MediaEngine 视频像素**（实测与 Mac 不同，画面直接进截图），肉眼/时间读数双保险
- `tap` 坐标是窗口内容左上原点（Mac 上控制栏按钮：播放≈(62,658)、上一集≈(121,658)、下一集≈(185,658)、弹幕关≈(61,690)、系统栏≈(198,690)；**DPR/窗口装饰可能导致偏移，先用 shot 校一遍坐标**——实测 Win11 430×800 内容坐标与 Mac 完全一致，上表坐标直接可用）
- 对比 w1/w2：时间推进、集数变化、弹幕位置变化

## 关注点（Mac↔Windows 差异项）

1. **MediaEngine 对「未下载完的 fMP4 + Range 等待」的容忍度**：serve 在数据未就绪时会挂住请求最长 30s（Mac 上 AVPlayer 表现为缓冲）；MediaEngine 若对慢 Range 更敏感，会表现为起播失败或卡 0:00——这是本闸门最大的未知数。**实测解除**：演示剧集只几 MB，顺序填充秒级完成，MediaEngine 未表现出对慢 Range 的敏感；大剧集长播场景留 P1 观察
2. **DPR**：高 DPI 屏上 shot 按比例 1 渲染，tap 坐标按点而非物理像素
3. **性能**：对照 Mac 的 FPS 读数（Mac 弹幕 60 条顶格 ~100）；Windows Direct2D 路径的弹幕帧率单独记录
4. **字体**：中文文案依赖系统字体（微软雅黑），缺字表现为豆腐块

## 失败现场要抓的东西

- `hg-nsh-demo.log` 全文
- 失败步骤的 shot 截图
- 若卡编译：`cargo build 2> build-err.log` 的错误段
- 若卡播放：日志里 `[Online]` / `[Stream]` / `[HgNshRs]` 段 + 当时的时间读数

## 范围外

- 弹幕数据源（服务端只回引导条）——需重抓 hgplayer 请求重对齐 `danmaku_payload`，与 Windows 无关
- 小窗/单实例——P1 前单独选型
