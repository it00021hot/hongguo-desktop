//! 设置与代理配置。
//!
//! `Settings` 两种形态都要：落盘在 `data.json`（沿用 Electron 版 snake_case 键名平滑迁移），又是设置页 IPC 载荷（camelCase）——字段一律 camelCase 收发，`alias` 兼容旧落盘键。

use serde::{Deserialize, Serialize};

/// 代理模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 跟随系统环境变量
    System,
    /// 手动指定
    Manual,
    /// 强制直连（忽略环境变量）。新装默认：本 app 直连官方 API 本就可达，
    /// 跟随系统反而容易吃进代理软件的环境变量把请求带进不可控通路。
    #[default]
    Direct,
}

/// 代理配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 兼容旧版落盘的 `{"mode":"system"}` 对象写法
    #[serde(default, deserialize_with = "deserialize_mode")]
    pub mode: ProxyMode,
    /// 形如 `http://user:pass@host:port`
    #[serde(default)]
    pub url: String,
}

/// 代理模式的兼容读取：旧版落盘成 `{"mode":"system"}`（serde tag 形态），现在契约是裸字符串——两种都要能读，否则升级即丢代理设置。
fn deserialize_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ProxyMode, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape {
        Plain(String),
        Legacy { mode: String },
    }

    let name = match Shape::deserialize(d)? {
        Shape::Plain(s) | Shape::Legacy { mode: s } => s,
    };
    match name.as_str() {
        "system" => Ok(ProxyMode::System),
        "manual" => Ok(ProxyMode::Manual),
        "direct" => Ok(ProxyMode::Direct),
        other => Err(serde::de::Error::custom(format!("未知代理模式: {other}"))),
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            mode: ProxyMode::Direct,
            url: String::new(),
        }
    }
}

impl ProxyConfig {
    /// 当前配置下的代理地址；`None` 表示直连。
    pub fn resolved(&self) -> Option<String> {
        match self.mode {
            ProxyMode::Direct => None,
            ProxyMode::Manual => {
                if self.url.trim().is_empty() {
                    None
                } else {
                    Some(self.url.trim().to_string())
                }
            }
            ProxyMode::System => {
                for key in [
                    "HTTPS_PROXY",
                    "https_proxy",
                    "HTTP_PROXY",
                    "http_proxy",
                    "ALL_PROXY",
                    "all_proxy",
                ] {
                    if let Ok(v) = std::env::var(key)
                        && !v.trim().is_empty()
                    {
                        return Some(v);
                    }
                }
                None
            }
        }
    }
}

/// 文件命名模板。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NamingTemplate {
    /// `{series_title} {vid_index}`
    #[default]
    TitleIndex,
    /// `{series_title} {vid_index} {ep_title}`
    TitleIndexEpisode,
    /// 仅剧名
    OnlyTitle,
}

/// 目录名长度上限。Windows 的整条路径有 260 字符限制，剧名给到 80
/// 还能容下下载根目录、`<剧名> 合集.mp4` 与集号。
const FOLDER_NAME_MAX: usize = 80;

/// 清洗结果为空时的占位名。
const UNNAMED: &str = "未命名";

/// 清洗目录名：去掉路径分隔符与 Windows 保留字符，避免路径穿越。
pub fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        UNNAMED.to_string()
    } else {
        trimmed.chars().take(FOLDER_NAME_MAX).collect()
    }
}

/// 清洗文件名：规则与目录名完全一致，上限同 `FOLDER_NAME_MAX`（文件名是单段路径，不必为子目录预留长度）。
pub fn sanitize_file_name(name: &str) -> String {
    sanitize_folder_name(name)
}

