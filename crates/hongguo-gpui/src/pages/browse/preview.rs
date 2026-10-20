//! 找剧页预览与取证种子（screenshot feature）：假数据灌入、真实关键词注入。

use std::collections::HashMap;

use gpui_kit::Window;
use gpui_kit::prelude::*;

impl super::BrowseView {
    /// 预览种子（screenshot feature）：灌假面板/假结果/合成封面，跳过网络。
    /// `browse-search` = 搜索模式；`browse-empty` = 空态。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.panel_rows = Some(crate::preview::fixtures::browse_panel_rows());
        let items = crate::preview::fixtures::browse_feed_items();
        if scene == "browse-search" {
            self.submitted = "命运".into();
            self.search_items = crate::preview::fixtures::search_results();
        }
        if scene == "browse-suggest" {
            // 联想列表：开态 + 高亮第二条（验高亮底色与行高，不走网络）
            self.suggest_items = crate::preview::fixtures::suggest_items();
            self.suggest_open = true;
            self.suggest_index = 1;
        }
        if scene != "browse-empty" && scene != "browse-loading" {
            self.feed_items = items;
        }
        // 首拉加载态：空列表 + 在拉（渲染走整区居中 Spinner）
        self.feed_loading = scene == "browse-loading";
        self.downloaded = HashMap::from([("7100001".to_string(), 3u32)]);
        for (i, item) in self.feed_items.iter().enumerate() {
            crate::preview::fixtures::put_preview_image(
                item.cover.clone(),
                crate::preview::fixtures::cover(i),
            );
        }
        for (i, item) in self.search_items.iter().enumerate() {
            crate::preview::fixtures::put_preview_image(
                item.cover.clone(),
                crate::preview::fixtures::cover(6 + i),
            );
        }
        cx.notify();
    }

    /// 取证辅助（screenshot feature 的 `HONGGUO_SHOT_PANEL=suggest`，配
    /// `HONGGUO_PAGE=browse`）：往搜索框灌一个真实关键词——300ms 防抖后走
    /// 真实联想接口，抓帧拍的是真实列表与首条高亮。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_fill_keyword(
        &mut self,
        keyword: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `set_value` 是静默写值（emit_events=false，不派 InputEvent::Change），
        // 所以这里手动补上 Change 分支该做的事：开联想 + 归零高亮 + 起防抖。
        self.keyword
            .update(cx, |input, cx| input.set_value(keyword, window, cx));
        self.suggest_open = true;
        self.suggest_index = 0;
        self.refresh_suggest(cx);
        cx.notify();
    }
}
