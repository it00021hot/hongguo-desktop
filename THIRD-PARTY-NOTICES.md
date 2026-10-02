# 第三方组件许可声明 / Third-Party Notices

本项目使用了以下第三方组件。其版权与许可条款归各自作者所有。
本项目自身以 **GPL-3.0** 授权，详见 `LICENSE`。

This project uses the following third-party components. Their copyrights and licenses
remain with their respective authors. This project itself is licensed under **GPL-3.0**.

---

## 一、桌面框架 / Desktop Framework

| 组件 | 用途 | 许可 |
|---|---|---|
| [Tauri](https://tauri.app) | 桌面外壳、IPC、自定义 URI 协议、sidecar | Apache-2.0 OR MIT |
| [wry](https://github.com/tauri-apps/wry) | 系统 WebView 封装 | Apache-2.0 OR MIT |
| [tao](https://github.com/tauri-apps/tao) | 跨平台窗口 | Apache-2.0 OR MIT |
| [tauri-plugin-dialog](https://github.com/tauri-apps/plugins-workspace) | 原生目录选择对话框 | Apache-2.0 OR MIT |
| [tauri-plugin-opener](https://github.com/tauri-apps/plugins-workspace) | 打开外链与定位文件 | Apache-2.0 OR MIT |

## 二、编解码（替代 FFmpeg）/ Codecs (replacing FFmpeg)

以下均为**纯 Rust 实现**，不含 C 代码，不依赖任何外部二进制。

| 组件 | 用途 | 许可 |
|---|---|---|
| [rusty_h265](https://crates.io/crates/rusty_h265) | HEVC/H.265 解码（兼容模式转码输入） | Apache-2.0 |
| [rusty_h264](https://crates.io/crates/rusty_h264) | H.264 编码（兼容模式转码输出） | BSD-2-Clause |
| [rusty_aac](https://crates.io/crates/rusty_aac) | AAC-LC 编解码 | Apache-2.0 |
| [muxide](https://github.com/Michael-A-Kuykendall/muxide) | MP4 封装 | MIT OR Apache-2.0 |
| [aes](https://crates.io/crates/aes) | AES-128 加密（密钥派生） | MIT OR Apache-2.0 |
| [ctr](https://crates.io/crates/ctr) | AES-CTR 模式（CENC 解密） | MIT OR Apache-2.0 |
| [md-5](https://crates.io/crates/md-5) | MD5 摘要 | MIT OR Apache-2.0 |
| [sm3](https://crates.io/crates/sm3) | SM3 摘要（国密） | MIT OR Apache-2.0 |

## 三、异步与网络 / Async & Networking

| 组件 | 用途 | 许可 |
|---|---|---|
| [tokio](https://tokio.rs) | 异步运行时 | MIT |
| [reqwest](https://github.com/seanmonstar/reqwest) | HTTP 客户端 | MIT OR Apache-2.0 |
| [futures-util](https://github.com/rust-lang/futures-rs) | 异步流 | MIT OR Apache-2.0 |

## 四、序列化与数据 / Serialization

| 组件 | 用途 | 许可 |
|---|---|---|
| [serde](https://github.com/serde-rs/serde) | 序列化框架 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | JSON | MIT OR Apache-2.0 |

## 五、前端 / Frontend

| 组件 | 用途 | 许可 |
|---|---|---|
| [React](https://react.dev) | UI 运行时 | MIT |
| [TanStack Router](https://tanstack.com/router) | 路由 | MIT |
| [TanStack Query](https://tanstack.com/query) | 服务端状态 | MIT |
| [TanStack Table](https://tanstack.com/table) | 表格 | MIT |
| [Zustand](https://github.com/pmndrs/zustand) | 客户端状态 | MIT |
| [Radix UI](https://www.radix-ui.com) | 无障碍组件原语 | MIT |
| [Tailwind CSS](https://tailwindcss.com) | 原子化 CSS | MIT |
| [shadcn/ui](https://ui.shadcn.com) | 组件源码（复制到项目内，无运行时依赖） | MIT |
| [class-variance-authority](https://github.com/joe-bell/cva) | 组件变体 | Apache-2.0 |
| [clsx](https://github.com/lukeed/clsx) | 类名合并 | MIT |
| [tailwind-merge](https://github.com/dcastil/tailwind-merge) | Tailwind 类名合并 | MIT |
| [lucide-react](https://lucide.dev) | 图标 | ISC |
| [sonner](https://github.com/emilkowalski/sonner) | 通知 | MIT |
| [react-hook-form](https://react-hook-form.com) | 表单 | MIT |
| [@hookform/resolvers](https://github.com/react-hook-form/resolvers) | 校验器桥接 | MIT |
| [zod](https://zod.dev) | 运行时类型校验 | MIT |

## 六、构建工具 / Build Tooling

| 组件 | 用途 | 许可 |
|---|---|---|
| [Vite](https://vite.dev) | 构建工具 | MIT |
| [TypeScript](https://www.typescriptlang.org) | 类型系统 | Apache-2.0 |
| [Vitest](https://vitest.dev) | 测试 | MIT |
| [ESLint](https://eslint.org) | Lint | MIT |
| [Prettier](https://prettier.io) | 格式化 | MIT |
| [knip](https://github.com/webpro-nl/knip) | 未使用依赖检测 | MIT |

## 七、签名算法来源 / Signature Algorithm Origin

| 来源 | 说明 | 许可 |
|---|---|---|
| [woshishiq1/drpys](https://github.com/woshishiq1/drpys) @ `22261ad` | `spider/js/红果果[短].js`，签名算法移植来源 | GPL-3.0 |

本项目的 `src-tauri/src/signer/` 目录下的签名实现为该源文件的 **1:1 移植**，
未做任何数值改动。作为 GPL-3.0 项目的组成部分继续以 GPL-3.0 授权。

---

## 八、随软件分发的许可文本 / License Texts

本项目为源码分发。以下许可文本可从各组件的官方仓库获取：

- Apache-2.0：https://www.apache.org/licenses/LICENSE-2.0
- MIT：https://opensource.org/licenses/MIT
- BSD-2-Clause：https://opensource.org/licenses/BSD-2-Clause
- ISC：https://opensource.org/licenses/ISC-0BSD

本项目自身许可文本见 `LICENSE`，修改声明见 `NOTICE`。
