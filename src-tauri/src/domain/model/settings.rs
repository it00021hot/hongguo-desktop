//! 设置与代理配置。
//!
//! ⚠️ `Settings` 两种形态都要：落盘在 `data.json`（沿用 Electron 版的 snake_case
//! 键名以便平滑迁移），同时又是设置页的 IPC 载荷（前端 `settingsSchema` 声明
//! camelCase）。所以字段一律 camelCase 收发，用 `alias` 兼容旧落盘键。

use serde::{Deserialize, Serialize};

/// 代理模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 跟随系统环境变量
    #[default]
    System,
    /// 手动指定
    Manual,
    /// 强制直连（忽略环境变量）
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

/// 代理模式的兼容读取。
///
/// 旧版 `ProxyMode` 用了 `#[serde(tag = "mode")]`，落盘成 `{"mode":"system"}`；
/// 现在契约要求裸字符串 `"system"`。两种都要能读，否则升级即丢代理设置。
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
            mode: ProxyMode::System,
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
                // 依次读常见环境变量，与系统「跟随系统」语义一致
                for key in [
                    "HTTPS_PROXY",
                    "https_proxy",
                    "HTTP_PROXY",
                    "http_proxy",
                    "ALL_PROXY",
                    "all_proxy",
                ] {
                    if let Ok(v) = std::env::var(key) {
                        if !v.trim().is_empty() {
                            return Some(v);
                        }
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
    #[serde(alias = "TitleIndex")]
    TitleIndex,
    /// `{series_title} {vid_index} {ep_title}`
    #[serde(alias = "TitleIndexEpisode")]
    TitleIndexEpisode,
    /// 仅剧名
    #[serde(alias = "OnlyTitle")]
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

/// 清洗文件名。
///
/// 规则与目录名完全一致，长度也就跟着 [`FOLDER_NAME_MAX`]：文件名是单段路径，
/// 不必再为「红果短剧」子目录预留长度，所以没必要给一个更宽、上限更高的数字——
/// 那只会让人以为文件名的上限与目录名不同。
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
    /// 登录时间（unix 秒）
    #[serde(default)]
    pub login_at: i64,
    /// x-tt-token 长凭据（登录响应头下发，`00<access>--<refresh>-3.0.3`
    /// 形态；请求带它的前 56 位短形式。旧账号无此字段，下次登录补上）
    #[serde(default)]
    pub token: String,
}

/// 应用设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 下载根目录
    #[serde(alias = "download_dir")]
    pub download_dir: String,
    /// 命名模板
    #[serde(default)]
    pub naming: NamingTemplate,
    /// 最大并发数（1–10）
    #[serde(alias = "max_concurrency")]
    pub max_concurrency: usize,
    /// 代理
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// 看完自动删
    #[serde(default, alias = "auto_delete_after_play")]
    pub auto_delete_after_play: bool,
    /// 播完自动下一集
    #[serde(default = "default_true", alias = "auto_next_episode")]
    pub auto_next_episode: bool,
    /// 主题：auto / light / dark
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
    "auto".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        let download_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        Self {
            download_dir,
            naming: NamingTemplate::default(),
            max_concurrency: 3,
            proxy: ProxyConfig::default(),
            auto_delete_after_play: false,
            auto_next_episode: true,
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
    fn reads_legacy_snake_case_data_file() {
        // Electron 版 data.json 的真实形状：一个字段都不能丢
        let json = r#"{"download_dir":"C:\\Users\\me\\Downloads","naming":"TitleIndex","max_concurrency":5,"proxy":{"mode":{"mode":"manual"},"url":"http://127.0.0.1:1080"},"compat_mode":false,"auto_delete_after_play":true,"auto_next_episode":false,"theme":"dark"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.download_dir, "C:\\Users\\me\\Downloads");
        assert_eq!(s.naming, NamingTemplate::TitleIndex);
        assert_eq!(s.max_concurrency, 5);
        assert_eq!(s.proxy.mode, ProxyMode::Manual);
        assert_eq!(s.proxy.url, "http://127.0.0.1:1080");
        assert!(s.auto_delete_after_play);
        assert!(!s.auto_next_episode);
        assert_eq!(s.theme, "dark");
        // 旧文件里的 compat_mode 属于已下线的播放兜底，忽略即可，
        // 不能因为字段没了就把整个 data.json 判成损坏
        let back = serde_json::to_value(&s).unwrap();
        assert!(back.get("compatMode").is_none());
    }

    #[test]
    fn reads_legacy_plain_and_tagged_proxy_modes() {
        // 旧版带 tag 的对象写法与新版裸字符串写法都要能读
        let tagged: ProxyConfig =
            serde_json::from_str(r#"{"mode":{"mode":"direct"},"url":""}"#).unwrap();
        assert_eq!(tagged.mode, ProxyMode::Direct);
        let plain: ProxyConfig = serde_json::from_str(r#"{"mode":"manual","url":"x"}"#).unwrap();
        assert_eq!(plain.mode, ProxyMode::Manual);
    }

    #[test]
    fn unknown_proxy_mode_is_rejected() {
        // 不认识的值不能悄悄回落成 system，那等于把用户的代理设置改掉
        let err = serde_json::from_str::<ProxyConfig>(r#"{"mode":"nonsense"}"#).unwrap_err();
        assert!(err.to_string().contains("nonsense"), "实际: {err}");
    }

    #[test]
    fn goes_out_as_camel_case_for_ipc() {
        // 前端 settingsSchema / proxyConfigSchema / namingTemplateSchema
        let s = Settings {
            download_dir: "D:\\dl".into(),
            naming: NamingTemplate::TitleIndex,
            max_concurrency: 4,
            proxy: ProxyConfig {
                mode: ProxyMode::Direct,
                url: "".into(),
            },
            auto_delete_after_play: false,
            auto_next_episode: true,
            theme: "auto".into(),
            account: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["downloadDir"], "D:\\dl");
        assert_eq!(v["maxConcurrency"], 4);
        assert_eq!(v["autoDeleteAfterPlay"], false);
        assert_eq!(v["autoNextEpisode"], true);
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
        // Windows 不允许目录名以点或空格结尾
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
        // 文件名复用同一套规则，上限也就同一个
        assert_eq!(sanitize_file_name(&long), sanitize_folder_name(&long));
    }
}
