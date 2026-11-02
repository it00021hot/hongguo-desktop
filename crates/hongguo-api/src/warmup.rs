//! 冷启动行为预热。
//!
//! 服务端按**设备行为状态**发放剧集授权：注册后只调播放端点的设备
//! （采集器特征）只发第 1 集；完成官方客户端冷启动行为序列的设备全量
//! 放行（2026-11-03 对照实验：向我们的全新设备按序重放 120 条官方冷
//! 启动请求后 ep1~6 全部发货，不重放则 ep2 起全拒——此前签名/注册形态/
//! 身份字段/间隔全部逐一排除）。
//!
//! 序列本体抓包固化于 [`SEQUENCE`]（官方 pm clear 后 register 之后的
//! 前 120 条业务请求，body 存解码后 JSON 文本）；运行期按序重发，身份
//! 字段（iid/device_id/cdid/openudid）重写为当前设备并重新签名。请求
//! 间加 60ms 间隔对齐官方节奏。
//!
//! 触发点：设备注册成功后、已采纳设备的每次启动（一次/进程，见
//! `hongguo_service::device`）。

use crate::client::{ApiEnv, gzip_bytes};
use serde_json::Value;

/// 官方冷启动行为序列（host + path + body 文本）。
const SEQUENCE: &str = include_str!("warmup_sequence.json");

/// 请求间间隔（毫秒）：官方冷启动约 40s 摊 120 条，我们收紧到 60ms
/// 顺序连发（对照实验即此节奏，已验证解锁）。
const REQUEST_INTERVAL_MS: u64 = 60;

/// 重写 query 中的身份字段；档案已携带的字段由签名器统一注入，去掉避免重复。
fn rewrite_query(path: &str, env: &ApiEnv) -> (String, Vec<(String, String)>) {
    let (base, q) = match path.split_once('?') {
        Some((b, q)) => (b.to_string(), q.to_string()),
        None => (path.to_string(), String::new()),
    };
    let mut biz: Vec<(String, String)> = Vec::new();
    for piece in q.split('&') {
        let Some((k, v)) = piece.split_once('=') else {
            continue;
        };
        let v = match k {
            "iid" => env.device.get("iid").to_string(),
            "device_id" => env.device.get("device_id").to_string(),
            "cdid" => env.device.get("cdid").to_string(),
            "openudid" => env.device.get("openudid").to_string(),
            // 设备证明票据我们的栈生成不了，去掉（其缺席由行为层补齐）
            "klink_egdi" => continue,
            // 签名器会自带 ts/_rticket
            "ts" | "_rticket" => continue,
            _ => v.to_string(),
        };
        if !env.device.get(k).is_empty() {
            // 档案已携带的字段由签名器统一注入，去重
            continue;
        }
        biz.push((k.to_string(), v));
    }
    (base, biz)
}

/// 重写 JSON body 里的身份字段。
fn rewrite_json_body(body: &str, env: &ApiEnv) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_str(body).ok()?;
    let iid = env.device.get("iid").to_string();
    let device_id = env.device.get("device_id").to_string();
    let cdid = env.device.get("cdid").to_string();
    let openudid = env.device.get("openudid").to_string();
    fn walk(v: &mut Value, iid: &str, device_id: &str, cdid: &str, openudid: &str) {
        match v {
            Value::Object(m) => {
                for (k, val) in m.iter_mut() {
                    match k.as_str() {
                        "iid" | "install_id" => {
                            if let Ok(n) = iid.parse::<i64>() {
                                *val = serde_json::json!(n);
                            }
                        }
                        "device_id" => {
                            if let Ok(n) = device_id.parse::<i64>() {
                                *val = serde_json::json!(n);
                            }
                        }
                        "cdid" => *val = serde_json::json!(cdid),
                        "openudid" => *val = serde_json::json!(openudid),
                        _ => walk(val, iid, device_id, cdid, openudid),
                    }
                }
            }
            Value::Array(a) => {
                for item in a.iter_mut() {
                    walk(item, iid, device_id, cdid, openudid);
                }
            }
            _ => {}
        }
    }
    walk(&mut v, &iid, &device_id, &cdid, &openudid);
    gzip_bytes(serde_json::to_vec(&v).ok()?.as_slice()).ok()
}

/// 执行冷启动行为预热：按官方序列顺序重发（身份重写 + 逐条重签名）。
/// 返回成功（2xx）条数。单条失败/超时不中断整体（预热是尽力而为的
/// 行为铺垫，个别 404/限制不影响解锁效果——对照实验里也含失败条目）。
pub async fn run(env: &ApiEnv) -> usize {
    let Ok(seq) = serde_json::from_str::<Vec<Value>>(SEQUENCE) else {
        return 0;
    };
    let client = match crate::client::build_client(&env.proxy) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let mut ok = 0usize;
    for item in &seq {
        let (Some(host), Some(path)) = (
            item.get("host").and_then(Value::as_str),
            item.get("path").and_then(Value::as_str),
        ) else {
            continue;
        };
        let (pathname, biz_query) = rewrite_query(path, env);
        let body = item
            .get("body")
            .and_then(Value::as_str)
            .and_then(|b| rewrite_json_body(b, env));
        let mut extra = vec![
            ("x-ss-dp".to_string(), "8662".into()),
            ("lc".to_string(), "101".into()),
            ("x-tt-store-region".to_string(), "cn-gd".into()),
            ("x-tt-store-region-src".to_string(), "did".into()),
        ];
        if body.is_some() {
            extra.push(("Content-Encoding".into(), "gzip".into()));
        }
        if let Some(c) = &env.cookie
            && !c.is_empty()
        {
            extra.push(("Cookie".into(), c.clone()));
        }
        let origin = format!("https://{host}");
        let signed = hongguo_signer::ticket::sign_request_with(
            &origin,
            &pathname,
            body,
            &env.device,
            &biz_query,
            &extra,
        );
        let mut req = client.post(&signed.url);
        for (k, v) in &signed.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Some(b) = &signed.body {
            req = req.body(b.clone());
        }
        if let Ok(resp) = req.send().await
            && resp.status().is_success()
        {
            ok += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(REQUEST_INTERVAL_MS)).await;
    }
    ok
}