/// 已登录账号的会话快照（短信登录成功后落库，业务请求带它取登录态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    /// 登录手机号（明文，仅本机存储）
    pub mobile: String,
    /// 会话 Cookie（`k=v; k=v`，附加到签名请求，不参与签名）
    pub cookies: String,
    /// 用户昵称（服务端返回，无则空）
    #[serde(default)]
    pub user_name: String,
    /// 用户头像 URL（登录响应 data.avatar_url 下发，无则空）
    #[serde(default)]
    pub avatar_url: String,
    /// 用户 id（字符串形态）
    #[serde(default)]
    pub user_id: String,
    /// 登录时间（UTC 毫秒时间戳）
    #[serde(default)]
    pub login_at: i64,
    /// x-tt-token 长凭据（登录响应头下发，`00<access>--<refresh>-3.0.3`
    /// 形态；请求带它的前 56 位短形式。旧账号无此字段，下次登录补上）
    #[serde(default)]
    pub token: String,
    /// sms_login 响应原文（JSON 整体落库；长尾字段按需读取。旧账号为空）
    #[serde(default)]
    pub raw_login: String,
    /// user_info 响应原文（JSON 整体落库；含 biz_user_id 红果号、
    /// req_id 数字 uid、vip_info 等 60+ 字段，账号资料刷新时更新）
    #[serde(default)]
    pub raw_profile: String,
}

/// 应用设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 下载根目录
    pub download_dir: String,
    /// 在线媒体、兼容转码和封面缓存的根目录；空 = 跟随数据目录。
    /// 改动即时生效于新写入，旧目录残留不迁移（与 download_dir 同语义）。
    #[serde(default)]
    pub cache_dir: String,
    /// 封面与头像缓存占用上限（MB；0 = 不限）。
    #[serde(default)]
    pub cache_limit_mb: u32,
    /// 封面与头像缓存清理周期（天；0 = 关闭）。超过这个天数的缓存文件在自动清理
    /// 时被删除（按最后修改时间判定）。
    #[serde(default)]
    pub cache_ttl_days: u32,
    /// 命名模板
    #[serde(default)]
    pub naming: NamingTemplate,
    /// 最大并发数（1–10）
    pub max_concurrency: usize,
    /// 代理
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// 看完自动删
    #[serde(default)]
    pub auto_delete_after_play: bool,
    /// 播完自动下一集
    #[serde(default = "default_true")]
    pub auto_next_episode: bool,
    /// 最小化自动暂停，恢复窗口后继续播。
    #[serde(default = "default_true")]
    pub pause_on_minimize: bool,
    /// 老板键全局快捷键（tauri accelerator 形态，如 "CommandOrControl+Alt+H"；
    /// 空 = 未启用）。注册在壳侧（issue #4：隐藏/显示主窗口）
    #[serde(default)]
    pub boss_key: String,
    /// 主题：auto / light / dark（默认 dark——黑色是应用的默认观感）
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 登录账号（未登录为 None）
    #[serde(default)]
    pub account: Option<AccountState>,
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "dark".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        let download_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        Self {
            download_dir,
            cache_dir: String::new(),
            cache_limit_mb: 0,
            cache_ttl_days: 0,
            naming: NamingTemplate::default(),
            max_concurrency: 3,
            proxy: ProxyConfig::default(),
            auto_delete_after_play: false,
            auto_next_episode: true,
            pause_on_minimize: true,
            boss_key: String::new(),
            theme: default_theme(),
            account: None,
        }
    }
}

impl Settings {
    /// 收敛并发到 1–10，保存前调用。
    pub fn clamp_concurrency(&mut self) {
        self.max_concurrency = self.max_concurrency.clamp(1, 10);
    }

    /// 缓存根覆盖值：非空给 `Some`，空表示跟随数据目录（`None`）。
    /// 启动装载与保存后都要经它应用到 `hongguo_store::paths`，三处调用方共用。
    pub fn cache_override(&self) -> Option<String> {
        let trimmed = self.cache_dir.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    /// 缓存上限（字节）：0 = 不限。给 `hongguo_service::storage_service::cache::auto_clean` 用。
    pub fn cache_limit_bytes(&self) -> u64 {
        u64::from(self.cache_limit_mb) * 1024 * 1024
    }

    /// 某部剧的下载子目录：`<下载根>/红果短剧/<剧名>`。
    pub fn series_dir(&self, series_title: &str) -> std::path::PathBuf {
        std::path::Path::new(&self.download_dir)
            .join("红果短剧")
            .join(sanitize_folder_name(series_title))
    }

    /// 按命名模板渲染文件名（不含扩展名）。
    pub fn render_file_name(&self, series_title: &str, vid_index: u32, ep_title: &str) -> String {
        let index = format!("{vid_index:03}");
        let base = match self.naming {
            NamingTemplate::TitleIndex => format!("{series_title} {index}"),
            NamingTemplate::TitleIndexEpisode => {
                if ep_title.trim().is_empty() {
                    format!("{series_title} {index}")
                } else {
                    format!("{series_title} {index} {ep_title}")
                }
            }
            NamingTemplate::OnlyTitle => series_title.to_string(),
        };
        sanitize_file_name(&base)
    }
}

/// 代理连通性测试结果。只走 IPC，不落盘。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub ok: bool,
    /// 耗时（毫秒）
    pub elapsed_ms: u128,
    #[serde(default)]
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_concurrency_is_3() {
        assert_eq!(Settings::default().max_concurrency, 3);
    }

