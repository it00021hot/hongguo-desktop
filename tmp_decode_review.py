"""临时:查剧均评分字段位置(用后即删)。"""
import base64
import gzip
import json

import brotli

n = 0
for line in open("captures/flows-review.jsonl", encoding="utf-8"):
    try:
        e = json.loads(line)
    except Exception:
        continue
    if "/comment/list/" not in e.get("path", ""):
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
    print("list extra 键 =", sorted(extra.keys()))
    bi = extra.get("book_info") or {}
    score_fields = {k: v for k, v in bi.items() if "score" in k.lower() or "rating" in k.lower()}
    print("book_info score 字段 =", json.dumps(score_fields, ensure_ascii=False))
    lst = data.get("data_list") or []
    if lst:
        expand = lst[0].get("comment", {}).get("expand") or {}
        cs = expand.get("common_stat") or {}
        print("expand.common_stat =", json.dumps(cs, ensure_ascii=False)[:300])
        print("expand.extra =", json.dumps(expand.get("extra") or {}, ensure_ascii=False)[:200])
    # 同响应找带不同 score 的条目
    scores = []
    for it in lst:
        c = it.get("comment") or {}
        ex = c.get("expand") or {}
        scores.append(ex.get("score"))
    print("本页全部 score =", scores)
