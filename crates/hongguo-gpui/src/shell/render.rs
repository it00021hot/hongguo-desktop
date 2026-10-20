use super::*;

use super::nav::View;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};

impl super::Shell {
    // —— 渲染 ——

    fn render_content(&self, _cx: &Context<Self>) -> gpui_kit::AnyElement {
        match self.dest.view() {
            View::Home => self.feed.clone().into_any_element(),
            View::Rank => self.rank.clone().into_any_element(),
            View::Browse => self.browse.clone().into_any_element(),
            View::New => self.new_drama.clone().into_any_element(),
            View::Liked => self.liked.clone().into_any_element(),
            View::Collections => self.collections.clone().into_any_element(),
            View::Follows => self.follows.clone().into_any_element(),
            View::History => self.history.clone().into_any_element(),
            View::Reservations => self.reservations.clone().into_any_element(),
            View::Tasks => self.tasks.clone().into_any_element(),
            View::Settings => self.settings_page.clone().into_any_element(),
            View::Storage => self.storage.clone().into_any_element(),
            View::Merge => self.merge.clone().into_any_element(),
            View::Creator => self.creator.clone().into_any_element(),
            View::Detail => self
                .detail
                .as_ref()
                .expect("Detail 目的地必有实体")
                .clone()
                .into_any_element(),
        }
    }
}

impl gpui_kit::Render for Shell {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // chrome 全藏的两种形态：小屏播放（480×270 纯播放器，Tauri 小屏
        // 同款，拖窗走播放页拖拽条）与播放器沉浸全屏（播放器全屏按钮/F
        // 进入；顶栏最大化钮是另一档「应用全屏」，chrome 保留，对齐 Tauri）
        let in_mini = crate::mini::is_active();
        // 竖屏：侧栏藏掉、顶栏保留（Tauri app-shell 的 `!miniScreen &&
        // !playerFullscreen && !portrait` 门控同款——竖条只放播放器）
        let in_portrait = crate::portrait::is_active();
        let immersive = self.feed.read(cx).immersive_fullscreen();
        let chrome_hidden = in_mini || immersive;
        // 窗口几何采样按当前形态路由（随关闭落盘，见 install_close_guard）：
        // 常态/小屏/竖屏各记各的槽位——竖屏条写竖屏槽、小窗写小窗槽，切
        // 形态谁也不覆盖谁。全屏/最大化/最小化中的几何归系统管，不进任何
        // 槽位（最小化时 bounds 可能变成 0 或屏外值）；塌缩过恢复下限的
        // 采样也不进槽位——过渡态会读出坏值，写进去会毒化该形态的记忆
        // （重进时被恢复判定拒掉回默认）。
        let system_managed = window.is_fullscreen()
            || window.is_maximized()
            || crate::platform::is_minimized(window);
        if !system_managed {
            // 记忆槽位的坐标空间由平台定义（macOS = AppKit 全局外框，Windows
            // = gpui 逻辑坐标）：gpui 在 macOS 的 bounds 是「相对本屏」的，
            // 存进去多屏必错位。
            let sampled = crate::platform::memory_rect(window)
                .map(|r| [r.x as f32, r.y as f32, r.w as f32, r.h as f32]);
            if let Some(sampled) = sampled {
                let (floor_w, floor_h) = if in_mini {
                    (
                        crate::window_restore::MINI_RESTORE_W,
                        crate::window_restore::MINI_RESTORE_H,
                    )
                } else if in_portrait {
                    (
                        crate::window_restore::PORTRAIT_RESTORE_W,
                        crate::window_restore::PORTRAIT_RESTORE_H,
                    )
                } else {
                    (
                        crate::window_restore::MIN_RESTORE_W,
                        crate::window_restore::MIN_RESTORE_H,
                    )
                };
                let sane = sampled[2] >= floor_w && sampled[3] >= floor_h;
                if in_mini {
                    if sane && sampled != self.mini_bounds {
                        self.mini_bounds = sampled;
                        self.bounds_dirty = true;
                    }
                } else if in_portrait {
                    // 铺满档是派生的满高几何，不进竖屏记忆槽（同
                    // `portrait::exit` 的守卫）：采样进来会让「上次竖屏大小」
                    // 变成满高，下次进竖屏铺满钮形同虚设。
                    if sane && !crate::portrait::is_full() && sampled != self.portrait_bounds {
                        self.portrait_bounds = sampled;
                        self.bounds_dirty = true;
                    }
                } else if !immersive && sane && sampled != self.win_bounds {
                    self.win_bounds = sampled;
                    self.bounds_dirty = true;
                }
            }
        }
        // 先建好所有吃 cx 的闭包/子渲染，再取 theme（theme 借 cx，两者不能
        // 交叠——闭包里要 &mut Context）
        let toolbar = (!chrome_hidden).then(|| self.render_toolbar(window, cx));
        let sidebar = (!chrome_hidden && !in_portrait).then(|| self.render_sidebar(cx));
        // 下载浮层（Tauri Sheet 固定整窗底边）：横贯侧栏，由壳层渲染
        let download_sheet = self
            .feed
            .update(cx, |view, cx| view.render_downloads_sheet(cx));
        let content = self.render_content(cx);
        let theme = cx.theme();
        // 系统后端 + 首页：壳根透明，舞台的透明洞才露得出下层视频视图；
        // 其他目的地照常铺主题底色（视频只在首页播放）。
        let video_hole = crate::video_backend::is_system()
            && self.dest == Dest::Home
            && !crate::preview::is_active();
        v_flex()
            .id("shell")
            .size_full()
            .bg(theme.background)
            .when(video_hole, |el| el.bg(gpui_kit::rgba(0x00000000)))
            .text_color(theme.foreground)
            .children(toolbar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    // 小屏/沉浸全屏时侧栏藏掉（视频区吃满，播放跟随窗口）
                    .children(sidebar)
                    .child(
                        // 内容区：吃掉剩余宽度；过载时只允许它收缩（coding-guides
                        // 「flex_1 仍可能拒绝收缩」），滚动归视图自己。
                        v_flex().flex_1().min_w_0().child(content),
                    ),
            )
            .children(download_sheet)
    }
}
