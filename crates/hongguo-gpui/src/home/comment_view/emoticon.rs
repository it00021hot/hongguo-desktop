//! 表情面板（官方三档：系统表情 / 第三方 GIF / 自定义表情）。
//!
//! 2026-10-25 官方 App 7.3.9.32 实抓对齐：
//! - **系统表情**：壳内内置（webp + `[名字]` 文本码），发评论仍是纯文本——
//!   与 Tauri 壳同口径，不走服务端；
//! - **第三方 GIF**（官方 GIF tab）：服务端 `search_type=2` 下发，官方面板
//!   顶部标「内容来自于第三方」；选中后作为**附图**发送（`image_data` +
//!   `business_param.emoji_scene_type=2`）；
//! - **自定义表情**（官方 ♡ tab）：服务端 `search_type=4&keyword=profile`
//!   下发我上传的表情（`emoji_scene_type=4`）。
//!
//! 图源：官方 emoticon 的 `web_uri` 是静态图（jpeg/png/webp），`dynamic_url`
//! 才是动图——壳内 `image` crate 不解 gif，按静态图画（Tauri 壳那边放的是
//! 动图，两边形态差异在此显式说明）。

use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ImageSource, ObjectFit};

use hongguo_api::emoticon::{Emoticon, EmoticonCollectRequest};
use hongguo_api::params::emoticon::{COLLECT_TYPE_ADD, COLLECT_TYPE_DELETE};

use crate::bridge;
use crate::home::HomeView;

/// 表情图显示边长上限（物理像素；面板格子 48px，2 倍图足够清晰）。
const EMOTICON_MAX_EDGE: u32 = 128;
/// 面板一次预解码的图数（其余等用户滚动时再拉，避免一开面板就几十个请求）。
const EMOTICON_PREWARM: usize = 40;

/// 面板档位（官方面板顺序：系统表情 → 第三方 GIF → 自定义表情）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EmoticonTab {
    /// 系统表情（壳内内置）。
    #[default]
    System,
    /// 第三方表情（官方 GIF tab）。
    ThirdParty,
    /// 自定义表情（官方 ♡ tab）。
    Own,
}

/// 面板状态：档位 + 两份远端列表。
#[derive(Default)]
pub(crate) struct EmoticonPanel {
    /// 当前档位。
    pub tab: EmoticonTab,
    /// 第三方表情列表。
    pub third_party: Vec<Emoticon>,
    /// 自定义表情列表。
    pub own: Vec<Emoticon>,
    /// 远端列表在拉。
    pub loading: bool,
}

impl EmoticonPanel {
    /// 当前档位该显示的列表。
    pub fn items(&self) -> &[Emoticon] {
        match self.tab {
            EmoticonTab::ThirdParty => &self.third_party,
            EmoticonTab::Own => &self.own,
            EmoticonTab::System => &[],
        }
    }
}

impl HomeView {
    /// 表情图元素（没解码好就返回 None，调用方给占位）。
    pub(super) fn emoticon_image(&self, item: &Emoticon) -> Option<AnyElement> {
        let cached = self.emoticon_images.get(&item.web_uri)?;
        Some(
            gpui_kit::img(ImageSource::Render(cached.clone()))
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
        )
    }

    /// 切档并按需拉列表（同一档只拉一次，回包进面板状态）。
    pub(super) fn select_emoticon_tab(&mut self, tab: EmoticonTab, cx: &mut Context<Self>) {
        self.emoticon_panel.tab = tab;
        let cached = match tab {
            EmoticonTab::ThirdParty => !self.emoticon_panel.third_party.is_empty(),
            EmoticonTab::Own => !self.emoticon_panel.own.is_empty(),
            EmoticonTab::System => true,
        };
        if cached {
            cx.notify();
            return;
        }
        self.emoticon_panel.loading = true;
        self.load_emoticon_tab(tab, cx);
    }

