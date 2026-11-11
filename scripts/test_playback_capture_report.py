import base64
import gzip
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import playback_capture_report as report


MODEL_PATH = "/novel/player/multi_video_model/v1/"


def encoded_body(value):
    raw = gzip.compress(json.dumps(value).encode())
    return "b64:" + base64.b64encode(raw).decode(), len(raw)


def flow(**extra):
    return {
        "host": "reading.snssdk.com", "path": MODEL_PATH,
        "method": "POST", "status": 200, **extra,
    }


class CaptureReportTests(unittest.TestCase):
    def test_gzip_base64_extracts_vids_and_model_without_media_secrets(self):
        request, request_len = encoded_body({
            "mixed_video_id_map": {"1004": ["7697465038512065561"]},
            "biz_param": {"device_level": 3},
        })
        response, response_len = encoded_body({"data": {
            "7697465038512065561": {"video_model": json.dumps({
                "version": 3, "video_list": [{
                    "main_url": "https://cdn.invalid/PRIVATE-MEDIA",
                    "encrypt_info": {"spade_a": "PRIVATE-KEY"},
                    "video_meta": {"codec_type": "bytevc2", "definition": "1080p"},
                }],
            })},
        }})
        result = report.summarize([flow(
            req_body=request, resp_body=response,
            req_raw_bytes=request_len, resp_raw_bytes=response_len,
            req_body_truncated=False, resp_body_truncated=False,
        )])
        record = result["records"][0]
        self.assertEqual(record["requestBody"]["videoIds"], ["7697465038512065561"])
        self.assertEqual(record["requestBody"]["rawBytes"], request_len)
        self.assertEqual(record["responseBody"]["rawBytes"], response_len)
        self.assertEqual(record["responseBody"]["models"][0]["streamCount"], 1)
        self.assertEqual(record["responseBody"]["models"][0]["codecs"], ["bytevc2"])
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_duplicate_query_order_and_report_local_identity_labels(self):
        first = flow(path=MODEL_PATH + "?iid=secret-iid&video_id=123&iid=secret-iid&device_id=secret-did")
        second = flow(path=MODEL_PATH + "?iid=secret-iid&video_id=123")
        result = report.summarize([first, second])
        query = result["records"][0]["query"]
        self.assertEqual([pair["name"] for pair in query], ["iid", "video_id", "iid", "device_id"])
        self.assertEqual(query[0]["identity"], query[2]["identity"])
        self.assertEqual(query[0]["identity"], result["records"][1]["query"][0]["identity"])
        self.assertEqual(result["videoGroups"]["123"], [1, 2])
        self.assertNotIn("secret-", json.dumps(result))

    def test_credentials_and_duplicate_response_cookies_are_lengths_only(self):
        result = report.summarize([flow(
            path=MODEL_PATH + "?token=PRIVATE-QUERY&url=https%3A%2F%2FPRIVATE-URL",
            req_headers={"Cookie": "iid=PRIVATE-IID; sessionid=PRIVATE-SESSION",
                         "x-argus": base64.b64encode(b"PRIVATE-SIGNATURE").decode()},
            resp_headers=[["Set-Cookie", "ttreq=PRIVATE-TICKET; Path=/"],
                          ["Set-Cookie", "odin_tt=PRIVATE-ODIN; Path=/"],
                          ["x-tt-token", "PRIVATE-TOKEN"]],
        )])
        record = result["records"][0]
        self.assertEqual([cookie["name"] for cookie in record["responseCookies"]], ["ttreq", "odin_tt"])
        self.assertEqual(record["requestHeaders"][1]["decodedBytes"], len(b"PRIVATE-SIGNATURE"))
        self.assertNotIn("PRIVATE-", json.dumps(result))
        self.assertNotIn(base64.b64encode(b"PRIVATE-SIGNATURE").decode(), json.dumps(result))

    def test_missing_lengths_and_truncation_never_become_empty_success(self):
        result = report.summarize([
            flow(),
            flow(req_body="b64:invalid", resp_body='{"data":',
                 req_body_truncated=True, resp_body_truncated=True,
                 req_raw_bytes=900000, resp_raw_bytes=800000),
            flow(req_body="", resp_body="", req_raw_bytes=0, resp_raw_bytes=0,
                 req_body_truncated=False, resp_body_truncated=False),
        ])
        missing, truncated, empty = result["records"]
        self.assertIsNone(missing["responseBody"]["rawBytes"])
        self.assertEqual(missing["responseBody"]["state"], "unknown")
        self.assertEqual(truncated["responseBody"]["state"], "truncated")
        self.assertEqual(truncated["requestBody"]["state"], "truncated")
        self.assertEqual(empty["responseBody"]["state"], "empty")

    def test_irrelevant_paths_are_omitted_and_invalid_lines_report_locations_only(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "capture.jsonl"
            source.write_text(json.dumps(flow(path="/passport/login", req_body="PRIVATE-BODY"))
                              + "\nPRIVATE-INVALID-JSON\n" + json.dumps(flow()) + "\n")
            result = report.read_captures([source])
        self.assertEqual(len(result["records"]), 1)
        self.assertEqual(result["records"][0]["source"], "capture.jsonl")
        self.assertEqual(result["records"][0]["line"], 3)
        self.assertEqual(result["errors"], [{"source": "capture.jsonl", "line": 2, "error": "invalidJson"}])
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_old_capture_preserves_unknown_lengths_and_cookie_names(self):
        result = report.summarize([flow(
            req_body=json.dumps({"video_id": "123"}),
            resp_body='{"code":0,"data":{}}',
            set_cookies=["ttreq=PRIVATE-TICKET; Path=/", "odin_tt=PRIVATE-ODIN"],
        )])
        record = result["records"][0]
        self.assertEqual(record["requestBody"]["videoIds"], ["123"])
        self.assertIsNone(record["requestBody"]["rawBytes"])
        self.assertIsNone(record["responseHeaders"])
        self.assertEqual([cookie["name"] for cookie in record["responseCookies"]], ["ttreq", "odin_tt"])
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_invalid_encoding_and_decode_limit_are_visible(self):
        large_body, _ = encoded_body({"unused": "x" * 100})
        with patch.object(report, "MAX_DECODED_BYTES", 32):
            result = report.summarize([
                flow(req_body="b64:broken", resp_body="b64:H4sI"),
                flow(req_body=large_body),
            ])
        first, oversized = result["records"]
        self.assertEqual(first["requestBody"]["state"], "unparsed")
        self.assertEqual(first["responseBody"]["state"], "unparsed")
        self.assertEqual(oversized["requestBody"]["state"], "unparsed")

    def test_known_business_fields_preserve_types_and_unknown_values_stay_private(self):
        body = {
            "video_id": "123", "dr_scene": "player", "content_type": 1004,
            "video_content_type": 1, "unknown": "PRIVATE-TOP",
            "biz_param": {
                "device_level": 3, "video_platform": 1024, "use_server_dns": True,
                "use_server_dns_scene": "cold_start_stage", "disable_video_relate_book": False,
                "need_all_video_definition": True, "from_video_id": "122",
                "unknown_secret": "PRIVATE-BIZ", "need_mp4_align": "PRIVATE-WRONG-TYPE",
            },
        }
        result = report.summarize([flow(req_body=json.dumps(body))])
        details = result["records"][0]["requestBody"]
        self.assertEqual(details["bodyKeys"], sorted(body))
        self.assertEqual(details["bizParamKeys"], sorted(body["biz_param"]))
        self.assertEqual(details["bizParam"], {
            "device_level": 3, "video_platform": 1024, "use_server_dns": True,
            "use_server_dns_scene": "cold_start_stage", "disable_video_relate_book": False,
            "need_all_video_definition": True, "from_video_id": "122",
        })
        self.assertEqual(details["drScene"], "player")
        self.assertEqual(details["contentType"], 1004)
        self.assertEqual(details["videoContentType"], 1)
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_safe_header_values_are_retained_but_never_credentials_or_invalid_values(self):
        headers = [
            ["Content-Encoding", "gzip"], ["Content-Type", "application/json; charset=UTF-8"],
            ["x-ss-dp", "8662"], ["lc", "101"], ["x-tt-store-region", "cn-gd"],
            ["x-tt-store-region-src", "did"], ["x-tt-token", "PRIVATE-TOKEN"],
            ["Content-Type", "application/json; boundary=PRIVATE-BOUNDARY"],
            ["Content-Encoding", "PRIVATE-ENCODING"], ["x-ss-dp", "PRIVATE-DP"],
            ["x-tt-store-region-src", "PRIVATE-SOURCE"],
        ]
        result = report.summarize([flow(req_headers=headers)])
        summary = result["records"][0]["requestHeaders"]
        self.assertEqual([item["value"] for item in summary[:6]], [pair[1] for pair in headers[:6]])
        self.assertTrue(all("value" not in item for item in summary[6:]))
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_business_type_mismatches_and_unknown_scene_are_not_exported(self):
        result = report.summarize([flow(req_body=json.dumps({
            "dr_scene": "PRIVATE-SCENE", "content_type": True, "video_content_type": "PRIVATE-TYPE",
            "biz_param": {"device_level": False, "video_platform": "PRIVATE-PLATFORM",
                          "use_server_dns_scene": "PRIVATE-DNS"},
        }))])
        details = result["records"][0]["requestBody"]
        self.assertEqual(details["bizParam"], {})
        self.assertIsNone(details["drScene"])
        self.assertIsNone(details["contentType"])
        self.assertIsNone(details["videoContentType"])
        self.assertNotIn("PRIVATE-", json.dumps(result))

    def test_explicit_decoded_brotli_body_preserves_raw_length_and_its_own_truncation(self):
        decoded = json.dumps({"code": 0, "data": {"video_model": json.dumps({"video_list": []})}})
        result = report.summarize([flow(
            resp_headers=[["Content-Encoding", "br"]], resp_body="b64:AQID",
            resp_raw_bytes=3, resp_body_truncated=True,
            resp_decoded_body=decoded, resp_decoded_bytes=len(decoded.encode()),
            resp_decoded_body_truncated=False,
        )])
        details = result["records"][0]["responseBody"]
        self.assertEqual(details["state"], "json")
        self.assertEqual(details["rawBytes"], 3)
        self.assertEqual(details["decodedBytes"], len(decoded.encode()))
        self.assertEqual(details["bodySource"], "decoded")
        self.assertFalse(details["isTruncated"])
        self.assertTrue(details["rawIsTruncated"])
        self.assertEqual(details["models"][0]["streamCount"], 0)

    def test_truncated_decoded_body_is_not_parsed_and_old_brotli_stays_unparsed(self):
        result = report.summarize([
            flow(resp_body="b64:AQID", resp_raw_bytes=3,
                 resp_decoded_body='{"code":0}', resp_decoded_bytes=900000,
                 resp_decoded_body_truncated=True),
            flow(resp_headers=[["Content-Encoding", "br"]], resp_body="b64:AQID", resp_raw_bytes=3),
        ])
        decoded, old = [record["responseBody"] for record in result["records"]]
        self.assertEqual(decoded["state"], "truncated")
        self.assertEqual(decoded["models"], [])
        self.assertEqual(old["state"], "unparsed")
        self.assertIsNone(old["decodedBytes"])


class AddonCaptureTests(unittest.TestCase):
    def test_addon_records_raw_lengths_repeated_headers_and_truncation(self):
        spec = importlib.util.spec_from_file_location("capture_addon", Path(__file__).resolve().parents[1] / "captures/addon.py")
        addon = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(addon)

        class Headers:
            def items(self, multi=False):
                return [("Set-Cookie", "a=one"), ("Set-Cookie", "b=two")]

        request = SimpleNamespace(pretty_host="reading.snssdk.com", method="POST", path=MODEL_PATH,
                                  raw_content=b"r" * 200001, headers=Headers())
        response = SimpleNamespace(status_code=200, raw_content=b"s" * 500001, headers=Headers())
        with tempfile.TemporaryDirectory() as directory, patch.object(addon, "OUT", str(Path(directory) / "flows.jsonl")):
            addon.response(SimpleNamespace(request=request, response=response))
            captured = json.loads(Path(addon.OUT).read_text())
        self.assertEqual(captured["req_raw_bytes"], 200001)
        self.assertEqual(captured["resp_raw_bytes"], 500001)
        self.assertTrue(captured["req_body_truncated"])
        self.assertTrue(captured["resp_body_truncated"])
        self.assertEqual(captured["resp_headers"], [["Set-Cookie", "a=one"], ["Set-Cookie", "b=two"]])
        self.assertEqual(captured["set_cookies"], ["a=one", "b=two"])

    def test_addon_preserves_brotli_raw_body_and_bounds_mitm_decoded_body(self):
        spec = importlib.util.spec_from_file_location("capture_addon", Path(__file__).resolve().parents[1] / "captures/addon.py")
        addon = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(addon)

        class Headers(dict):
            def items(self, multi=False):
                return list(super().items())

        request = SimpleNamespace(pretty_host="reading.snssdk.com", method="POST", path=MODEL_PATH,
                                  raw_content=b"{}", headers=Headers())
        decoded = b"d" * (addon.RESPONSE_BODY_LIMIT + 1)
        response = SimpleNamespace(status_code=200, raw_content=b"\x01\x02\x03", content=decoded,
                                   headers=Headers({"Content-Encoding": "br"}))
        with tempfile.TemporaryDirectory() as directory, patch.object(addon, "OUT", str(Path(directory) / "flows.jsonl")):
            addon.response(SimpleNamespace(request=request, response=response))
            captured = json.loads(Path(addon.OUT).read_text())
        self.assertEqual(captured["resp_body"], "b64:AQID")
        self.assertEqual(captured["resp_raw_bytes"], 3)
        self.assertFalse(captured["resp_body_truncated"])
        self.assertEqual(captured["resp_decoded_bytes"], len(decoded))
        self.assertTrue(captured["resp_decoded_body_truncated"])
        self.assertEqual(len(captured["resp_decoded_body"]), addon.RESPONSE_BODY_LIMIT)


if __name__ == "__main__":
    unittest.main()
