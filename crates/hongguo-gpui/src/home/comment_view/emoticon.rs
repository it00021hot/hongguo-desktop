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
/// 动图帧数上限（内存护栏：预warm几十个表情时每个动图的位图总量有界）。
const EMOTICON_MAX_FRAMES: usize = 30;
/// 面板一次预解码的图数（其余等用户滚动时再拉，避免一开面板就几十个请求）。
const EMOTICON_PREWARM: usize = 40;

/// 面板档位（官方面板顺序：搜索 → 系统表情 → 自定义表情 → 第三方 GIF，
/// 与 Tauri `EmoticonPanel` 四档同序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EmoticonTab {
    /// 搜索表情（官方 `search_type=1`，逐字前缀匹配）。
    Search,
    /// 系统表情（壳内内置）。
    #[default]
    System,
    /// 第三方表情（官方 GIF tab）。
    ThirdParty,
    /// 自定义表情（官方 ♡ tab）。
    Own,
}

/// 评论本地图片附件草稿（选图不请求网络，发表时才上传——Tauri 同口径）。
#[derive(Clone)]
pub(crate) struct LocalImageAttachment {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    /// 解码好的预览位图（附件预览行直接画）。
    pub preview: Arc<gpui_kit::RenderImage>,
}

/// 面板状态：档位 + 三份远端列表。
#[derive(Default)]
pub(crate) struct EmoticonPanel {
    /// 当前档位。
    pub tab: EmoticonTab,
    /// 第三方表情列表。
    pub third_party: Vec<Emoticon>,
    /// 自定义表情列表。
    pub own: Vec<Emoticon>,
    /// 搜索结果（关键词非空才请求，空关键词保持空列表——Tauri 的
    /// `enabled && trimmed.length > 0` 同一门）。
    pub search: Vec<Emoticon>,
    /// 远端列表在拉。
    pub loading: bool,
    /// 「加载更多」在拉（追加期间不把整格打成加载态）。
    pub loading_more: bool,
    /// 自定义表情上传在飞（上传 tile 转圈 + 防重入）。
    pub uploading: bool,
    /// 第三方列表已到尽头（上一页返回空 → 收起「加载更多」）。
    pub third_party_exhausted: bool,
}

impl EmoticonPanel {
    /// 当前档位该显示的列表。
    pub fn items(&self) -> &[Emoticon] {
        match self.tab {
            EmoticonTab::ThirdParty => &self.third_party,
            EmoticonTab::Own => &self.own,
            EmoticonTab::Search => &self.search,
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
                // 动图（GIF）的当前帧存 img 的元素状态，没有 id 的 img
                // 永远停在第一帧（官方文档 Image › Troubleshooting 明载）。
                .id(gpui_kit::ElementId::from(format!(
                    "emoticon-img-{}",
                    item.web_uri
                )))
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
            // 搜索档没有「拉一次就缓存」：空关键词保持空态，有关键词才查
            // （结果按关键词命中缓存，切回不重查同词）。
            EmoticonTab::Search => {
                self.emoticon_panel.search.is_empty() && self.emoticon_keyword(cx).is_empty()
            }
            EmoticonTab::System => true,
        };
        if cached {
            cx.notify();
            return;
        }
        self.emoticon_panel.loading = true;
        self.load_emoticon_tab(tab, cx);
    }

    /// 当前搜索关键词（面板搜索输入框内容，已 trim）。
    pub(super) fn emoticon_keyword(&self, cx: &Context<Self>) -> String {
        self.emoticon_search_input
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default()
    }

