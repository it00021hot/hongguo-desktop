#!/usr/bin/env python3
"""Summarize playback JSONL captures without exporting credentials or media URLs."""
import argparse
import base64
import binascii
import json
from pathlib import Path
import re
import zlib
from urllib.parse import parse_qsl, urlsplit


MAX_DECODED_BYTES = 8 * 1024 * 1024
IDENTITY_NAMES = {"iid", "install_id", "device_id", "device_id_str", "install_id_str",
                  "cdid", "openudid", "clientudid", "client_udid", "klink_egdi"}
VIDEO_NAMES = {"video_id", "videoId", "vid"}
SIGNATURE_NAMES = {"x-argus", "x-ladon", "x-helios", "x-medusa"}
ENDPOINTS = {
    "/service/2/device_register/": "register",
    "/service/2/app_log/": "startup",
    "/service/2/app_alert_check/": "startup",
    "/passport/account/info/v2/": "startup",
    "/passport/general/login_guiding_strategy/": "startup",
    "/reading/bookapi/read_progress/upload/v": "history",
    "/reading/bookapi/read_history/update/v": "history",
}
VIDEO_ID = re.compile(r"[0-9]{1,40}\Z")
PUBLIC_QUERY = {
    "aid": r"[0-9]{1,10}", "version_code": r"[0-9]{1,10}",
    "manifest_version_code": r"[0-9]{1,10}", "update_version_code": r"[0-9]{1,10}",
    "version_name": r"[0-9.]{1,20}", "need_personal_recommend": r"[01]",
}
MODEL_KEYS = {"video_list", "version", "duration", "video_duration", "video_id", "vid",
              "video_status", "total", "enable_ssl", "media_type", "video_info"}
PUBLIC_HEADER = {
    "content-encoding": r"(?:gzip|identity|br|deflate|zstd)",
    "content-type": r"(?:application/(?:json|x-protobuf|octet-stream|x-www-form-urlencoded)|text/plain)"
                    r"(?:;\s*charset=utf-8|;\s*tt-data=a)?",
    "x-ss-dp": r"[0-9]{1,10}", "lc": r"[0-9]{1,10}",
    "x-tt-store-region": r"[a-z]{2}-[a-z]{2,3}", "x-tt-store-region-src": r"(?:did|uid)",
}
BIZ_BOOLEAN_FIELDS = {"disable_digg_stat", "disable_video_relate_book", "need_all_video_definition",
                      "need_mp4_align", "use_os_player", "use_server_dns"}
BIZ_INTEGER_FIELDS = {"detail_page_version", "device_level", "video_platform", "video_id_type", "source"}
DNS_SCENES = {"", "cold_start_stage", "player", "preload"}
DR_SCENES = {"", "default", "player", "preload"}


def read_captures(paths):
    reporter, errors = CaptureReporter(), []
    for path in paths:
        with Path(path).open(encoding="utf-8") as source:
            for number, line in enumerate(source, 1):
                try:
                    row = json.loads(line)
                except json.JSONDecodeError:
                    errors.append({"source": Path(path).name, "line": number, "error": "invalidJson"})
                    continue
                if not isinstance(row, dict):
                    errors.append({"source": Path(path).name, "line": number, "error": "invalidRecord"})
                    continue
                reporter.add(row, Path(path).name, number)
    result = reporter.result()
    result["errors"] = errors
    return result


def summarize(rows):
    reporter = CaptureReporter()
    for row in rows:
        reporter.add(row)
    return reporter.result()