    #[test]
    fn clamp_concurrency_bounds() {
        let mut s = Settings {
            max_concurrency: 0,
            ..Default::default()
        };
        s.clamp_concurrency();
        assert_eq!(s.max_concurrency, 1);
        s.max_concurrency = 99;
        s.clamp_concurrency();
        assert_eq!(s.max_concurrency, 10);
    }

    #[test]
    fn legacy_prefetch_count_is_discarded_on_save() {
        let original = Settings::default();
        let mut saved = serde_json::to_value(&original).unwrap();
        for count in [0, 1, 10] {
            saved["prefetchEpisodes"] = serde_json::json!(count);
            let loaded: Settings = serde_json::from_value(saved.clone()).unwrap();
            assert_eq!(loaded, original);
            assert!(
                serde_json::to_value(loaded)
                    .unwrap()
                    .get("prefetchEpisodes")
                    .is_none()
            );
        }
    }

    #[test]
    fn proxy_direct_ignores_url() {
        let p = ProxyConfig {
            mode: ProxyMode::Direct,
            url: "http://127.0.0.1:7890".into(),
        };
        assert!(p.resolved().is_none());
    }

    #[test]
    fn proxy_manual_empty_is_direct() {
        let p = ProxyConfig {
            mode: ProxyMode::Manual,
            url: "   ".into(),
        };
        assert!(p.resolved().is_none());
    }

    #[test]
    fn proxy_manual_returns_url() {
        let p = ProxyConfig {
            mode: ProxyMode::Manual,
            url: "http://127.0.0.1:1080".into(),
        };
        assert_eq!(p.resolved().unwrap(), "http://127.0.0.1:1080");
    }

    #[test]
    fn naming_template_renders() {
        let mut s = Settings {
            naming: NamingTemplate::TitleIndex,
            ..Default::default()
        };
        assert_eq!(s.render_file_name("剧名", 7, "标题"), "剧名 007");

        s.naming = NamingTemplate::TitleIndexEpisode;
        assert_eq!(s.render_file_name("剧名", 7, "标题"), "剧名 007 标题");

        s.naming = NamingTemplate::OnlyTitle;
        assert_eq!(s.render_file_name("剧名", 7, "标题"), "剧名");
    }

    #[test]
    fn naming_template_skips_empty_episode() {
        let s = Settings {
            naming: NamingTemplate::TitleIndexEpisode,
            ..Default::default()
        };
        assert_eq!(s.render_file_name("剧名", 3, "  "), "剧名 003");
    }

    #[test]
    fn series_dir_uses_hongguo_subfolder() {
        let s = Settings {
            download_dir: "/tmp/dl".into(),
            ..Default::default()
        };
        let dir = s.series_dir("我的剧");
        assert!(dir.to_string_lossy().contains("红果短剧"));
        assert!(dir.to_string_lossy().contains("我的剧"));
    }

