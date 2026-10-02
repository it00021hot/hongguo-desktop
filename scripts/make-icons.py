"""生成应用图标：官方红果 logo + 微信风格圆角外框。

直接照搬官方 logo 会有两个问题：
1. 官方图形本身是小尺寸方形，贴到任务栏后四角是硬的，和微信那种
   「整块圆角底板 + 居中图形」的观感差很远；
2. 官方 PNG 只有 160px，直接放大到 256/512 会糊。

所以这里统一按 1024 画布重绘：圆角矩形底板 → 官方图形居中等比缩放 →
浅色内描边，全部用 LANCZOS 一次性降采样到各尺寸，边缘才不会有锯齿。
描边是用底板自身的亮色描一圈，在深色任务栏上能看出图标边界
（微信正是这么做的），纯色方块贴在深色背景上会「消失」。
"""

from pathlib import Path

from PIL import Image, ImageDraw

# 官方 logo（站点 <link rel="icon"> 指向的图）
SOURCE = Path.home() / "AppData/Local/Temp/hg-official-logo.png"
ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "src-tauri/icons"
WEB_DIR = ROOT / "public"

CANVAS = 1024
# 应用内界面用的图标边长。32px 侧边栏方块在高 DPI 屏上会发虚，
# 出 128px 让浏览器自己降采样，与系统图标的观感一致。
WEB_ICON_SIZE = 128
# 圆角半径占边长的比例。微信桌面端图标约 22.5%，
# 这里取 24% —— 任务栏 24px 显示时四角才不会显得过尖。
CORNER_RATIO = 0.24
# 官方 logo 在底板里的占比。留出边距让圆角底板看得出来。
LOGO_RATIO = 0.72
# 内描边宽度（画布像素）与颜色
BORDER_WIDTH = 10
BORDER_COLOR = (255, 255, 255, 235)


def rounded_mask(size: int, radius: int) -> Image.Image:
    """圆角矩形遮罩：圆内全白，圆外全黑。"""
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, size - 1, size - 1), radius=radius, fill=255
    )
    return mask


def build_base() -> Image.Image:
    """1024 画布的底板：官方图形填满 + 圆角遮罩 + 内描边。

    官方图是 40×40 的小图且自带圆角与透明角，比例接近方形，
    所以按「填满画布再由遮罩统一裁圆角」处理——让圆角完全由这里决定，
    不受源图自带圆角的影响。
    """
    base = Image.open(SOURCE).convert("RGBA")

    # 源图只有 40px，直接拉到 1024 边缘会糊；先按 4 倍放大取中间结果，
    # 再由最终的 LANCZOS 降采样做抗锯齿
    big = base.resize((CANVAS, CANVAS), Image.LANCZOS)

    # 圆角遮罩裁掉方形四角
    mask = rounded_mask(CANVAS, int(CANVAS * CORNER_RATIO))
    big.putalpha(mask)

    # 内描边画在遮罩之内，跟着圆角走
    stroke = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    ImageDraw.Draw(stroke).rounded_rectangle(
        (BORDER_WIDTH, BORDER_WIDTH, CANVAS - 1 - BORDER_WIDTH, CANVAS - 1 - BORDER_WIDTH),
        radius=int(CANVAS * CORNER_RATIO),
        outline=BORDER_COLOR,
        width=BORDER_WIDTH,
    )
    big.alpha_composite(stroke)
    return big


def main() -> None:
    base = build_base()
    WEB_DIR.mkdir(parents=True, exist_ok=True)

    # Tauri bundle.icon 声明的尺寸
    for size in (32, 128, 256, 512, 1024):
        out = base.resize((size, size), Image.LANCZOS)
        name = f"{size}x{size}.png" if size not in (256, 512, 1024) else f"{size}x{size}.png"
        out.save(OUT_DIR / name)
        print(f"wrote {name} ({size}x{size})")

    # 128x128@2x 是 Retina，惯例就是 256 那张
    base.resize((256, 256), Image.LANCZOS).save(OUT_DIR / "128x128@2x.png")
    print("wrote 128x128@2x.png (256x256)")

    base.save(OUT_DIR / "icon.png")
    print("wrote icon.png (1024x1024)")

    # Windows: .ico 内嵌多尺寸，任务栏与资源管理器各取所需
    base.save(
        OUT_DIR / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("wrote icon.ico (16..256)")

    # macOS: .icns
    base.save(
        OUT_DIR / "icon.icns",
        format="ICNS",
        append_images=[
            base.resize((s, s), Image.LANCZOS) for s in (32, 64, 128, 256, 512)
        ],
    )
    print("wrote icon.icns")

    # 应用内界面（侧边栏左上角）用的那份。
    # 走 public/ 而不是 src/assets：Vite 对 public/ 是原样拷贝，
    # 侧边栏可以直接 <img src="/app-icon.png"> 引用，不需要 import 路径哈希。
    # 与 bundle 图标同源产出，避免两份副本各自漂移。
    web_icon = base.resize((WEB_ICON_SIZE, WEB_ICON_SIZE), Image.LANCZOS)
    web_icon.save(WEB_DIR / "app-icon.png")
    print(f"wrote public/app-icon.png ({WEB_ICON_SIZE}x{WEB_ICON_SIZE})")


if __name__ == "__main__":
    main()
