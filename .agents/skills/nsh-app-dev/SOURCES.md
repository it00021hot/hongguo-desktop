# Sources

本 skill 正文综合自 Native Shell（NSH）仓库自带的文档与实现（本地 checkout：`~/projects/nsh`）：

- `SKILL.md` 与 `references/` 的命令面、API 面、嵌入接口对应 checkout 内的 `website/docs/`、`website/modules/` 与 `crates/nsh-platform-*`、`crates/nsh-js` 等实现；`website/docs/` 即官网 <https://nsh.dev>（含 zh-CN）的源文档。
- 对齐时点：2026-10-22 的 nsh checkout（main）。
- `evals/` 的评测场景为本 skill 原创，fixture 依赖 `nsh init` 模板与 checkout 的 `nsh` CLI。

NSH 仓库演进后如需同步，以 checkout 内 `website/docs/` 为准更新 `references/`，重点盯平台差异（`platform-specific.md`）、web 目标（`web.md`）与嵌入接口（`ios.md` / `android.md` / `gpui.md` / `web.md`）。
