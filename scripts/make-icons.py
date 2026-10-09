#!/usr/bin/env python3
"""生成应用图标：参考端高清 logo + macOS 标准留白网格。

只依赖 ImageMagick（`magick`）与 macOS 自带的 `iconutil`，不需要 Pillow——
脚本会在缺工具时给出明确报错。

## 图形来源（src-tauri/icons/source.png）

参考端（hgplayer，`/Applications/红果短剧.app`）的 1024 原图——比官网
`<link rel="icon">` 那张 40px 小图清晰得多（旧脚本把 40px 放大 25 倍，
边缘糊成一块，Dock 里一眼就看出来）。提取方式（macOS 自带工具，可复现）：

    iconutil -c iconset -o /tmp/hg.iconset \
      "/Applications/红果短剧.app/Contents/Resources/iconfile.icns"
    cp /tmp/hg.iconset/icon_512x512@2x.png src-tauri/icons/source.png

source.png 不存在时脚本会自己跑一遍上面的提取（见 extract_reference）。

## 为什么要有留白

macOS 的应用图标有一套固定的「画布网格」：1024 画布里图形本体占 824
（四周各留 100），贴到 Dock 才和系统应用一样大。计算器/备忘录/Chrome
实测一致——图形占画布 80.5%。参考端与旧版我们的图标都是**满幅 1024**，
没有留白，Dock 里就比别的应用大一圈。这里统一按标准网格重新摆位；
源图自带的圆角（约 23%，与标准 22.5% 同量级）原样保留，不二次裁切——
在已经圆角的图上再套圆角遮罩，边缘会被啃掉一圈毛边。
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_DIR = ROOT / "src-tauri" / "icons"
SOURCE = ICON_DIR / "source.png"
WEB_DIR = ROOT / "public"

CANVAS = 1024
# macOS 标准网格：图形本体占画布的比例（824/1024，四周各留 100）
PLATE = 824
REFERENCE_APP = Path("/Applications/红果短剧.app")
# Tauri bundle.icon 声明的各档
PNG_SIZES = (32, 128, 256, 512, 1024)
# macOS .icns 的标准档位（每种各带 @2x）
ICNS_SIZES = (16, 32, 128, 256, 512)
# 应用内界面（侧边栏等）用的边长。**不带标准留白**：那套留白是给系统
# Dock/任务栏摆位用的，界面里按图形本体铺满才不会被读成「缩了一圈」。
WEB_ICON_SIZE = 128


def run(args: list[str]) -> None:
    subprocess.run(args, check=True)


def magick(*args: str, out: Path) -> None:
    """跑一条 magick：`magick <args...> <out>`。"""
    run(["magick", *args, str(out)])


def require_tools() -> None:
    missing = [t for t in ("magick",) if not shutil.which(t)]
    if missing:
        sys.exit(f"缺少工具：{', '.join(missing)}（brew install imagemagick）")


def extract_reference() -> None:
    """从参考端 .icns 里提取 1024 原图（source.png 缺失时的兜底）。"""
    icns = REFERENCE_APP / "Contents" / "Resources" / "iconfile.icns"
    if not shutil.which("iconutil") or not icns.exists():
        sys.exit(
            f"缺少图标源 {SOURCE.relative_to(ROOT)}——"
            "放一张 1024×1024 的 PNG 到该路径后再跑。"
        )
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "ref.iconset"
        run(["iconutil", "-c", "iconset", "-o", str(iconset), str(icns)])
        best = iconset / "icon_512x512@2x.png"
        if not best.exists():
            sys.exit(f"参考端 .icns 里没有 1024 图（{iconset}）")
        shutil.copyfile(best, SOURCE)
    print(f"从参考端提取图标源 → {SOURCE.relative_to(ROOT)}")


def build_master() -> Path:
    """1024 画布 + 居中 824 图形（macOS 标准网格），落 /tmp 备用。"""
    out = Path(tempfile.mkdtemp()) / "master.png"
    plate = out.with_name("plate.png")
    magick(str(SOURCE), "-filter", "Lanczos", "-resize", f"{PLATE}x{PLATE}", out=plate)
    magick("-size", f"{CANVAS}x{CANVAS}", "xc:none", str(plate), "-gravity", "center",
           "-composite", out=out)
    return out


def write_pngs(master: Path) -> None:
    """Tauri bundle.icon 声明的各尺寸（Windows 任务栏与 Linux 的包都吃它）。"""
    for size in PNG_SIZES:
        magick(str(master), "-filter", "Lanczos", "-resize", f"{size}x{size}",
               out=ICON_DIR / f"{size}x{size}.png")
        print(f"wrote {size}x{size}.png")
    # Retina 那档的惯例命名：128 点的 @2x = 256 像素
    magick(str(master), "-filter", "Lanczos", "-resize", "256x256",
           out=ICON_DIR / "128x128@2x.png")
    print("wrote 128x128@2x.png (256x256)")
    shutil.copyfile(master, ICON_DIR / "icon.png")
    print("wrote icon.png (1024x1024)")


def write_ico(master: Path) -> None:
    """Windows：.ico 内嵌多尺寸，任务栏与资源管理器各取所需。"""
    magick(str(master), "-define", "icon:auto-resize=256,128,64,48,32,24,16",
           out=ICON_DIR / "icon.ico")
    print("wrote icon.ico (16..256)")


def write_icns(master: Path) -> None:
    """macOS：走 iconutil 生成标准 iconset（16..512 及各自 @2x）。

    PIL 的 ICNS 写出器只塞它拿到的几档、尺寸表常有缺口，Dock 取不到
    对应档位就会拿小图放大——正是「装完图标发虚」的另一个来源。
    """
    if not shutil.which("iconutil"):
        print("跳过 icon.icns：非 macOS（另有各尺寸 PNG 可用）")
        return
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        for size in ICNS_SIZES:
            magick(str(master), "-filter", "Lanczos", "-resize", f"{size}x{size}",
                   out=iconset / f"icon_{size}x{size}.png")
            magick(str(master), "-filter", "Lanczos", "-resize", f"{size * 2}x{size * 2}",
                   out=iconset / f"icon_{size}x{size}@2x.png")
        run(["iconutil", "-c", "icns", "-o", str(ICON_DIR / "icon.icns"), str(iconset)])
    print("wrote icon.icns (16..512 + @2x)")


def write_web_icon() -> None:
    """界面内用的那份：按图形本体裁掉标准留白（见文件头说明）。

    走 public/ 而不是 src/assets：Vite 对 public/ 是原样拷贝，
    引用时不需要 import 路径哈希。
    """
    WEB_DIR.mkdir(parents=True, exist_ok=True)
    magick(str(SOURCE), "-trim", "+repage", "-filter", "Lanczos", "-resize",
           f"{WEB_ICON_SIZE}x{WEB_ICON_SIZE}", out=WEB_DIR / "app-icon.png")
    print(f"wrote public/app-icon.png ({WEB_ICON_SIZE}x{WEB_ICON_SIZE}, 无留白)")


def main() -> None:
    require_tools()
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    if not SOURCE.exists():
        extract_reference()
    master = build_master()
    write_pngs(master)
    write_ico(master)
    write_icns(master)
    write_web_icon()


if __name__ == "__main__":
    main()
