#!/usr/bin/env python3
"""生成 macOS 应用名本地化文件（CFBundleDisplayName / CFBundleName）。

## 为什么需要它

Dock / Finder / 菜单栏显示的应用名来自 bundle 里的 `Info.plist`，Tauri
按 `productName` 写死一份英文名（Hongguo），系统语言是中文时也照旧。
macOS 的原生做法是 `Contents/Resources/<语言>.lproj/InfoPlist.strings`
覆盖显示名（微信就是范例：文件夹叫 `WeChat.app`，Dock 里显示「微信」）。

## 编码要求

InfoPlist.strings 是 old-style plist，Apple 的工具链按 **UTF-16
（带 BOM）** 读写（`file` 看微信的这份就是「UTF-16, little-endian」）。
UTF-8 在部分系统版本上会被静默忽略，所以这里显式写 UTF-16LE + BOM。

产物提交进仓库（不在构建期生成），`tauri.conf.json` 的
`bundle.macOS.files` 负责把它们拷进 `Contents/Resources/`。
改名时改这里、重跑脚本即可。
"""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_ROOT = ROOT / "src-tauri" / "i18n"

# 各系统语言下的应用名。key 是 lproj 的 BCP-47 语言标识。
# 与 productName（Hongguo，也是二进制/包目录名）有意区分：显示名可以
# 本地化，包名不能——改了包名等于换了应用。
DISPLAY_NAMES = {
    "en": "Hongguo",
    "zh-Hans": "红果播放器",
    "zh-Hant": "紅果播放器",
}

# CFBundleName 是菜单栏用的短名（Apple 建议 ≤ 16 字符），与应用显示名同值
TEMPLATE = '"{key}" = "{value}";\n'


def write_strings(lang: str, name: str) -> Path:
    body = "".join(
        TEMPLATE.format(key=key, value=name)
        for key in ("CFBundleDisplayName", "CFBundleName")
    )
    out = OUT_ROOT / f"{lang}.lproj" / "InfoPlist.strings"
    out.parent.mkdir(parents=True, exist_ok=True)
    # UTF-16LE + BOM：见文件头「编码要求」
    out.write_bytes(b"\xff\xfe" + body.encode("utf-16-le"))
    return out


def main() -> None:
    for lang, name in DISPLAY_NAMES.items():
        out = write_strings(lang, name)
        print(f"wrote {out.relative_to(ROOT)} → {name}")


if __name__ == "__main__":
    main()
