---
name: hongguo-dev
description: 番果（guava）NSH 客户端开发规范。修改本仓库前，与用户级 dev-habits 一起阅读。
---

# 番果 NSH 开发

先读用户级 `dev-habits` 及对应门禁文档，再读本项目 `AGENTS.md`。
结构边界见 `docs/project-structure.md`：功能状态与呈现归 `features/`，共享控件归 `ui/`，窗口与导航装配归 `shell/`。
UI 与平台装配遵循 `nsh`、`nsh-design-guides` 及 [NSH 接入说明](references/nsh.md)。
通用控件通过本壳 `ui` 构建，功能专用呈现归 `features/<功能>/presentation` 并组合共享控件；采用语义令牌与设计文档规定的交互、动效和可访问性。
组件只执行一次，变化值通过 signal/memo/闭包绑定；异步结果须隔离 owner 与来源代次。
业务调用直接经共享 service，原生组件事件边界使用具名 serde 类型。

接口以 `docs/hongguo-api-endpoints.md` 的请求、响应与实测效果为准；逆向分析参考 `references/reverse.md`。
开发、测试、打包使用 Makefile 与 NSH 接入说明；旧 Tauri/GPUI 壳的文档与代码只在备份分支 `codex/backup-tauri-final`，不据此恢复。
运行验证使用自建实例与独立数据目录，不操作用户活跃账号、窗口或数据库。
修改完成须运行适当的类型检查、严格 lint、构建与行为测试，分别报告真实平台运行与交叉编译结果。
