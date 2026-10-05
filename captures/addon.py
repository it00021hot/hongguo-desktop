"""mitmproxy addon: dump 红果相关域名的请求与响应到 JSONL。

用法（自持抓包工作流，详见 docs/hongguo-api-endpoints.md）：
    mitmdump.exe -p 8080 -s captures/addon.py
输出可用环境变量 MITM_OUT 覆盖，默认 captures/flows-YYYYMMDD.jsonl。
"""
import json
import os
import time

_DEFAULT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        time.strftime("flows-%Y%m%d.jsonl"))
OUT = os.environ.get("MITM_OUT", _DEFAULT)
TARGETS = ("snssdk.com", "fqnovel.com", "byteoversea.com", "toutiao.com", "zjcdn.com", "ixigua.com")


def response(flow):
    host = flow.request.pretty_host
    if not any(t in host for t in TARGETS):
        return
    import base64

    def safe_text(raw: bytes | None) -> str:
        if not raw:
            return ""
        try:
            t = raw.decode("utf-8")
            # 控制字符（protobuf 二进制）会打坏 JSONL：转 base64 保真
            if any(ord(c) < 0x20 and c not in "\t" for c in t):
                raise UnicodeDecodeError("utf-8", raw, 0, 1, "ctrl")
            return t
        except UnicodeDecodeError:
            return "b64:" + base64.b64encode(raw).decode("ascii")

    req_body = safe_text(flow.request.raw_content)
    resp_body = safe_text(flow.response.raw_content)
    entry = {
        "t": time.time(),
        "method": flow.request.method,
        "host": host,
        "path": flow.request.path,
        "req_headers": dict(flow.request.headers.items()),
        "req_body": req_body[:200000],
        "status": flow.response.status_code,
        "resp_body": resp_body[:500000],
    }
    with open(OUT, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
