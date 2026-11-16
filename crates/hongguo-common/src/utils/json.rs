//! JSON 取值的纯助手（自 discover/detail/login/history 收敛），不知道任何接口名/字段含义。
//!
//! 三个取串函数语义刻意不同，按需选用：[`str_field`] 单键，缺失/非字符串给空串；[`str_field_any`] 候选键取第一个存在且为字符串的（空串也算命中）；[`str_field_paths`] 点分路径，跳过空串、全部落空给 `None`。两者的「空串处理」相反是线上行为锁（detail 的 cover 兜底依赖空串命中），勿合并。

use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 业务错误码检查（信封 `{"code":0,"message":"…"}`），文案前缀「接口返回」。
pub fn check_code(value: &Value) -> AppResult<()> {
    check_code_in(value, "接口")
}

/// 同 [`check_code`]，错误文案前缀可定制（`{label}返回 {code}: {msg}`）——detail 域各端点各自的报错前缀（「详情接口」…）是排障定位线索，不统一成一个词。
pub fn check_code_in(value: &Value, label: &str) -> AppResult<()> {
    if let Some(code) = value.get("code").and_then(Value::as_i64)
        && code != 0
    {
        let msg = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(AppError::Media(format!("{label}返回 {code}: {msg}")));
    }
    Ok(())
}

/// 业务拒绝：`code != 0` 归 [`AppError::Rejected`]，透传服务端原文（表情收藏「已添加过」等对用户有意义的文案）。
/// 与 [`check_code`]（→ Media）分流：互动/表情等用户动作域用本函数，媒体取流解析用 Media。
pub fn check_code_rejected(value: &Value, action: &str) -> AppResult<()> {
    if value.get("code").and_then(Value::as_i64) == Some(0) {
        return Ok(());
    }
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    let msg = value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/BaseResp/StatusMessage")
                .and_then(Value::as_str)
        })
        .unwrap_or("未知错误")
        .trim();
    Err(AppError::Rejected(format!(
        "{action}被服务端拒绝（{code}）: {msg}"
    )))
}

/// 单键字符串字段：缺失或非字符串给空串。
pub fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 按候选键取第一个**存在且为字符串**的字段。候选键的值是空串时直接返回空串、不向后兜——detail 线上行为（`series_cover` 为空不用 `cover_url` 补），与 [`str_field_paths`] 语义相反，勿「修复」。
pub fn str_field_any(v: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// 按候选点分路径宽松取字符串字段，跳过空串，全部落空给 `None` 由调用方决定默认值（passport 响应字段名/层级在端点间有出入，需逐路径试探）。
pub fn str_field_paths(v: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| {
        let mut cur = v;
        for seg in p.split('.') {
            cur = cur.get(seg)?;
        }
        cur.as_str().map(str::to_string).filter(|s| !s.is_empty())
    })
}

/// 数字字段容忍字符串形态（平台对大数偶发走字符串），缺失给 0。量级超限不归零：超 i64 饱和到 i64::MAX、浮点截断——字段在位说明服务端发了数，静默归零会出「播 9500 万变播 0」这类展示事故；字符串解析失败才是脏数据，维持 0。
pub fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_u64().map(|u| u.min(i64::MAX as u64) as i64))
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

/// 浮点字段容忍字符串形态（score 线上是 `"8.0"`），缺失给 0.0。
pub fn num_field(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `category_schema` 是 JSON 字符串：`[{"category_id":..,"name":"逆袭",...}]`，取 name 做题材标签（至多 4 个）；解析失败给空表（标签是展示增强，不值得报错）。
pub fn parse_tags(schema: Option<&Value>) -> Vec<String> {
    let Some(s) = schema.and_then(Value::as_str) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(s) else {
        return Vec::new();
    };
    parsed
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.get("name").and_then(Value::as_str))
                .take(4)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
