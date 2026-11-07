//! 排行榜接口（端点口径见 docs/hongguo-api-endpoints.md）。
//!
//! bookmall 系对设备敏感（静态旧设备报 ILLEGAL_ACCESS 110），探测用例
//! 需要注册态身份（见本模块 `probe::hg_env`）。

mod model;

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use super::client::{ApiEnv, api_call_reading};
#[cfg(test)]
use super::danmaku::LQ_API_ORIGIN;

pub use model::{RankItem, RankPage, RankPanelItem, RankPanelRow, RankSubList, RankTab};

/// 排行榜（书城 cell 换一换）。
pub const RANK_CELL_PATH: &str = "/reading/bookapi/bookmall/cell/change/v";

/// 排行榜页的 8 个「全部」tab 子榜（probe 遍历用）。
#[cfg(test)]
pub(crate) const ALL_TAB_SUBS: &[&str] = &[
    "ranklist_hot_sc",
    "ranklist_hot_play_sc",
    "ranklist_prestige",
    "ranklist_subscribe",
    "ranklist_new_rank_sc",
    "ranklist_hot_search_sc",
    "ranklist_must_watch",
    "ranklist_followed",
];

/// 一次榜单请求的定位参数（内容 tab × 子榜 × 筛选 × 月榜 + 翻页游标），
/// 字段与官方 query 一一对应。
#[derive(Debug, Clone, Default)]
pub struct RankReq<'a> {
    pub selected: &'a str,
    pub sub: &'a str,
    pub panel: Option<&'a str>,
    pub background: Option<&'a str>,
    pub offset: i64,
    pub session_id: &'a str,
    pub rank_version: &'a str,
}

mod parse;
mod session;

/// 新剧榜等兄弟域复用同一套 cell_view 解析。
pub(crate) use parse::{parse_cell_selector, parse_rank_items};
pub use session::fetch_rank_ex;

// 测试 / probe 经 `use super::*` 取这些符号（与拆分前同名同可见性）。
#[cfg(test)]
use parse::{collect_video_data, rank_version_field};
#[cfg(test)]
use session::{RankSessionBook, book_window, merge_into_book, selector_change_type};

