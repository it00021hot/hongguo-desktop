#!/usr/bin/env python3
"""生成应用图标：已确认的番果果实播放图标与 macOS 留白网格。

使用 ImageMagick 生成 NSH 打包用的 PNG 和 Windows 资源编译用的 ICO。

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
# NSH 应用元数据及 Android 独立宿主使用的尺寸。
PNG_SIZES = (512, 1024)


def magick(*args: str, out: Path) -> None:
    subprocess.run(["magick", *args, str(out)], check=True)


def require_tools() -> None:
    if not shutil.which("magick"):
        sys.exit("缺少 magick（brew install imagemagick）")


def build_master(directory: Path) -> Path:
    out = directory / "master.png"
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


def write_ico(master: Path) -> None:
    """Windows：.ico 内嵌多尺寸，任务栏与资源管理器各取所需。"""
    magick(str(master), "-define", "icon:auto-resize=256,128,64,48,32,24,16",
           out=ICON_DIR / "icon.ico")
    print("wrote icon.ico (16..256)")


def main() -> None:
    require_tools()
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    if not SOURCE.exists():
        sys.exit(f"缺少已确认的图标源 {SOURCE.relative_to(ROOT)}")
    with tempfile.TemporaryDirectory() as tmp:
        master = build_master(Path(tmp))
        write_pngs(master)
        write_ico(master)


if __name__ == "__main__":
    main()