class CaptureReporter:
    def __init__(self):
        self.records = []
        self.labels = {}
        self.identities = {}
        self.videos = {}

    def add(self, row, source=None, line=None):
        path = urlsplit(str(row.get("path", "")))
        family = endpoint_family(path.path)
        if family is None:
            return
        number = len(self.records) + 1
        req_headers = header_pairs(row.get("req_headers"))
        resp_headers = header_pairs(row.get("resp_headers"))
        req_body, req_value = body_summary(row, "req")
        resp_body, resp_value = body_summary(row, "resp")
        query = [self.parameter(key, value, number)
                 for key, value in parse_qsl(path.query, keep_blank_values=True)]
        req_cookies = cookie_summary(req_headers, self, number)
        collect_body_identities(req_value, self, number)
        collect_body_identities(resp_value, self, number)
        req_body.update(request_details(req_value))
        resp_body.update(response_details(resp_value))
        video_ids = req_body["videoIds"] + [item["value"] for item in query
                                                  if item["name"] in VIDEO_NAMES and "value" in item]
        for video_id in dict.fromkeys(video_ids):
            self.videos.setdefault(video_id, []).append(number)
        self.records.append({
            "record": number, "source": source, "line": line,
            "family": family, "method": row.get("method"),
            "host": safe_host(row.get("host")), "path": path.path,
            "status": row.get("status"), "query": query,
            "requestHeaders": header_summary(req_headers),
            "responseHeaders": header_summary(resp_headers),
            "requestCookies": req_cookies,
            "responseCookies": response_cookie_summary(row, resp_headers),
            "requestBody": req_body, "responseBody": resp_body,
        })

    def parameter(self, name, value, number):
        item = {"name": name, "valueLength": len(value)}
        if name in IDENTITY_NAMES:
            item["identity"] = self.identity(value, name, number)
        elif name in VIDEO_NAMES and VIDEO_ID.fullmatch(value):
            item["value"] = value
        elif name in PUBLIC_QUERY and re.fullmatch(PUBLIC_QUERY[name], value):
            item["value"] = value
        return item

    def identity(self, value, name, number):
        value = str(value)
        if not value:
            return None
        label = self.labels.setdefault(value, "identity-{:03d}".format(len(self.labels) + 1))
        group = self.identities.setdefault(label, {"fields": set(), "records": set()})
        group["fields"].add(name)
        group["records"].add(number)
        return label

    def result(self):
        return {"schemaVersion": 1, "records": self.records, "videoGroups": self.videos,
                "identityGroups": {label: {"fields": sorted(group["fields"]),
                                            "records": sorted(group["records"])}
                                   for label, group in self.identities.items()}, "errors": []}


def endpoint_family(path):
    if path.startswith(("/novel/player/multi_video_model/", "/novel/player/video_model/")):
        return "model"
    return ENDPOINTS.get(path)


def safe_host(value):
    if not isinstance(value, str) or not re.fullmatch(r"[a-zA-Z0-9.-]+(?::[0-9]+)?", value):
        return None
    return value


def header_pairs(headers):
    if isinstance(headers, dict):
        return [(str(name), str(value)) for name, value in headers.items()]
    if isinstance(headers, list):
        return [(str(pair[0]), str(pair[1])) for pair in headers
                if isinstance(pair, (list, tuple)) and len(pair) == 2]
    return None


def header_summary(headers):
    if headers is None:
        return None
    result = []
    for name, value in headers:
        item = {"name": name.lower(), "valueLength": len(value)}
        rule = PUBLIC_HEADER.get(name.lower())
        if rule and re.fullmatch(rule, value, re.IGNORECASE):
            item["value"] = value
        if name.lower() in SIGNATURE_NAMES:
            try:
                item["decodedBytes"] = len(base64.b64decode(value, validate=True))
            except (ValueError, binascii.Error):
                item["decodedBytes"] = None
        result.append(item)
    return result


def cookie_summary(headers, reporter, number):
    if headers is None:
        return None
    cookies = []
    for name, value in headers:
        if name.lower() != "cookie":
            continue
        for pair in value.replace(",", ";").split(";"):
            key, separator, content = pair.strip().partition("=")
            if separator:
                if key in IDENTITY_NAMES:
                    reporter.identity(content, key, number)
                cookies.append({"name": key, "valueLength": len(content)})
    return cookies


def response_cookie_summary(row, headers):
    lines = [value for name, value in headers or [] if name.lower() == "set-cookie"]
    if headers is None:
        lines = row.get("set_cookies")
    if not isinstance(lines, list):
        return None
    result = []
    for line in lines:
        name, separator, value = str(line).split(";", 1)[0].partition("=")
        if separator:
            result.append({"name": name.strip(), "valueLength": len(value)})
    return result


def collect_body_identities(value, reporter, number):
    if not isinstance(value, dict):
        return
    containers = [value, value.get("header", {})]
    for container in containers:
        if not isinstance(container, dict):
            continue
        for name in sorted(IDENTITY_NAMES.intersection(container)):
            field = container[name]
            if isinstance(field, (str, int)):
                reporter.identity(field, name, number)


