"""临时:找聚宝仙盆(7691228619774905368)剧评列表的评分/标签统计字段(用后即删)。"""
import base64
import gzip
import json

import brotli

TARGET = "7691228619774905368"
n = 0
for line in open("captures/flows-review.jsonl", encoding="utf-8"):
    try:
        e = json.loads(line)
    except Exception:
        continue
    if "/comment/list/" not in e.get("path", ""):
        continue
    if TARGET not in e["path"]:
        continue
    body = e.get("req_body") or ""
    if body.startswith("b64:"):
        try:
            body = gzip.decompress(base64.b64decode(body[4:])).decode("utf-8")
        except Exception:
            body = ""
    try:
        j = json.loads(body) if body.startswith("{") else {}
    except Exception:
        continue
    if j.get("group_type") != 1:
        continue
    n += 1
    if n > 1:
        break
    raw = e.get("resp_body") or ""
    d = json.loads(brotli.decompress(base64.b64decode(raw[4:])))
    data = d.get("data") or {}
    extra = data.get("extra") or {}
    print("extra 键 =", sorted(extra.keys()))
    for k, v in extra.items():
        if k == "book_info":
            bi = v or {}
            picks = {
                kk: vv
                for kk, vv in bi.items()
                if any(t in kk.lower() for t in ("score", "tag", "count", "collect"))
            }
            print("book_info 评分/标签相关 =", json.dumps(picks, ensure_ascii=False)[:500])
        else:
            print(k, "=", json.dumps(v, ensure_ascii=False)[:500])
    cli = data.get("common_list_info") or {}
    print("total =", cli.get("total"))
    # 每条的 tag/expand 字段
    lst = data.get("data_list") or []
    if lst:
        c0 = lst[0].get("comment") or {}
        expand = c0.get("expand") or {}
        print("expand 全键 =", sorted(expand.keys()))
        tl = expand.get("comment_tag_list") or expand.get("tag_list") or None
        if tl:
            print("expand.comment_tag_list =", json.dumps(tl, ensure_ascii=False)[:400])
        common = c0.get("common") or {}
        ctl = common.get("comment_tag_list")
        if ctl:
            print("common.comment_tag_list =", json.dumps(ctl, ensure_ascii=False)[:400])