    #[test]
    fn settings_roundtrip_through_json() {
        let s = Settings {
            max_concurrency: 7,
            naming: NamingTemplate::OnlyTitle,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn reads_legacy_plain_and_tagged_proxy_modes() {
        let tagged: ProxyConfig =
            serde_json::from_str(r#"{"mode":{"mode":"direct"},"url":""}"#).unwrap();
        assert_eq!(tagged.mode, ProxyMode::Direct);
        let plain: ProxyConfig = serde_json::from_str(r#"{"mode":"manual","url":"x"}"#).unwrap();
        assert_eq!(plain.mode, ProxyMode::Manual);
    }

    #[test]
    fn unknown_proxy_mode_is_rejected() {
        let err = serde_json::from_str::<ProxyConfig>(r#"{"mode":"nonsense"}"#).unwrap_err();
        assert!(err.to_string().contains("nonsense"), "实际: {err}");
    }

    #[test]
    fn goes_out_as_camel_case_for_ipc() {
        let s = Settings {
            download_dir: "D:\\dl".into(),
            cache_dir: "D:\\cache".into(),
            cache_limit_mb: 512,
            cache_ttl_days: 7,
            naming: NamingTemplate::TitleIndex,
            max_concurrency: 4,
            proxy: ProxyConfig {
                mode: ProxyMode::Direct,
                url: "".into(),
            },
            auto_delete_after_play: false,
            auto_next_episode: true,
            pause_on_minimize: true,
            boss_key: "CommandOrControl+Alt+H".into(),
            theme: "auto".into(),
            account: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["downloadDir"], "D:\\dl");
        assert_eq!(v["cacheDir"], "D:\\cache");
        assert_eq!(v["cacheLimitMb"], 512);
        assert_eq!(v["cacheTtlDays"], 7);
        assert_eq!(v["maxConcurrency"], 4);
        assert_eq!(v["autoDeleteAfterPlay"], false);
        assert_eq!(v["autoNextEpisode"], true);
        assert_eq!(v["pauseOnMinimize"], true);
        assert_eq!(v["bossKey"], "CommandOrControl+Alt+H");
        assert_eq!(v["naming"], "titleIndex");
        assert_eq!(v["proxy"]["mode"], "direct");
        assert!(v.get("download_dir").is_none());
        assert!(v.get("compatMode").is_none());
    }

    #[test]
    fn proxy_test_result_goes_out_as_camel_case() {
        let r = ProxyTestResult {
            ok: true,
            elapsed_ms: 42,
            message: "ok".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["elapsedMs"], 42);
        assert!(v.get("elapsed_ms").is_none());
    }

    #[test]
    fn cache_override_trims_and_allows_empty() {
        let mut s = Settings::default();
        assert_eq!(s.cache_override(), None, "默认空 = 跟随数据目录");
        s.cache_dir = "  ".into();
        assert_eq!(s.cache_override(), None);
        s.cache_dir = " D:\\cache ".into();
        assert_eq!(s.cache_override().as_deref(), Some("D:\\cache"));
    }

    #[test]
    fn cache_limit_bytes_converts_mb() {
        let mut s = Settings::default();
        assert_eq!(s.cache_limit_bytes(), 0, "0 = 不限");
        s.cache_limit_mb = 512;
        assert_eq!(s.cache_limit_bytes(), 512 * 1024 * 1024);
    }

    #[test]
    fn cache_policy_defaults_to_off() {
        let s = Settings::default();
        assert_eq!(s.cache_limit_mb, 0);
        assert_eq!(s.cache_ttl_days, 0);
    }

    #[test]
    fn old_store_without_cache_policy_reads_as_off() {
        let s: Settings =
            serde_json::from_str(r#"{"downloadDir":"/tmp","maxConcurrency":3,"theme":"dark"}"#)
                .unwrap();
        assert_eq!(s.cache_limit_mb, 0);
        assert_eq!(s.cache_ttl_days, 0);
    }

    #[test]
    fn folder_name_strips_path_separators() {
        assert_eq!(sanitize_folder_name("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_folder_name("我的:剧"), "我的_剧");
    }

    #[test]
    fn folder_name_handles_empty_and_reserved() {
        assert_eq!(sanitize_folder_name(""), "未命名");
        assert_eq!(sanitize_folder_name("   "), "未命名");
        assert_eq!(sanitize_folder_name("..."), "未命名");
    }

    #[test]
    fn folder_name_strips_trailing_dot_and_space() {
        assert_eq!(sanitize_folder_name("剧名. "), "剧名");
    }

    #[test]
    fn folder_name_strips_control_chars() {
        assert_eq!(sanitize_folder_name("剧\u{1}名"), "剧_名");
    }

    #[test]
    fn names_are_length_bounded() {
        let long = "剧".repeat(200);
        assert_eq!(sanitize_folder_name(&long).chars().count(), FOLDER_NAME_MAX);
        assert_eq!(sanitize_file_name(&long), sanitize_folder_name(&long));
    }
}