#[cfg(test)]
#[path = "rank_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    pub(crate) fn anon_env() -> ApiEnv {
        ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default())
    }

    /// 已注册态的探测身份：从 `PROBE_DEVICE` 环境变量读档案 JSON
    /// （sqlite3 -readonly 从壳数据库 device_profile 表 id=1 行导出）。
    pub fn hg_env(base: &ApiEnv) -> ApiEnv {
        let json = std::env::var("PROBE_DEVICE").expect(
            "探测需要注册态身份：设 PROBE_DEVICE=<DeviceProfile JSON>，\
             用 sqlite3 -readonly 从壳数据库 device_profile 表 id=1 行导出",
        );
        let device: hongguo_signer::device::DeviceProfile =
            serde_json::from_str(&json).expect("PROBE_DEVICE 不是合法的 DeviceProfile JSON");
        ApiEnv {
            proxy: base.proxy.clone(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            x_tt_token: None,
            device,
        }
    }

    /// 榜单 cell 的 limit/offset 分页行为。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_paging() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut device: hongguo_signer::device::DeviceProfile =
            serde_json::from_str(&device_json).expect("device json");
        hongguo_signer::device::align_app_version(&mut device);
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            device,
            cookie: None,
            x_tt_token: None,
        };
        for (limit, offset) in [(0i64, 0i64), (50, 0), (20, 20), (20, 40)] {
            let q: Vec<(String, String)> = [
                ("cell_id", "7470092475068071998"),
                ("tab_type", "26"),
                ("selected_items", "all"),
                ("category_id", "0"),
                ("cell_sub_id", "0"),
                ("client_req_type", "2"),
                ("client_template", "2"),
                ("gender", "2"),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("sub_selected_items", "ranklist_subscribe"),
                ("unlimited_selector_change_type", "2"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
            match api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env).await {
                Ok(bytes) => {
                    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    let data = v.get("data");
                    let count = data
                        .map(|d| d.to_string().matches("series_id").count())
                        .unwrap_or(0);
                    println!(
                        "[limit={limit} offset={offset}] keys={:?} next_offset={:?} has_more={:?} series_id_hits={count}",
                        data.map(|d| d
                            .as_object()
                            .map(|o| o.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default())
                            .unwrap_or_default(),
                        data.and_then(|d| d.get("next_offset")).cloned(),
                        data.and_then(|d| d.get("has_more")).cloned(),
                    );
                }
                Err(e) => println!("[limit={limit} offset={offset}] ERR {e}"),
            }
        }
    }

    /// 筛选面板 schema 直连，dump 完整响应到 captures/。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_schema() {
        let env = hg_env(&anon_env());
        let q: Vec<(String, String)> = [
            ("cell_id", "7470092475068071998"),
            ("tab_type", "26"),
            ("selected_items", "all"),
            ("category_id", "0"),
            ("cell_sub_id", "0"),
            ("client_req_type", "2"),
            ("client_template", "2"),
            ("gender", "2"),
            ("limit", "0"),
            ("offset", "0"),
            ("sub_selected_items", "ranklist_hot_sc"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("排行榜请求");
        let out = std::env::current_dir()
            .ok()
            .map(|d| d.join("../captures/rank-selector-schema.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("captures/rank-selector-schema.json"));
        std::fs::write(&out, &bytes).expect("写 captures/rank-selector-schema.json");
        println!("[rank-selector] {} bytes -> {}", bytes.len(), out.display());
    }

    /// cell_selector 形态二分：服务端按客户端版本号分发两级 / 一级结构。
    /// 用法：`PROBE_VARIANT=asis|vreg cargo test probe_rank_login_form -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").expect("PROBE_COOKIE");
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());

        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "vreg" {
            let bump = [
                ["version_code", hongguo_signer::REGISTER_VERSION_CODE_STR],
                ["version_name", hongguo_signer::REGISTER_VERSION_NAME],
                [
                    "manifest_version_code",
                    hongguo_signer::REGISTER_VERSION_CODE_STR,
                ],
                [
                    "update_version_code",
                    hongguo_signer::REGISTER_VERSION_CODE_STR,
                ],
            ];
            let fields = v["fields"].as_array_mut().expect("fields");
            for [k, val] in bump {
                for f in fields.iter_mut() {
                    if f[0].as_str() == Some(k) {
                        f[1] = serde_json::json!(val);
                    }
                }
            }
        }
        let device: hongguo_signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let cookie = format!(
            "{}; {}",
            hongguo_signer::device::anonymous_cookie(&device),
            account_cookie
        );
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            device,
            cookie: Some(cookie),
            x_tt_token: None,
        };

        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("榜单请求");
        println!("[{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
        println!(
            "[{variant}] items={} #1={:?}",
            page.items.len(),
            page.items.first().map(|i| i.title.as_str())
        );
    }

    /// 形态二分补充：注册口径版本 + 匿名（区分版本号与会话哪个是开关）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_login_form_anon() {
        let variant = std::env::var("PROBE_VARIANT").unwrap_or_else(|_| "asis".into());
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let mut v: Value = serde_json::from_str(&device_json).expect("device json");
        if variant == "vreg" {
            let fields = v["fields"].as_array_mut().expect("fields");
            for f in fields.iter_mut() {
                match f[0].as_str() {
                    Some("version_code" | "manifest_version_code" | "update_version_code") => {
                        f[1] = serde_json::json!(hongguo_signer::REGISTER_VERSION_CODE_STR);
                    }
                    Some("version_name") => {
                        f[1] = serde_json::json!(hongguo_signer::REGISTER_VERSION_NAME)
                    }
                    _ => {}
                }
            }
        }
        let device: hongguo_signer::device::DeviceProfile = serde_json::from_value(v).unwrap();
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("榜单请求");
        println!("[anon/{variant}] tabs={} 第一层 id:", page.tabs.len());
        for tab in &page.tabs {
            let subs: Vec<&str> = tab.subs.iter().map(|s| s.name.as_str()).collect();
            println!("  {:?} {:?} subs={:?}", tab.id, tab.name, subs);
        }
    }

    /// 排行榜内容 tab「演员」形态验证（ranklist_celebrity；
    /// 响应 cell_data 若不是剧集形态则前端跳过该 tab）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_celebrity() {
        let env = hg_env(&anon_env());
        let q: Vec<(String, String)> = [
            ("cell_id", "7470092475068071998"),
            ("tab_type", "26"),
            ("selected_items", "ranklist_celebrity"),
            ("category_id", "0"),
            ("cell_sub_id", "0"),
            ("client_req_type", "2"),
            ("client_template", "2"),
            ("gender", "2"),
            ("limit", "0"),
            ("offset", "0"),
            ("unlimited_selector_change_type", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = api_call_reading(LQ_API_ORIGIN, RANK_CELL_PATH, None, &q, &env)
            .await
            .expect("演员榜请求");
        let v: Value = serde_json::from_slice(&bytes).expect("JSON");
        println!("[celebrity] code = {:?}", v.get("code"));
        let data = &v["data"];
        println!(
            "[celebrity] data keys = {:?}",
            data.as_object().map(|m| m.keys().collect::<Vec<_>>())
        );
        let mut raws = Vec::new();
        collect_video_data(data, &mut raws);
        println!("[celebrity] video_data 条数 = {}", raws.len());
        if let Some(first) = raws.first() {
            println!(
                "[celebrity] 首条 keys = {:?}",
                first
                    .as_object()
                    .map(|m| m.keys().take(20).collect::<Vec<_>>())
            );
            println!(
                "[celebrity] title = {:?}, series_id = {:?}, sub_title = {:?}",
                first.get("title").and_then(Value::as_str),
                first.get("series_id").and_then(Value::as_str),
                first.get("sub_title").and_then(Value::as_str),
            );
        }
    }

    /// 筛选面板参数真连：同一子榜，总榜 vs 女频 vs 题材，条目应变化且带 tabs schema。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_panel_filter() {
        let env = hg_env(&anon_env());
        let base = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("总榜");
        println!(
            "[panel:总榜] {} 条 #1={:?} tabs={}",
            base.items.len(),
            base.items.first().map(|i| i.title.as_str()),
            base.tabs.len()
        );
        assert!(!base.items.is_empty());
        assert_eq!(base.tabs[0].id, "all", "tabs schema 应随行下发");

        for p in ["gender_female", "gender_male", "cate_308"] {
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_sc",
                    panel: Some(p),
                    ..Default::default()
                },
                &env,
            )
            .await
            .unwrap_or_else(|e| panic!("panel={p}: {e}"));
            println!(
                "[panel:{p}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "panel={p} 不应为空");
        }

        // 内容 tab 切换（真人剧 / 漫剧 / 系列剧，官方样本 id）
        for (sel, sub) in [
            ("human", "human_hot_sc"),
            ("comic_series_rank", "comic_series_hot_rank"),
            ("series_album", "series_album_hot_sc"),
        ] {
            let page = fetch_rank_ex(
                RankReq {
                    selected: sel,
                    sub,
                    ..Default::default()
                },
                &env,
            )
            .await
            .unwrap_or_else(|e| panic!("{sel}/{sub}: {e}"));
            println!(
                "[tab:{sel}] {} 条 #1={:?}",
                page.items.len(),
                page.items.first().map(|i| i.title.as_str())
            );
            assert!(!page.items.is_empty(), "{sel}/{sub} 不应为空");
        }
    }

    /// app 生产同款设备（匿名档案 + 匿名 cookie）下发的 selector schema：
    /// 排查两端 tabs 是否一致（灰度可能按设备分发）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_selector_app_device() {
        let device = hongguo_signer::anonymous_profile(&hongguo_signer::fresh_cdid());
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
            cookie: Some(hongguo_signer::device::anonymous_cookie(&device)),
            device,
            x_tt_token: None,
        };
        let page = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("app 同款设备榜单");
        println!(
            "[app-device:tabs] {:?}",
            page.tabs
                .iter()
                .map(|t| (t.id.as_str(), t.name.as_str(), t.subs.len()))
                .collect::<Vec<_>>()
        );
        println!(
            "[app-device:subs] {:?}",
            page.tabs[0]
                .subs
                .iter()
                .map(|s| (s.id.as_str(), s.name.as_str(), s.panel.len()))
                .collect::<Vec<_>>()
        );
    }

    /// 翻页协议真连验证：首页（usct=2 无
    /// 会话）→ 翻页（usct=1 + session_id + rank_version 回传，filter_ids
    /// 由会话账本自动带上），两页条目应为 10 条上下且第二页出现新条目。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_paging_session() {
        let env = hg_env(&anon_env());
        let page1 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_must_watch",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("必看榜首页");
        println!(
            "[page1] {} 条 #1={:?} rv={} sess={}",
            page1.items.len(),
            page1.items.first().map(|i| i.title.as_str()),
            page1.rank_version,
            page1.session_id,
        );
        assert!(!page1.session_id.is_empty(), "首页应下发会话");
        assert!(
            page1.next_offset > 0 && page1.has_more,
            "首页应下发翻页游标"
        );

        let page2 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_must_watch",
                offset: page1.next_offset,
                session_id: &page1.session_id,
                rank_version: &page1.rank_version,
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("必看榜翻页");
        println!(
            "[page2] {} 条 #1={:?} 去重后 {} 条",
            page2.items.len(),
            page2.items.first().map(|i| i.title.as_str()),
            page2.items.len(),
        );
        let p1_ids: std::collections::HashSet<_> =
            page1.items.iter().map(|i| i.series_id.as_str()).collect();
        let fresh = page2
            .items
            .iter()
            .filter(|i| !p1_ids.contains(i.series_id.as_str()))
            .count();
        println!("[page2] 与首页不重复条目 {fresh}");
        assert!(
            !page2.items.is_empty(),
            "翻页不应为空（usct=1 + filter_ids 形态）"
        );
    }

    /// 深滚逐页对照：连拉到 has_more=false，逐页打印与官方 diff。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_deep_paging_diff() {
        let env = hg_env(&anon_env());
        let mut offset = 0i64;
        let mut session = String::new();
        let mut rank_version = String::new();
        let mut page_no = 0;
        loop {
            page_no += 1;
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_play_sc",
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                    ..Default::default()
                },
                &env,
            )
            .await
            .expect("榜单页");
            let ids: Vec<String> = page
                .items
                .iter()
                .map(|i| i.series_id[i.series_id.len().saturating_sub(6)..].to_string())
                .collect();
            println!(
                "[p{page_no}] items={} ids={ids:?} next={} has_more={}",
                page.items.len(),
                page.next_offset,
                page.has_more,
            );
            let (next_offset, has_more) = (page.next_offset, page.has_more);
            session = page.session_id;
            rank_version = page.rank_version;
            offset = next_offset;
            if !has_more || next_offset <= 0 || page_no >= 12 {
                break;
            }
        }
    }

    /// 重挂载重拉模拟：翻 3 页后按前端重拉形态再请求第 2 页，
    /// 判定服务端回原窗口还是跳过已见。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_refetch_simulation() {
        let env = hg_env(&anon_env());
        let mut offset = 0i64;
        let mut session = String::new();
        let mut rank_version = String::new();
        let mut pass1 = Vec::new();
        for _ in 0..3 {
            let page = fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub: "ranklist_hot_play_sc",
                    offset,
                    session_id: &session,
                    rank_version: &rank_version,
                    ..Default::default()
                },
                &env,
            )
            .await
            .expect("首轮翻页");
            pass1.extend(page.items.iter().map(|i| tail6(&i.series_id)));
            session = page.session_id;
            rank_version = page.rank_version;
            offset = page.next_offset;
        }
        println!("[首轮3页] 共 {} 条: {pass1:?}", pass1.len());

        let p1 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_play_sc",
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("重拉首页");
        println!("[重拉p1] {} 条", p1.items.len());
        let p2 = fetch_rank_ex(
            RankReq {
                selected: "all",
                sub: "ranklist_hot_play_sc",
                offset: 10,
                session_id: &session,
                rank_version: &rank_version,
                ..Default::default()
            },
            &env,
        )
        .await
        .expect("重拉第2页（旧会话满 fid）");
        let p2_ids: Vec<String> = p2.items.iter().map(|i| tail6(&i.series_id)).collect();
        println!("[重拉p2] {} 条: {p2_ids:?}", p2.items.len());
        let head: Vec<String> = pass1.iter().skip(10).take(10).cloned().collect();
        println!("[判定] 首轮第 11~20 位 = {head:?}");
        println!(
            "[判定] 重拉 p2 {}（原窗口=未被 fid 排异；跳新条目=fid 全局排异）",
            if p2_ids == head {
                "回原窗口"
            } else {
                "跳过已见"
            }
        );
    }

    fn tail6(id: &str) -> String {
        id[id.len().saturating_sub(6)..].to_string()
    }

    /// 排行榜直连：8 个「全部」tab 子榜逐个打，code==0 且条目数 >0 即转正。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_rank_all_lists() {
        let env = hg_env(&anon_env());
        for sub in ALL_TAB_SUBS {
            match fetch_rank_ex(
                RankReq {
                    selected: "all",
                    sub,
                    ..Default::default()
                },
                &env,
            )
            .await
            {
                Ok(page) => {
                    let top = page.items.first();
                    println!(
                        "[rank/{sub}] {} 条, #1={:?} rank={:?} rec={:?}",
                        page.items.len(),
                        top.map(|i| i.title.as_str()).unwrap_or(""),
                        top.map(|i| i.rank).unwrap_or(0),
                        top.map(|i| i.rec_text.as_str()).unwrap_or(""),
                    );
                    assert!(!page.items.is_empty(), "{sub} 榜单不应为空");
                }
                Err(e) => panic!("[rank/{sub}] ERR {e}"),
            }
        }
    }
}
