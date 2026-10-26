"""mitmproxy addon: dump 红果相关域名的请求与响应到 JSONL。

用法（自持抓包工作流，详见 docs/hongguo-api-endpoints.md）：
    mitmdump.exe -p 8080 -s captures/addon.py
输出可用环境变量 MITM_OUT 覆盖，默认 captures/flows-YYYYMMDD.jsonl。

两路输出：
- 全量 dump（请求头/请求体/响应体）：仅 TARGETS 命中的域名；
- 轻量索引（方法/域名/路径/状态/类型/长度）：所有域名，写 MITM_INDEX_OUT。
  用于发现目标名单外的域名（如图片上传、第三方 GIF 源），确认后再补进 TARGETS 重抓。
"""
import json
import os
import time

_DEFAULT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        time.strftime("flows-%Y%m%d.jsonl"))
OUT = os.environ.get("MITM_OUT", _DEFAULT)
INDEX_OUT = os.environ.get("MITM_INDEX_OUT", OUT.replace(".jsonl", "-index.jsonl"))
TARGETS = ("snssdk.com", "fqnovel.com", "byteoversea.com", "toutiao.com", "zjcdn.com",
           "ixigua.com", "pstatp.com", "zijieapi.com", "bytedance.com", "ibytedtos.com",
           "byteimg.com", "bytegoofy.com", "bytedapm.com", "gifshow.com", "amemv.com")


def response(flow):
    host = flow.request.pretty_host
    matched = any(t in host for t in TARGETS)
    if not matched:
        _index(flow, host)
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
    # Set-Cookie 是身份票（ttreq/odin_tt）下发的唯一观测点，必须落盘
    set_cookies = [v for k, v in flow.response.headers.items(True) if k.lower() == "set-cookie"]
    entry = {
        "t": time.time(),
        "method": flow.request.method,
        "host": host,
        "path": flow.request.path,
        "req_headers": dict(flow.request.headers.items()),
        "req_body": req_body[:200000],
        "status": flow.response.status_code,
        "set_cookies": set_cookies,
        "resp_body": resp_body[:500000],
    }
    with open(OUT, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")


def _index(flow, host):
    """非目标域名只留一行轻量索引，不落请求/响应体。"""
    resp = flow.response
    entry = {
        "t": time.time(),
        "method": flow.request.method,
        "host": host,
        "path": flow.request.path.split("?", 1)[0],
        "status": resp.status_code,
        "content_type": resp.headers.get("content-type", ""),
        "content_length": resp.headers.get("content-length", ""),
    }
    with open(INDEX_OUT, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