    /// 搜索表情（逐字前缀匹配：输入变化即查，Tauri useQuery 同口径）。
    pub(crate) fn search_emoticons(&mut self, cx: &mut Context<Self>) {
        let keyword = self.emoticon_keyword(cx);
        if keyword.is_empty() {
            self.emoticon_panel.search.clear();
            self.emoticon_panel.loading = false;
            cx.notify();
            return;
        }
        if self.emoticon_panel.loading {
            return; // 上一发查询在飞：结果回来后以最新关键词再判
        }
        self.emoticon_panel.loading = true;
        let state = Arc::clone(&self.state);
        let task_keyword = keyword.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::emoticon::search_emoticons(&task_keyword, 15, 0, true, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.emoticon_panel.loading = false;
                // 只收最后一次查询的结果（防乱序回包覆盖新词）
                if view.emoticon_keyword(cx) == keyword
                    && let Ok(page) = result
                {
                    view.emoticon_panel.search = page.items;
                    let items: Vec<Emoticon> = view
                        .emoticon_panel
                        .search
                        .iter()
                        .take(EMOTICON_PREWARM)
                        .cloned()
                        .collect();
                    view.request_emoticon_images(&items, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 第三方档「加载更多」（offset 翻页追加；空页即到尽头）。
    pub(super) fn load_more_third_party(&mut self, cx: &mut Context<Self>) {
        if self.emoticon_panel.loading_more || self.emoticon_panel.third_party_exhausted {
            log::debug!(
                "[Emoticon] 加载更多被门拦下：loading_more={} exhausted={}",
                self.emoticon_panel.loading_more,
                self.emoticon_panel.third_party_exhausted
            );
            return;
        }
        self.emoticon_panel.loading_more = true;
        let offset = self.emoticon_panel.third_party.len() as i64;
        log::debug!("[Emoticon] 加载更多发起：offset={offset}");
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::emoticon::list_third_party_emoticons(100, offset, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.emoticon_panel.loading_more = false;
                match &result {
                    Ok(page) => log::debug!(
                        "[Emoticon] 加载更多回包：{} 条（累计 {}）",
                        page.items.len(),
                        view.emoticon_panel.third_party.len() + page.items.len()
                    ),
                    Err(e) => log::warn!("[Emoticon] 加载更多失败: {e}"),
                }
                if let Ok(page) = result {
                    view.emoticon_panel.third_party_exhausted = page.items.is_empty();
                    let items: Vec<Emoticon> =
                        page.items.iter().take(EMOTICON_PREWARM).cloned().collect();
                    view.emoticon_panel.third_party.extend(page.items);
                    view.request_emoticon_images(&items, cx);
                }
                cx.notify();
            });
        })
        .detach();
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
                EmoticonTab::Search | EmoticonTab::System => {
                    unreachable!("搜索档走 search_emoticons、系统表情档不拉远端")
                }
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
                        EmoticonTab::Search | EmoticonTab::System => {}
                    }
                    let items: Vec<Emoticon> = view
                        .emoticon_panel
                        .items()
                        .iter()
                        .take(EMOTICON_PREWARM)
                        .cloned()
                        .collect();
                    view.request_emoticon_images(&items, cx);
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

    /// 请求一批表情图位图（面板/评论里的图共用一份缓存，URL 命中或在飞
    /// 即跳过）。
    ///
    /// 取图 URL 用 `dynamic_url` 优先——官方 `web_uri` 是静态图（jpeg/png/
    /// webp），动图在 `dynamic_url`（GIF）；缓存键仍用 `web_uri`（评论渲染
    /// 只认它）。在飞去重由 [`HomeView::emoticon_fetching`] 承担，滚动到
    /// 哪请求到哪（渲染期每帧调用本函数，命中后零开销）。
    pub(crate) fn request_emoticon_images(&self, items: &[Emoticon], cx: &Context<Self>) {
        let to_fetch: Vec<(String, String)> = items
            .iter()
            .filter_map(|item| {
                let key = item.web_uri.clone();
                if key.is_empty()
                    || self.emoticon_images.contains_key(&key)
                    || self.emoticon_fetching.borrow().contains(&key)
                {
                    return None;
                }
                let url = item
                    .dynamic_url
                    .clone()
                    .filter(|url| !url.is_empty())
                    .unwrap_or_else(|| key.clone());
                Some((key, url))
            })
            .collect();
        if to_fetch.is_empty() {
            return;
        }
        self.emoticon_fetching
            .borrow_mut()
            .extend(to_fetch.iter().map(|(key, _)| key.clone()));
        for (key, url) in to_fetch {
            let rx = bridge::spawn(async move { fetch_emoticon_image(url).await });
            cx.spawn(async move |this, cx| {
                let decoded = rx.await.expect("tokio 桥不会丢弃发送端");
                let _ = this.update(cx, |view, _| {
                    view.emoticon_fetching.borrow_mut().remove(&key);
                    if let Some(decoded) = decoded {
                        // 位图在 GPUI 侧造（RenderImage 不跨线程，跨桥的是像素）
                        let image = match decoded {
                            DecodedEmoticon::Static(rgba) => {
                                crate::image_upload::render_image(rgba)
                            }
                            DecodedEmoticon::Animated(frames) => {
                                crate::image_upload::render_image_frames(frames)
                            }
                        };
                        view.emoticon_images.insert(key, Arc::new(image));
                    }
                });
            })
            .detach();
        }
    }

    /// 评论附图等只有 URL 的场景：键与取图 URL 同为 `web_uri`（历史评论的
    /// 附图是用户上传的静态图，无动图形态）。
    pub(crate) fn request_emoticon_image_urls(&self, urls: &[String], cx: &Context<Self>) {
        let items: Vec<Emoticon> = urls
            .iter()
            .filter(|url| !url.is_empty())
            .map(|url| Emoticon {
                web_uri: url.clone(),
                ..Default::default()
            })
            .collect();
        self.request_emoticon_images(&items, cx);
    }

    /// 选本地图片（评论附图与「上传自定义表情」共用入口）：系统文件面板 →
    /// 桥线程读字节 + 解码，回主线程按用途分流。
    ///
    /// 面板必须用 rfd 的 **Async**FileDialog：同步版在 GPUI 事件回调里阻塞
    /// 主线程，模态递归进事件循环时 GPUI 的上下文 RefCell 还在借用中，
    /// 重入直接 panic（`RefCell already borrowed`，2026-10-26 实测）；异步版
    /// 由 rfd 派发到主线程呈现、回调经 waker 驱动，不嵌套 GPUI 的派发。
    pub(super) fn pick_local_image(&mut self, purpose: LocalImagePurpose, cx: &mut Context<Self>) {
        let rx = bridge::spawn(async move {
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter(
                    crate::i18n::t("player.emoticon.image").as_str(),
                    &["png", "jpg", "jpeg", "webp", "gif"],
                )
                .pick_file()
                .await
            else {
                return Ok(None); // 用户取消
            };
            let path = file.path().to_string_lossy().to_string();
            read_local_image(path)
                .await
                .ok_or_else(|| "图片读取失败".to_string())
                .map(Some)
        });
        cx.spawn(async move |this, cx| {
            let decoded = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match (decoded, purpose) {
                (Ok(Some(raw)), LocalImagePurpose::CommentAttachment) => {
                    view.comment_local_attachment = Some(LocalImageAttachment {
                        preview: Arc::new(crate::image_upload::render_image(raw.rgba)),
                        bytes: raw.bytes,
                        filename: raw.filename,
                        width: raw.width,
                        height: raw.height,
                        format: raw.format,
                    });
                    // 本地图与表情图两态互斥（官方同款：新附件顶掉旧附件）
                    view.comment_attachment = None;
                    cx.notify();
                }
                (Ok(Some(raw)), LocalImagePurpose::CustomEmoticon) => {
                    view.upload_custom_emoticon(raw, cx);
                }
                (Ok(None), _) => {}
                (Err(message), _) => {
                    view.show_toast(
                        crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// 上传自定义表情（官方两步：`upload/picture` → collect 收藏进「我的
    /// 表情」）；成功回执与列表刷新走收藏同一条链。
    fn upload_custom_emoticon(&mut self, raw: LocalImageRaw, cx: &mut Context<Self>) {
        if self.emoticon_panel.uploading {
            return;
        }
        self.emoticon_panel.uploading = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::emoticon::upload_picture(&raw.bytes, &raw.filename, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match result {
                Ok(uploaded) => {
                    view.emoticon_panel.uploading = false;
                    let request = EmoticonCollectRequest {
                        collect_type: COLLECT_TYPE_ADD,
                        image_height: raw.height,
                        image_id: String::new(),
                        image_type: 1,
                        image_url: uploaded.url,
                        image_width: raw.width,
                    };
                    view.run_collect_emoticon(request, "player.emoticon.uploadDone", cx);
                }
                Err(e) => {
                    view.emoticon_panel.uploading = false;
                    log::warn!("[Home] 自定义表情上传失败: {e}");
                    let message = crate::i18n::app_error_text(&e);
                    view.show_toast(
                        crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

/// 拉一张表情图并解码：动图（GIF）解成多帧带 delay 的序列交 `img` 元素
/// 原生动画；静图单帧。动图帧数封顶 [`EMOTICON_MAX_FRAMES`]（预warm几十个
/// 表情 × 每个几十帧的位图内存必须有个上界）。
async fn fetch_emoticon_image(url: String) -> Option<DecodedEmoticon> {
    let response = reqwest::get(&url).await.ok()?;
    let bytes = response.bytes().await.ok()?.to_vec();
    let decoded = if bytes.starts_with(b"GIF8") {
        decode_gif(&bytes).map(DecodedEmoticon::Animated)
    } else {
        None
    };
    let decoded = match decoded {
        Some(decoded) => decoded,
        None => DecodedEmoticon::Static(decode_static(&bytes)?),
    };
    match &decoded {
        DecodedEmoticon::Animated(frames) => {
            log::debug!(
                "[Emoticon] 动图 {frame_count} 帧 url={url}",
                frame_count = frames.len()
            )
        }
        DecodedEmoticon::Static(_) => log::debug!("[Emoticon] 静图 url={url}"),
    }
    Some(decoded)
}

/// 静图解码 + 限定最大边长。
fn decode_static(bytes: &[u8]) -> Option<image::RgbaImage> {
    let decoded = image::load_from_memory(bytes).ok()?;
    Some(resize_image(decoded))
}

/// GIF 解码成全画布帧序列（`image` crate 的 GifDecoder 按 disposal method
/// 合成，吐出的帧都是整幅画布，可直接逐帧 resize）。
fn decode_gif(bytes: &[u8]) -> Option<Vec<image::Frame>> {
    use image::AnimationDecoder as _;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let mut frames = Vec::new();
    for frame in decoder.into_frames() {
        let mut frame = frame.ok()?;
        let buffer = std::mem::take(frame.buffer_mut());
        let mut resized = resize_image(image::DynamicImage::ImageRgba8(buffer));
        std::mem::swap(frame.buffer_mut(), &mut resized);
        frames.push(frame);
        if frames.len() >= EMOTICON_MAX_FRAMES {
            break;
        }
    }
    (!frames.is_empty()).then_some(frames)
}

fn resize_image(decoded: image::DynamicImage) -> image::RgbaImage {
    if decoded.width().max(decoded.height()) > EMOTICON_MAX_EDGE {
        decoded
            .resize(
                EMOTICON_MAX_EDGE,
                EMOTICON_MAX_EDGE,
                image::imageops::FilterType::Triangle,
            )
            .into_rgba8()
    } else {
        decoded.into_rgba8()
    }
}

/// 本地图片的用途（选图后分流：进评论附件草稿 or 直传自定义表情）。
#[derive(Clone, Copy)]
pub(crate) enum LocalImagePurpose {
    CommentAttachment,
    CustomEmoticon,
}

/// 桥线程解码出的表情图：静图单帧 / 动图多帧序列（像素过桥，RenderImage
/// 回主线程再造）。
pub(crate) enum DecodedEmoticon {
    Static(image::RgbaImage),
    Animated(Vec<image::Frame>),
}

/// 桥线程解码出的本地图片（bytes 留给上传，rgba 回主线程造预览）。
pub(crate) struct LocalImageRaw {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub width: i64,
    pub height: i64,
    pub format: String,
    pub rgba: image::RgbaImage,
}

/// 读本地图片文件并解码（bytes/尺寸/格式一次带齐；失败返回 None）。
/// 本地小图用 std::fs 同步读——跑在桥线程，不占 GPUI 主线程。
async fn read_local_image(path: String) -> Option<LocalImageRaw> {
    let bytes = std::fs::read(&path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let filename = path.rsplit('/').next()?.to_string();
    let format = filename
        .rsplit('.')
        .next()
        .unwrap_or("png")
        .to_ascii_lowercase();
    let (width, height) = (decoded.width() as i64, decoded.height() as i64);
    Some(LocalImageRaw {
        bytes,
        filename,
        width,
        height,
        format,
        rgba: decoded.into_rgba8(),
    })
}
