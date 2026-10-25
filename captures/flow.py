#!/usr/bin/env python3
"""抓包流量查询：按关键词过滤 JSONL 抓包，打印请求参数与响应体。

用法:
    python captures/flow.py emoticon            # 列出匹配流量的方法/域名/路径与参数
    python captures/flow.py emoticon --full     # 附带完整请求体与响应体
    python captures/flow.py comment/add --full
"""
import glob
import json
import os
import sys
from urllib.parse import parse_qsl, urlparse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from audit import body_text  # noqa: E402  复用响应体解压逻辑

ROOT = os.path.dirname(os.path.abspath(__file__))


def load(patterns: list[str]) -> list[dict]:
    rows = []
    for fn in sorted(glob.glob(os.path.join(ROOT, "flows-*.jsonl"))):
        if fn.endswith("-index.jsonl"):
            continue
        with open(fn, encoding="utf-8") as f:
            for line in f:
                try:
                    d = json.loads(line)
                except Exception:
                    continue
                hay = f"{d.get('host','')} {d.get('path','')}"
                if any(p in hay for p in patterns):
                    rows.append(d)
    return rows


def main() -> None:
    patterns = [a for a in sys.argv[1:] if not a.startswith("--")]
    full = "--full" in sys.argv
    rows = load(patterns)
    print(f"匹配 {len(rows)} 条流量")
    for i, d in enumerate(rows):
        u = urlparse(d.get("path", ""))
        q = [(k, v) for k, v in parse_qsl(u.query) if k not in {
            "aid", "ac", "app_name", "channel", "compliance_status", "device_id", "device_platform",
            "iid", "klink_egdi", "manifest_version_code", "version_code", "version_name", "os_api",
            "os_version", "resolution", "dpi", "device_type", "device_brand", "host_abi", "language",
            "cdid", "openudid", "update_version_code", "ssmix", "ts", "nonce", "_rticket", "upload_device_id",
            "os", "device_platform_type", "os_version"}]
        print(f"\n[{i}] {d.get('method')} {d.get('host')}{u.path}  status={d.get('status')}")
        if q:
            print("    query:", ", ".join(f"{k}={v[:60]}" for k, v in q))
        body = body_text(d.get("req_body", ""))
        if body:
            print("    req:", body[:2000])
        if full:
            resp = body_text(d.get("resp_body", ""))
            print("    resp:", resp[:4000])


if __name__ == "__main__":
    main()