def body_summary(row, prefix):
    raw_count = byte_count(row.get(prefix + "_raw_bytes"))
    raw_truncated = row.get(prefix + "_body_truncated")
    if not isinstance(raw_truncated, bool):
        raw_truncated = None
    body = row.get(prefix + "_body")
    decoded_body = row.get("resp_decoded_body") if prefix == "resp" else None
    use_decoded = isinstance(decoded_body, str)
    decoded_count = byte_count(row.get("resp_decoded_bytes")) if use_decoded else None
    truncated = row.get("resp_decoded_body_truncated") if use_decoded else raw_truncated
    if not isinstance(truncated, bool):
        truncated = None
    summary = {"rawBytes": raw_count, "decodedBytes": decoded_count,
               "bodySource": "decoded" if use_decoded else "raw",
               "rawIsTruncated": raw_truncated, "isTruncated": truncated, "state": "unknown"}
    if use_decoded:
        body = decoded_body
    if truncated:
        summary["state"] = "truncated"
        return summary, None
    if not isinstance(body, str):
        return summary, None
    if not body:
        body_count = decoded_count if use_decoded else raw_count
        summary["state"] = "empty" if body_count == 0 else "unknown"
        return summary, None
    try:
        raw = base64.b64decode(body[4:], validate=True) if body.startswith("b64:") else body.encode()
        if not use_decoded and raw.startswith(b"\x1f\x8b"):
            decoder = zlib.decompressobj(zlib.MAX_WBITS | 16)
            raw = decoder.decompress(raw, MAX_DECODED_BYTES + 1)
            if len(raw) > MAX_DECODED_BYTES or not decoder.eof:
                raise ValueError("gzip limit or incomplete stream")
        if len(raw) > MAX_DECODED_BYTES:
            raise ValueError("body limit")
        value = json.loads(raw)
    except (ValueError, UnicodeError, binascii.Error, zlib.error):
        summary["state"] = "unparsed"
        return summary, None
    summary["state"] = "json"
    return summary, value


def byte_count(value):
    return value if type(value) is int and value >= 0 else None


def request_details(value):
    details = {"videoIds": [], "mixedVideoIdMap": None, "bodyKeys": None,
               "bizParamKeys": None, "bizParam": None, "drScene": None,
               "contentType": None, "videoContentType": None}
    if not isinstance(value, dict):
        return details
    details["bodyKeys"] = sorted(value)
    biz = value.get("biz_param")
    if isinstance(biz, dict):
        details["bizParamKeys"] = sorted(biz)
        details["bizParam"] = {name: item for name, item in biz.items() if public_business_value(name, item)}
    scene = value.get("dr_scene")
    if isinstance(scene, str) and scene in DR_SCENES:
        details["drScene"] = scene
    for source, target in [("content_type", "contentType"), ("video_content_type", "videoContentType")]:
        if type(value.get(source)) is int:
            details[target] = value[source]
    video_ids = [str(value[name]) for name in VIDEO_NAMES.intersection(value)
                 if VIDEO_ID.fullmatch(str(value[name]))]
    mixed = value.get("mixed_video_id_map", value.get("mixedVideoIdMap"))
    if isinstance(mixed, dict):
        groups = {key: [str(item) for item in items if VIDEO_ID.fullmatch(str(item))]
                  for key, items in mixed.items() if VIDEO_ID.fullmatch(key) and isinstance(items, list)}
        details["mixedVideoIdMap"] = groups
        video_ids.extend(item for items in groups.values() for item in items)
    details["videoIds"] = list(dict.fromkeys(video_ids))
    return details


def public_business_value(name, value):
    if name in BIZ_BOOLEAN_FIELDS:
        return type(value) is bool
    if name in BIZ_INTEGER_FIELDS:
        return type(value) is int
    if name == "use_server_dns_scene":
        return isinstance(value, str) and value in DNS_SCENES
    if name == "from_video_id":
        return isinstance(value, str) and (value == "" or bool(VIDEO_ID.fullmatch(value)))
    return False


def response_details(value):
    details = {"models": [], "code": None}
    if not isinstance(value, dict):
        return details
    code = value.get("code", value.get("Code"))
    details["code"] = code if isinstance(code, int) and not isinstance(code, bool) else None
    data = value.get("data")
    if not isinstance(data, dict):
        return details
    entries = [(None, data)] if "video_model" in data else data.items()
    for video_id, entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("video_model"), str):
            continue
        try:
            model = json.loads(entry["video_model"])
        except (ValueError, TypeError):
            continue
        if not isinstance(model, dict):
            continue
        streams = model.get("video_list")
        metas = [stream.get("video_meta", {}) for stream in streams if isinstance(stream, dict)] if isinstance(streams, list) else []
        codecs = sorted({meta["codec_type"] for meta in metas if isinstance(meta, dict)
                         and meta.get("codec_type") in {"h264", "h265", "hevc", "bytevc1", "bytevc2", "avc1", "hvc1"}})
        details["models"].append({"videoId": video_id if isinstance(video_id, str) and VIDEO_ID.fullmatch(video_id) else None,
                                  "keys": sorted(MODEL_KEYS.intersection(model)),
                                  "streamCount": len(streams) if isinstance(streams, list) else None,
                                  "codecs": codecs})
    return details


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("captures", type=Path, nargs="+")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = read_captures(args.captures)
    except (OSError, UnicodeError):
        parser.exit(1, "Cannot read capture input.\n")
    output = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(output, encoding="utf-8")
    else:
        print(output, end="")


if __name__ == "__main__":
    main()
