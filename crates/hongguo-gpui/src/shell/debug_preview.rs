use super::*;

#[cfg(feature = "screenshot")]
use crate::detail::DetailEvent;

impl super::Shell {
    /// 预览种子分发（screenshot feature）：rank 场景切到排行榜并灌假榜单，
    /// 其余全走首页。未知场景键的错误向上传（矩阵写错键名启动即报，
    /// 开发工具宁可显性失败）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(
        &mut self,
        scene: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if scene.starts_with("rank") {
            self.dest = Dest::Rank;
            self.rank
                .update(cx, |rank, cx| rank.debug_preview_seed(scene, cx));
        } else if scene.starts_with("storage") {
            self.dest = Dest::Storage;
            self.storage
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("liked") {
            self.dest = Dest::Liked;
            self.liked
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("collections") {
            self.dest = Dest::Collections;
            self.collections
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("history") {
            self.dest = Dest::History;
            self.history
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("reservation") {
            self.dest = Dest::Reservations;
            self.reservations
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("follows") {
            self.dest = Dest::Follows;
            self.follows
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("tasks") {
            self.dest = Dest::Tasks;
            self.tasks
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("settings") {
            self.dest = Dest::Settings;
            self.settings_page
                .update(cx, |view, cx| view.debug_preview_seed(scene, window, cx));
        } else if scene.starts_with("browse") {
            self.dest = Dest::Browse;
            self.browse
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("new-calendar") {
            self.dest = Dest::New;
            self.new_drama
                .update(cx, |view, cx| view.debug_preview_seed("new-calendar", cx));
        } else if scene.starts_with("new") {
            self.dest = Dest::New;
            self.new_drama
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else if scene.starts_with("merge") {
            self.dest = Dest::Merge;
            self.merge
                .update(cx, |view, cx| view.debug_preview_seed(scene, window, cx));
        } else if scene.starts_with("detail") {
            // 详情页场景：确保实体在（不走网络 load，直接灌种子）
            self.dest = Dest::Detail;
            if self.detail.is_none() {
                let v = cx.new(|cx| DetailView::new(self.state.clone(), cx));
                cx.subscribe(&v, |this, _, event: &DetailEvent, cx| {
                    this.on_detail_event(event.clone(), cx);
                })
                .detach();
                self.detail = Some(v);
            }
            if let Some(detail) = &self.detail {
                detail.update(cx, |view, cx| view.debug_preview_seed(scene, cx));
            }
            self.detail_series = Some("7100000".into());
        } else if scene.starts_with("creator") {
            self.dest = Dest::Creator;
            self.creator
                .update(cx, |view, cx| view.debug_preview_seed(scene, cx));
        } else {
            self.dest = Dest::Home;
            self.feed
                .update(cx, |home, cx| home.debug_preview_seed(scene, window, cx))?;
        }
        cx.notify();
        Ok(())
    }
}
