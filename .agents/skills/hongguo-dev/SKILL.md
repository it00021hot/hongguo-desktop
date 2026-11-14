---
name: hongguo-dev
description: 红果 NSH 客户端开发规范。修改本仓库前，与用户级 dev-habits 一起阅读。
---

# 红果 NSH 开发

先读用户级 `dev-habits` 及对应门禁文档，再读本项目 `AGENTS.md`。
UI 与平台装配遵循 `nsh`、`nsh-design-guides` 及 [NSH 接入说明](references/nsh.md)。
界面只通过本壳 `ui` 层构建，采用语义令牌与设计文档规定的交互、动效和可访问性。
组件只执行一次，变化值通过 signal/memo/闭包绑定；异步结果须隔离 owner 与来源代次。
业务调用直接经共享 service，原生组件事件边界使用具名 serde 类型。

接口以 `docs/hongguo-api-endpoints.md` 的请求、响应与实测效果为准；逆向分析参考 `references/reverse.md`。
开发、测试、打包使用 Makefile 与 NSH 接入说明，不再使用旧 Tauri/GPUI 的命令或目录。
`references/frontend.md`、`references/gpui.md`、`references/ipc.md` 等旧壳文档仅为历史研究资料。
运行验证使用自建实例与独立数据目录，不操作用户活跃账号、窗口或数据库。
修改完成须运行适当的类型检查、严格 lint、构建与行为测试，分别报告真实平台运行与交叉编译结果。