    /// 拉某一档的远端列表（切档与收藏后刷新都走这里）。
    ///
    /// 不动 `loading` 与当前档位——收藏后刷新「我的表情」时用户可能停在
    /// 第三方档，不能把当前档打成加载态。
    pub(super) fn load_emoticon_tab(&mut self, tab: EmoticonTab, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            match tab {
                EmoticonTab::ThirdParty => {
                    hongguo_api::emoticon::list_third_party_emoticons(100, 0, &env).await
                }
                EmoticonTab::Own => hongguo_api::emoticon::list_own_emoticons(49, 0, &env).await,
                EmoticonTab::System => unreachable!("系统表情档不拉远端"),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.emoticon_panel.loading = false;
                if let Ok(page) = result {
                    match tab {
                        EmoticonTab::ThirdParty => view.emoticon_panel.third_party = page.items,
                        EmoticonTab::Own => view.emoticon_panel.own = page.items,
                        EmoticonTab::System => {}
                    }
                    let urls: Vec<String> = view
                        .emoticon_panel
                        .items()
                        .iter()
                        .take(EMOTICON_PREWARM)
                        .map(|item| item.web_uri.clone())
                        .collect();
                    view.prewarm_emoticon_images(&urls, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 收藏一张表情到「我的表情」（官方同款：长按/右键 → 「添加到表情」）。
    ///
    /// 官方实包（2026-10-25）：`collect_type=1`，`image_id` 填**源表情 id**，
    /// `image_url` 填该表情的静态 URL（官方收藏动图表情时传的也是静态
    /// `web_uri`，`image_type` 仍是 2）；服务端把它并进「我的表情」列表
    /// （与上传的自定义表情同一列表，`search_type=4&keyword=profile`）。
    pub(super) fn collect_emoticon(&mut self, item: &Emoticon, cx: &mut Context<Self>) {
        let request = EmoticonCollectRequest {
            collect_type: COLLECT_TYPE_ADD,
            image_height: item.height,
            image_id: item.id.clone().unwrap_or_default(),
            image_type: item.image_type,
            image_url: item.web_uri.clone(),
            image_width: item.width,
        };
        self.run_collect_emoticon(request, "player.emoticon.addDone", cx);
    }

    /// 从「我的表情」删除（官方同款：长按/右键 → 「删除表情」）。
    ///
    /// 官方实包（2026-10-25）：同一端点、`collect_type=2`；服务端回
    /// `image_data: null`，没有可回显的表情对象。
    pub(super) fn delete_emoticon(&mut self, item: &Emoticon, cx: &mut Context<Self>) {
        let request = EmoticonCollectRequest {
            collect_type: COLLECT_TYPE_DELETE,
            image_height: item.height,
            image_id: item.id.clone().unwrap_or_default(),
            image_type: item.image_type,
            image_url: item.web_uri.clone(),
            image_width: item.width,
        };
        self.run_collect_emoticon(request, "player.emoticon.removeDone", cx);
    }

    /// 发收藏/删除请求，成功后刷新「我的表情」列表并回执。
    ///
    /// 失败必须可见：服务端拒绝（如「已添加过该表情」）沉默下去，用户会
    /// 以为收藏成功了（Tauri 壳的全局错误 toast 同位）。
    fn run_collect_emoticon(
        &mut self,
        request: EmoticonCollectRequest,
        success_key: &'static str,
        cx: &mut Context<Self>,
    ) {
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::emoticon::collect_emoticon(&request, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(_) => {
                        view.load_emoticon_tab(EmoticonTab::Own, cx);
                        view.show_toast(crate::i18n::t(success_key), cx);
                    }
                    Err(e) => {
                        log::warn!("[Home] 表情收藏操作失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 预解码一批表情图（面板/评论里的图共用一份缓存，URL 命中即跳过）。
    pub(crate) fn prewarm_emoticon_images(&mut self, urls: &[String], cx: &mut Context<Self>) {
        for url in urls {
            if url.is_empty() || self.emoticon_images.contains_key(url) {
                continue;
            }
            let target = url.clone();
            let rx = bridge::spawn(async move { fetch_emoticon_image(target).await });
            cx.spawn(async move |this, cx| {
                let decoded = rx.await.expect("tokio 桥不会丢弃发送端");
                let _ = this.update(cx, |view, _| {
                    if let Some((url, rgba)) = decoded {
                        // 位图在 GPUI 侧造（RenderImage 不跨线程，跨桥的是像素）
                        view.emoticon_images
                            .insert(url, Arc::new(crate::image_upload::render_image(rgba)));
                    }
                });
            })
            .detach();
        }
    }
}

/// 拉一张表情图并解码成 RGBA 像素（失败即没有图，面板给占位）。
async fn fetch_emoticon_image(url: String) -> Option<(String, image::RgbaImage)> {
    let response = reqwest::get(&url).await.ok()?;
    let bytes = response.bytes().await.ok()?.to_vec();
    let decoded = image::load_from_memory(&bytes).ok()?;
    let resized = if decoded.width().max(decoded.height()) > EMOTICON_MAX_EDGE {
        decoded.resize(
            EMOTICON_MAX_EDGE,
            EMOTICON_MAX_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    Some((url, resized.into_rgba8()))
}
