#!/usr/bin/env python3
"""生成应用图标：已确认的番果果实播放图标与 macOS 留白网格。

只依赖 ImageMagick（`magick`）与 macOS 自带的 `iconutil`，不需要 Pillow——
脚本会在缺工具时给出明确报错。

## 图形来源（assets/icons/source.png）

用户确认的圆润果实设计：暖白圆角底板、珊瑚橙果实、青绿叶片与浅色播放键。
source.png 为独立图标原图；源文件缺失时明确失败，不从其他应用提取图标。

## 为什么要有留白

macOS 的应用图标有一套固定的「画布网格」：1024 画布里图形本体占 824
（四周各留 100），贴到 Dock 才和系统应用一样大。计算器/备忘录/Chrome
实测一致——图形占画布 80.5%。官方壳与旧版我们的图标都是**满幅 1024**，
没有留白，Dock 里就比别的应用大一圈。这里统一按标准网格重新摆位；
导出时统一圆角边界，去掉生成源图在透明区域残留的零散像素。
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICON_DIR = ROOT / "assets" / "icons"
SOURCE = ICON_DIR / "source.png"

CANVAS = 1024
# macOS 标准网格：图形本体占画布的比例（824/1024，四周各留 100）
PLATE = 824
CORNER_RADIUS_RATIO = 0.225
# 应用图标的常用分辨率。
PNG_SIZES = (32, 128, 256, 512, 1024)
# macOS .icns 的标准档位（每种各带 @2x）
ICNS_SIZES = (16, 32, 128, 256, 512)


def run(args: list[str]) -> None:
    subprocess.run(args, check=True)


def magick(*args: str, out: Path) -> None:
    """跑一条 magick：`magick <args...> <out>`。"""
    run(["magick", *args, str(out)])


def require_tools() -> None:
    missing = [t for t in ("magick",) if not shutil.which(t)]
    if missing:
        sys.exit(f"缺少工具：{', '.join(missing)}（brew install imagemagick）")


def build_master() -> Path:
    """1024 画布 + 居中 824 图形（macOS 标准网格），落 /tmp 备用。"""
    out = Path(tempfile.mkdtemp()) / "master.png"
    plate = out.with_name("plate.png")
    magick(str(SOURCE), "-filter", "Lanczos", "-resize", f"{PLATE}x{PLATE}", out=plate)
    mask = out.with_name("mask.png")
    radius = PLATE * CORNER_RADIUS_RATIO
    magick("-size", f"{PLATE}x{PLATE}", "xc:none", "-fill", "white", "-draw",
           f"roundrectangle 0,0 {PLATE - 1},{PLATE - 1} {radius},{radius}", out=mask)
    magick(str(plate), str(mask), "-compose", "CopyOpacity", "-composite", "-depth", "8", out=plate)
    magick("-size", f"{CANVAS}x{CANVAS}", "xc:none", str(plate), "-gravity", "center",
           "-composite", out=out)
    return out


def write_pngs(master: Path) -> None:
    """生成原生应用使用的图标尺寸。"""
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


def main() -> None:
    require_tools()
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    if not SOURCE.exists():
        sys.exit(f"缺少已确认的图标源 {SOURCE.relative_to(ROOT)}")
    master = build_master()
    write_pngs(master)
    write_ico(master)
    write_icns(master)


if __name__ == "__main__":
    main()
