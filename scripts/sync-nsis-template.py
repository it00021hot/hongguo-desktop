#!/usr/bin/env python3
"""生成 Windows 安装器脚本：官方 NSIS 模板 + 应用显示名本地化补丁。

## 为什么要改模板

Tauri 的 `installerHooks` 只能注入宏，而模板把**快捷方式名写死成**
`${PRODUCTNAME}.lnk`（产品名 "Hongguo"）。Windows 上快捷方式名就是 `.lnk`
文件名、没法运行时本地化，所以名字必须在模板里改成变量 `$AppShortcutName`
（安装时按 `$LANGUAGE` 定，值由 `installer-hooks.nsh` 提供）。改这一处就够，
其余全靠钩子：模板只做「用变量」这件事。

补丁一共 6 处（脚本用精确上下文匹配，上游一变就报错而不是悄悄改错地方）：

1. `Var AppShortcutName` 声明
2. `.onInit`：初始化显示名 + 调 `NSIS_HOOK_SET_SHORTCUT_NAME`
3. `un.onInit`：从注册表读回安装时记录的名字
4. 所有 `\\${PRODUCTNAME}.lnk` → `\\$AppShortcutName.lnk`（安装/卸载/完成页）
5. `CheckIfAppIsRunning` 的消息里也用显示名（"红果播放器 正在运行"）
6. 「应用和功能」的 `DisplayName` 用显示名

## 用法

    python3 scripts/sync-nsis-template.py            # 生成/更新
    python3 scripts/sync-nsis-template.py --check    # 只校验是否最新（CI 可用）

上游版本取自本机 `@tauri-apps/cli` 的版本号（模板由 CLI 内的 bundler 渲染，
必须与 CLI 版本对齐；升级 Tauri 后重跑本脚本）。产物提交进仓库，构建不依赖
本脚本，也不需要网络。
"""

from __future__ import annotations

import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "nsis" / "installer.nsi"
CLI_PKG = ROOT / "node_modules" / "@tauri-apps" / "cli" / "package.json"

# 上游文件位置：Tauri 2.x 的 nsis 模板（早期版本在 .../windows/templates/，
# 这里按当前版本取；取不到会明确报错，不静默回退到旧路径）。
UPSTREAM_PATH_CANDIDATES = (
    "crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi",
    "crates/tauri-bundler/src/bundle/windows/templates/installer.nsi",
)

HEADER = """\
; ⚠️ 本文件由 scripts/sync-nsis-template.py 生成，勿手改。
; 上游：tauri-apps/tauri @ {tag} : {path}
; 补丁：快捷方式显示名改用 $AppShortcutName（安装时按 $LANGUAGE 定，
;       名字表与钩子在 src-tauri/nsis/installer-hooks.nsh，由 make-nsis-hooks.py 生成）
; 升级 Tauri 后重跑：python3 scripts/sync-nsis-template.py
; 校验是否与上游+补丁一致：python3 scripts/sync-nsis-template.py --check

"""

# ── 补丁（old, new, 期望命中次数）────────────────────────────────────
PATCHES: list[tuple[str, str, int]] = [
    (
        "Var OldMainBinaryName\n",
        "Var OldMainBinaryName\n"
        "; 快捷方式显示名：模板里所有 .lnk 都用它（安装时按语言定，见 installer-hooks.nsh）\n"
        "Var AppShortcutName\n",
        1,
    ),
    (
        '  !if "${DISPLAYLANGUAGESELECTOR}" == "true"\n'
        "    !insertmacro MUI_LANGDLL_DISPLAY\n"
        "  !endif\n",
        '  !if "${DISPLAYLANGUAGESELECTOR}" == "true"\n'
        "    !insertmacro MUI_LANGDLL_DISPLAY\n"
        "  !endif\n"
        "\n"
        "  ; 快捷方式显示名先给中性默认值；真正的中文名在 NSIS_HOOK_PREINSTALL\n"
        "  ; 里定（那是语言已选定、且一定早于快捷方式创建的时机）\n"
        '  StrCpy $AppShortcutName "${PRODUCTNAME}"\n',
        1,
    ),
    (
        "  !insertmacro MUI_UNGETLANGUAGE\n",
        "  !insertmacro MUI_UNGETLANGUAGE\n"
        "\n"
        "  ; 快捷方式显示名：读安装时的记录（卸载时系统语言可能已变，不能重算）\n"
        '  ReadRegStr $AppShortcutName SHCTX "${UNINSTKEY}" "ShortcutName"\n'
        '  ${If} $AppShortcutName == ""\n'
        '    StrCpy $AppShortcutName "${PRODUCTNAME}"\n'
        "  ${EndIf}\n",
        1,
    ),
    # 快捷方式文件名：安装、卸载、完成页（桌面快捷方式勾选）全都要走变量
    ("\\${PRODUCTNAME}.lnk", "\\$AppShortcutName.lnk", -1),
    # 「<名字> 正在运行」提示：默认写死 productName
    (
        '"$INSTDIR\\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"',
        '"$INSTDIR\\${MAINBINARYNAME}.exe" "$AppShortcutName"',
        2,
    ),
    # 「应用和功能」里的显示名
    (
        'WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"',
        'WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "$AppShortcutName"',
        1,
    ),
]


def cli_version() -> str:
    if not CLI_PKG.exists():
        sys.exit(f"找不到 {CLI_PKG.relative_to(ROOT)}，先跑 pnpm install")
    return json.loads(CLI_PKG.read_text(encoding="utf-8"))["version"]


def fetch_upstream(version: str) -> tuple[str, str]:
    """取上游模板，返回 (tag, 相对路径, 内容)。"""
    tag = f"tauri-v{version}"
    base = f"https://raw.githubusercontent.com/tauri-apps/tauri/{tag}/"
    errors = []
    for path in UPSTREAM_PATH_CANDIDATES:
        try:
            with urllib.request.urlopen(base + path, timeout=30) as resp:
                text = resp.read().decode("utf-8")
        except Exception as e:  # 网络/404 都归到这里，逐个候选试
            errors.append(f"{path}: {e}")
            continue
        if "!define PRODUCTNAME" not in text or "{{product_name}}" not in text:
            errors.append(f"{path}: 内容不像 NSIS 模板（缺特征串）")
            continue
        return tag, path, text
    sys.exit(
        f"取不到 {tag} 的上游 NSIS 模板：\n  " + "\n  ".join(errors) + "\n（版本号来自 @tauri-apps/cli）"
    )


def apply_patches(text: str) -> str:
    for old, new, expect in PATCHES:
        found = text.count(old)
        if expect < 0:
            if found == 0:
                sys.exit(f"补丁未命中：{old!r}")
        elif found != expect:
            sys.exit(f"补丁命中数不对（期望 {expect}，实际 {found}）：{old!r}")
        text = text.replace(old, new)
    return text


def render() -> str:
    version = cli_version()
    tag, path, upstream = fetch_upstream(version)
    header = HEADER.format(tag=tag, path=path)
    return header + apply_patches(upstream)


def main() -> None:
    check = "--check" in sys.argv
    content = render()
    if check:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current == content:
            print(f"{OUT.relative_to(ROOT)} 与上游+补丁一致")
            return
        sys.exit(
            f"{OUT.relative_to(ROOT)} 已过期——重跑 python3 scripts/sync-nsis-template.py"
        )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    # 不写 BOM：bundler 用 read_to_string 读模板，带 BOM 会把 U+FEFF 当正文
    # 渲染进脚本（它自己落盘时才 write_utf8_with_bom）
    OUT.write_text(content, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}（{len(content.splitlines())} 行）")


if __name__ == "__main__":
    main()
