#!/usr/bin/env python3
"""官方 App UI 驱动器（uiautomator）：导出可交互节点、按文本/描述点击。

用法:
    python captures/ui.py dump                 # 打印当前界面可交互节点
    python captures/ui.py tap 抢首评            # 按 text / content-desc 点击（取中心点）
    python captures/ui.py tap-id com.x:id       # 按 resource-id 点击
    python captures/ui.py shot shots/x.png      # 截图落盘
"""
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

PACKAGE = "com.phoenix.read"


def adb(*args: str) -> str:
    return subprocess.run(["adb", *args], capture_output=True, text=True).stdout


def dump_xml() -> str:
    adb("shell", "uiautomator", "dump", "/sdcard/hg-ui.xml")
    return adb("shell", "cat", "/sdcard/hg-ui.xml")


def nodes(xml: str) -> list[dict]:
    root = ET.fromstring(xml)
    out = []

    def walk(el):
        attrib = el.attrib
        text = attrib.get("text", "")
        desc = attrib.get("content-desc", "")
        rid = attrib.get("resource-id", "")
        cls = attrib.get("class", "")
        bounds = attrib.get("bounds", "")
        clickable = attrib.get("clickable") == "true"
        if text or desc or rid or clickable:
            out.append({"text": text, "desc": desc, "rid": rid, "cls": cls,
                        "bounds": bounds, "clickable": clickable})
        for child in el:
            walk(child)

    walk(root)
    return out


def center(bounds: str) -> tuple[int, int]:
    m = re.match(r"\[(\d+),(\d+)]\[(\d+),(\d+)]", bounds)
    if not m:
        raise ValueError(f"bad bounds: {bounds}")
    x1, y1, x2, y2 = (int(v) for v in m.groups())
    return (x1 + x2) // 2, (y1 + y2) // 2


def main() -> None:
    cmd = sys.argv[1] if len(sys.argv) > 1 else "dump"
    xml = dump_xml()
    items = nodes(xml)

    if cmd == "dump":
        for n in items:
            label = n["text"] or n["desc"] or n["rid"] or n["cls"]
            if not n["text"] and not n["desc"] and not n["rid"] and not n["clickable"]:
                continue
            print(f"{n['bounds']:>26}  click={int(n['clickable'])}  {n['cls'].split('.')[-1]:<18} {label}")
        return

    if cmd == "tap":
        needle = sys.argv[2]
        for n in items:
            if n["text"] == needle or n["desc"] == needle:
                x, y = center(n["bounds"])
                adb("shell", "input", "tap", str(x), str(y))
                print(f"tap {needle} @ {x},{y}")
                return
        raise SystemExit(f"not found: {needle}")

    if cmd == "tap-id":
        needle = sys.argv[2]
        for n in items:
            if n["rid"].endswith(needle):
                x, y = center(n["bounds"])
                adb("shell", "input", "tap", str(x), str(y))
                print(f"tap-id {needle} @ {x},{y}")
                return
        raise SystemExit(f"not found: {needle}")

    if cmd == "shot":
        out = sys.argv[2]
        adb("shell", "screencap", "-p", "/sdcard/hg-shot.png")
        subprocess.run(["adb", "pull", "/sdcard/hg-shot.png", out], capture_output=True)
        print(f"saved {out}")
        return

    raise SystemExit(f"unknown command: {cmd}")


if __name__ == "__main__":
    main()
