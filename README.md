# 🎬 红果短剧下载器（Tauri 桌面版）

<div align="center">

![平台](https://img.shields.io/badge/平台-Windows%20%2F%20macOS-blue?style=for-the-badge)
![技术栈](https://img.shields.io/badge/技术栈-Tauri%202%20%2B%20Rust%20%2B%20React%2019-47848F?style=for-the-badge)
![协议](https://img.shields.io/badge/协议-GPL--3.0-orange?style=for-the-badge)

**浏览 · 搜索 · 批量下载 · 在线播放 · 一键合并 · 磁盘清理**

纯 Rust 解密内核 · 无 ffmpeg 外部依赖 · 原画无水印 · 在线播放不落盘

[English](./README.en.md) · [许可证](./LICENSE) · [修改声明](./NOTICE)

</div>

---

## 📖 简介

面向 **红果短剧（番茄小说短剧频道 / novelread 系）** 的 Windows 与 macOS 桌面工具。

本版本由 Electron + Node.js **完全重写为 Tauri 2 + Rust**：内置字节系短剧 API 协议与
**CENC-AES-CTR 原生流式解密引擎**，媒体处理全部由纯 Rust crate 完成，**不再携带 FFmpeg
等任何外部二进制**，安装包体积从 158 MB 降到 70 MB 量级。

> **来源与修改声明**：本仓库基于 [327044572/hongguo-downloader](https://github.com/327044572/hongguo-downloader)
> 的**修改版本**（重写自 2026 年 10 月起），依照 GPL-3.0 第 5(a) 条声明。
> 具体修改内容见 [NOTICE](./NOTICE)，本版本整体继续以 **GPL-3.0** 授权。

---

## 🛠️ 技术栈

### 桌面端

| 层 | 选型 |
|---|---|
| 桌面外壳 | [Tauri 2](https://tauri.app) 2.12+（Rust / 系统 WebView） |
| 业务内核 | Rust 2021 · tokio · reqwest(rustls) |
| 编解码 | `rusty_h265` `rusty_h264` `rusty_aac` `muxide`（**均为纯 Rust**） |

### 前端

| 层 | 选型 |
|---|---|
| 路由 | TanStack Router 1.168（文件路由） |
| 服务端状态 | TanStack Query 5.99 |
| 表格 | TanStack Table 8.21 |
| 客户端状态 | Zustand 5.0 |
| UI | shadcn/ui（Radix UI）+ Tailwind CSS v4 |
| 语言 | TypeScript 6 · React 19 · Vite 8 |

---

## ✨ 功能总览

### 一、🔍 发现剧集

- **浏览**：按分类（真人剧 / 漫剧 / AI剧 / 漫画）+ 题材筛选，卡片式浏览，支持分页
- **搜索**：输入剧名，内置浏览器嗅探结果，封面卡片点选即用
- **粘贴链接 / ID**：支持 App 分享链接、含引导文案的长文本、纯数字 `series_id`
- 嗅探只依赖链接里的 `series_id`，不依赖站点样式类名，改版不易失效

### 二、🎯 选集与下载

- **快速选集**：全选 / 反选 / 清空 / 前 10 集 / 前 30 集 / 后 30 集
- **区间语法**：`1-50`、`1-10, 25, 30-45`
- **并发下载**：1~10 线程并行（默认 3），队列自动顺延
- **原生解密**：`spade_a` 密钥派生 + CENC-AES-CTR 流式解密，直接输出标准 MP4
- **防损机制**：先写 `.enc.tmp`，解密成功后才改名 `.mp4`，中途断网不留残缺文件

### 三、▶️ 播放

- **内置播放器**：下载完一集即可观看，播完自动下一集
- **断点续播**：记住每部剧看到第几集、第几秒
- **快捷键**：`空格` 播放/暂停 · `←` `→` 快退/快进 5 秒 · `↑` `↓` 上一集/下一集
- **在线播放**：未下载的集**点一下即可在线播放**，内存缓存不落盘
  （整集取回并解密后交给播放器，不是边下边解——CENC 要先读 moov 里的样本表才能定位每个样本）
- **兼容模式**：HEVC 解不出来时自动转 H.264；装了 ffmpeg 走它（可用 NVENC/QSV/AMF 硬编码），没装回退纯 Rust 软解

### 四、📋 下载管理

- 状态追踪：等待中 / 下载中 / 已完成 / 失败 / 已停止
- 实时进度、并发指示、一键启动 / 一键暂停 / 一键重试全部失败
- **扫描目录补登记**：磁盘文件还在、任务记录丢了也能自愈

### 五、🔗 一键合并

- **快速合并**：流复制拼接，不重编码，无损且极快
- **兼容合并**：转 H.264/AAC，任何播放器都能播
- 合并前校验磁盘空间与编码一致性，合并后校验输出可解析

### 六、🧹 磁盘清理

- 按剧删除、看完自动删、单集删除、全部清空
- 实时占用统计，**删除文件后保留剧集档案**，之后仍可在线播放或重新下载

### 七、⚙️ 设置

- 下载目录、文件命名模板、最大并发数（保存后立即生效，无需重启）
- 网络代理：跟随系统 / 手动指定（含常用端口预设）/ 强制直连，支持连通性测试
- 中英双语界面

---

## 💻 开发

### 环境要求

- Rust 1.85+（实测 1.98）
- Node.js 20+ / pnpm 10+（实测 Node 24 / pnpm 11）
- Windows 10/11 或 macOS 10.15+

### 本地开发

```bash
pnpm install
pnpm tauri:dev        # Vite + Tauri 开发模式
```

### 构建

```bash
pnpm tauri:build      # 打包 NSIS / DMG
```

> **无需准备 FFmpeg**——媒体处理已全部改为纯 Rust crate。

### 质量检查

```bash
make test-rust        # cargo test（305 个用例）
make test             # vitest（28 个用例）
make lint             # clippy + ESLint + Prettier
make typecheck        # tsc --noEmit
```

### 签名探针

签名失效时服务端返回 **HTTP 200 + 0 字节**（不是错误码），排查要看字节数：

```bash
cd src-tauri
cargo run --bin probe_api -- <series_id>
```

---

## 📂 项目结构

```text
hongguo-downloader-tauri/
├── src-tauri/src/
│   ├── signer/          # 字节系签名（1:1 移植，常量不可改）
│   ├── domain/          # 协议、加密、MP4 解析
│   ├── service/         # 应用服务（与 commands 同名同构）
│   ├── commands/        # Tauri command 薄层
│   ├── protocol/        # 自定义 URI 协议（Range 流）
│   ├── media/           # 纯 Rust 编解码
│   └── sniff/           # 内嵌浏览器嗅探
└── src/
    ├── routes/          # 六个路由页面
    ├── features/        # 按业务域拆分
    ├── lib/             # IPC 封装、schema、stores
    └── components/      # shadcn/ui + 布局
```

---

## ❓ 常见问题

### 拉不到剧集 / 接口返回空响应

官方 App 接口要求每个请求携带 `x-gorgon` / `x-argus` / `x-ladon` / `x-helios` / `x-medusa`
五个签名头。**签名缺失或算错时，服务端不报错，而是返回 HTTP 200 + 0 字节**——只看状态码会误判。

用 `cargo run --bin probe_api` 逐端点看**字节数**来诊断。

### 播放页黑屏但有声音

视频是 HEVC 编码，系统 WebView 解码需要硬件支持。应用已内置「兼容模式」：转成 H.264 后播放——
装了 ffmpeg 走它（可用 NVENC/QSV/AMF 硬编码，接近实时），没装回退纯 Rust 软解。

### 签名魔数能改吗

**不能。** `signer/constants.rs` 里的值是黑盒实测产物，与服务端一一对应。改任何一个，
签名都会被静默丢弃。

---

## 📄 许可证

**GPL-3.0**，见 [LICENSE](./LICENSE)。

本项目是上游项目的修改版本，修改声明见 [NOTICE](./NOTICE)，
第三方组件声明见 [THIRD-PARTY-NOTICES.md](./THIRD-PARTY-NOTICES.md)。

