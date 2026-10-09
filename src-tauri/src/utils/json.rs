//! JSON 取值的公共助手（P3-C9 自 discover/detail/login/history 收敛）。
//!
//! 无业务语义：只描述「怎么从 `serde_json::Value` 里安全取一个字段」，
//! 不知道任何接口名/字段含义。三个取串函数语义刻意不同，按需选用：
//!
//! | 函数 | 入参 | 行为 | 来源 |
//! | --- | --- | --- | --- |
//! | [`str_field`] | 单键 | 缺失/非字符串给空串 | discover（原实现） |
//! | [`str_field_any`] | 候选键数组 | 第一个**存在且为字符串**的键（空串也算存在） | detail `pick` |
//! | [`str_field_paths`] | 点分路径数组 | 跳过空串，全部落空给 `None` | login `str_field` |
//!
//! ⚠️ [`str_field_any`] 与 [`str_field_paths`] 的「空串处理」相反，合并会
//! 改变线上行为（detail 的 cover 兜底依赖「空串也算命中」），故保留两个。

use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 业务错误码检查（信封 `{"code":0,"message":"…"}`），文案前缀「接口返回」。
pub(crate) fn check_code(value: &Value) -> AppResult<()> {
    check_code_in(value, "接口")
}

/// 同 [`check_code`]，错误文案前缀可定制（`{label}返回 {code}: {msg}`）。
///
/// detail 域三个端点各自的报错前缀（「详情接口」「相关作品接口」…）
/// 是排障时的定位线索，不能统一成一个词。
pub(crate) fn check_code_in(value: &Value, label: &str) -> AppResult<()> {
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

/// 单键字符串字段：缺失或非字符串给空串。
pub(crate) fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 按候选键取第一个**存在且为字符串**的字段（原 detail 域 `pick`）。
///
/// 注意：候选键的值是空字符串时**直接返回空串**，不向后兜——这是
/// detail 线上行为（如 `series_cover` 存在但为空时不用 `cover_url` 补），
/// 与 [`str_field_paths`] 的「跳过空串」语义相反，勿「修复」。
pub(crate) fn str_field_any(v: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// 按候选点分路径宽松取字符串字段，跳过空串（原 login 域 `str_field`）。
///
/// passport 响应字段名/层级在端点间有出入（`data.encrypt_uid` 与
/// `data.event_params.log_id` 这类嵌套路径），逐路径试探取首个非空值；
/// 全部落空给 `None`，由调用方决定默认值。
pub(crate) fn str_field_paths(v: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| {
        let mut cur = v;
        for seg in p.split('.') {
            cur = cur.get(seg)?;
        }
        cur.as_str().map(str::to_string).filter(|s| !s.is_empty())
    })
}

/// 数字字段容忍字符串形态（平台对大数偶发走字符串），缺失给 0。
pub(crate) fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

/// 浮点字段容忍字符串形态（score 线上是 `"8.0"`），缺失给 0.0。
pub(crate) fn num_field(v: &Value, key: &str) -> f64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `category_schema` 是 JSON 字符串：`[{"category_id":..,"name":"逆袭",...}]`，
/// 取 name 做题材标签（至多 4 个）。解析失败给空表（标签是展示增强，
/// 不值得报错）。
pub(crate) fn parse_tags(schema: Option<&Value>) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn str_field_reads_present_missing_and_non_string() {
        let v = json!({ "title": "剧", "n": 5 });
        assert_eq!(str_field(&v, "title"), "剧");
        assert!(str_field(&v, "absent").is_empty());
        assert!(str_field(&v, "n").is_empty(), "非字符串给空串不报错");
    }

    #[test]
    fn int_field_tolerates_string_numbers() {
        let v = json!({ "a": 42, "b": "12345678", "c": "abc", "d": 1.5 });
        assert_eq!(int_field(&v, "a"), 42);
        assert_eq!(int_field(&v, "b"), 12_345_678, "字符串形态的大数要能读");
        assert_eq!(int_field(&v, "c"), 0);
        assert_eq!(int_field(&v, "d"), 0, "非整型数字给 0");
        assert_eq!(int_field(&v, "absent"), 0);
    }

    #[test]
    fn num_field_tolerates_string_floats() {
        let v = json!({ "a": 8.7, "b": "8.0", "c": "abc" });
        assert_eq!(num_field(&v, "a"), 8.7);
        assert_eq!(num_field(&v, "b"), 8.0);
        assert_eq!(num_field(&v, "c"), 0.0);
        assert_eq!(num_field(&v, "absent"), 0.0);
    }

    #[test]
    fn check_code_passes_zero_and_missing() {
        check_code(&json!({ "code": 0 })).unwrap();
        check_code(&json!({ "data": {} })).unwrap();
    }

    #[test]
    fn check_code_rejects_nonzero_with_message() {
        let err = check_code(&json!({ "code": 100103, "message": "PARAM_INVALID" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("100103"));
        assert!(err.contains("PARAM_INVALID"));
    }

    #[test]
    fn check_code_in_keeps_label_prefix() {
        let err = check_code_in(&json!({ "code": 7, "message": "bad" }), "详情接口")
            .unwrap_err()
            .to_string();
        assert!(err.contains("详情接口返回 7: bad"), "{err}");
    }

    #[test]
    fn str_field_any_returns_first_present_string_even_empty() {
        // 空串也算命中（detail 线上行为锁）：不向后兜
        let v = json!({ "a": "", "b": "x" });
        assert!(str_field_any(&v, &["a", "b"]).is_empty());
        let v = json!({ "b": "x" });
        assert_eq!(str_field_any(&v, &["a", "b"]), "x");
        let v = json!({ "a": 5, "b": "x" });
        assert_eq!(str_field_any(&v, &["a", "b"]), "x", "非字符串继续向后");
        assert!(str_field_any(&json!({}), &["a", "b"]).is_empty());
    }

    #[test]
    fn str_field_paths_walks_dot_paths_and_skips_empty() {
        let v = json!({ "data": { "encrypt_uid": "abc" } });
        assert_eq!(
            str_field_paths(&v, &["data.encrypt_uid", "x"]),
            Some("abc".into())
        );
        // 空串跳过、向后兜
        let v = json!({ "a": "", "b": "x" });
        assert_eq!(str_field_paths(&v, &["a", "b"]), Some("x".into()));
        // 非字符串与全落空
        assert_eq!(str_field_paths(&json!({ "a": 5 }), &["a"]), None);
        assert_eq!(str_field_paths(&json!({}), &["a.b", "c"]), None);
    }

    #[test]
    fn parse_tags_reads_names_and_degrades() {
        let schema = json!(
            "[{\"name\":\"逆袭\"},{\"name\":\"穿越\"},{\"name\":\"a\"},{\"name\":\"b\"},{\"name\":\"c\"}]"
        );
        let tags = parse_tags(Some(&schema));
        assert_eq!(tags, vec!["逆袭", "穿越", "a", "b"], "至多取 4 个");

        assert!(parse_tags(Some(&json!("not json"))).is_empty());
        assert!(parse_tags(Some(&json!(5))).is_empty());
        assert!(parse_tags(None).is_empty());
    }
}
