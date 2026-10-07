#!/usr/bin/env python3
"""接口参数对齐审计：从抓包 JSONL 生成每个接口的全量参数表。

用法:
    python captures/audit.py [接口名过滤词]

输出每个接口（按 path 分组）在抓包里出现过的：
- query 参数（多值时列出全部出现过的值）
- form body 字段 / JSON body 键
- Cookie 键（值脱敏为前 8 字符）
- 关键请求头

实现新接口前先跑它，对着表逐字段填——不许凭"大概有用"挑字段。
（2026-10-05 用户整顿产物：type=3731 / mfa_token / 空字段 连续丢参事故）
"""
import json
import sys
import glob
import base64
import collections
from urllib.parse import urlparse, parse_qsl
from pathlib import Path

ROOT = Path(__file__).resolve().parent

# 与业务无关的设备指纹/通用参数（这些 client.rs 统一注入，不在逐接口对齐范围）
DEVICE_QS = {
    'ac', 'aid', 'app_name', 'cdid', 'channel', 'device_brand', 'device_id',
    'device_platform', 'device_type', 'dpi', 'host_abi', 'iid', 'language',
    'manifest_version_code', 'os', 'os_api', 'os_version', 'resolution',
    'ssmix', 'update_version_code', 'version_code', 'version_name',
    '_rticket', 'ts', 'nonce', 'openudid', 'upload_device_id',
    'compliance_status', 'dragon_device_type', 'is_android_pad_screen',
    'player_so_load', 'pv_player', 'need_personal_recommend',
    'video_type_preferences_str', 'is_first_load', 'book_type',
    'last_min_read_timestamp_ms', 'full_field', ' privacy',
}
INTERESTING_HEADERS = {
    'content-type', 'x-ss-dp', 'x-reading-request', 'x-tt-token',
    'x-argus', 'x-gorgon', 'x-ladon', 'x-helios', 'x-medusa',
}


def body_text(b):
    if not isinstance(b, str) or not b:
        return ''
    if b.startswith('b64:'):
        raw = base64.b64decode(b[4:])
        for dec in (gzip_decompress, brotli_decompress):
            try:
                return dec(raw).decode('utf-8', 'replace')
            except Exception:
                continue
        return raw.decode('utf-8', 'replace')
    return b


def gzip_decompress(raw):
    import gzip as g
    return g.decompress(raw)


def brotli_decompress(raw):
    import brotli
    return brotli.decompress(raw)


def load_flows():
    rows = []
    for fn in sorted(glob.glob(str(ROOT / 'flows-*.jsonl'))):
        with open(fn, encoding='utf-8') as f:
            for line in f:
                try:
                    rows.append(json.loads(line))
                except Exception:
                    pass
    return rows


def main():
    keyword = sys.argv[1] if len(sys.argv) > 1 else ''
    groups = collections.OrderedDict()
    for f in load_flows():
        path = f.get('path', '')
        if 'fqnovel.com' not in f.get('host', '') and 'snssdk.com' not in f.get('host', ''):
            continue
        base = path.split('?', 1)[0]
        if keyword and keyword not in base:
            continue
        g = groups.setdefault(base, {'n': 0, 'qs': collections.OrderedDict(),
                                     'body': set(), 'cookies': set(), 'headers': set()})
        g['n'] += 1
        for k, v in parse_qsl(path.split('?', 1)[1] if '?' in path else ''):
            if k in DEVICE_QS:
                continue
            g['qs'].setdefault(k, set()).add(v)
        rb = body_text(f.get('req_body', ''))
        if rb:
            if '=' in rb and '&' in rb or (rb and not rb.lstrip().startswith('{')):
                for k, _ in parse_qsl(rb):
                    g['body'].add(k)
            else:
                try:
                    g['body'].update(json.loads(rb).keys())
                except Exception:
                    g['body'].add(rb[:60])
        h = f.get('req_headers', {})
        if isinstance(h, dict):
            for hk, hv in h.items():
                lk = hk.lower()
                if lk == 'cookie':
                    # addon 存的 cookie 是逗号分隔（mitmproxy 多值合并），
                    # 兼容标准分号形态
                    for part in str(hv).replace(';', ',').split(','):
                        kv = part.strip()
                        if '=' in kv:
                            name, val = kv.split('=', 1)
                            g['cookies'].add(f"{name}={val[:8]}…" if len(val) > 8 else kv)
                elif lk in INTERESTING_HEADERS:
                    g['headers'].add(f"{hk}: {str(hv)[:40]}")

    for base, g in groups.items():
        print(f"\n## {base}  ({g['n']} 次)")
        print("  query:")
        for k, vals in g['qs'].items():
            show = list(vals)
            tag = '' if len(show) == 1 else f'  [{len(show)}个值]'
            print(f"    {k} = {show[0][:60]}{tag}")
        if g['body']:
            print("  body 字段:", ', '.join(sorted(g['body'])))
        if g['cookies']:
            print("  cookie:", ', '.join(sorted(g['cookies'])))
        if g['headers']:
            print("  headers:", ' | '.join(sorted(g['headers'])))


if __name__ == '__main__':
    main()
