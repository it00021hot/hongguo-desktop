//! 轻量导航：侧栏四页 + 首页分类 tab 的共享状态。
//!
//! 播放**不是独立页面**（与 Tauri 壳一致）：首页就是沉浸式播放器，选中哪部剧
//! 就在首页那一路 `video` 上换源。所以这里只有四个侧栏页，没有 Player。

use nsh_core::ui::prelude::*;

/// 一个可导航的页面（对齐 Tauri 侧栏顺序：推荐 / 排行榜 / 找剧 / 设置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// 推荐（首页沉浸流）。
    Home,
    /// 排行榜。
    Rank,
    /// 找剧。
    Browse,
    /// 设置。
    Settings,
}

impl Page {
    /// 侧栏顺序（冒烟脚本的点击坐标按这个顺序排）。
    pub const ALL: [Self; 4] = [Self::Home, Self::Rank, Self::Browse, Self::Settings];

    /// 页面标题的 i18n key（复用 Tauri 壳既有词条，不新造）。
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Home => "nav.home.title",
            Self::Rank => "nav.rank.title",
            Self::Browse => "nav.browse.title",
            Self::Settings => "nav.settings.title",
        }
    }
}

/// 首页分类 tab 的共享状态：顶栏画 tab，首页按选中的 tab 重新拉流。
#[derive(Clone, Copy)]
pub struct FeedNav {
    /// 当前 tab 的 tab_type（"16" 推荐 / "36" 漫剧 / "39" 真人）。
    tab: Signal<String>,
}

impl FeedNav {
    /// 推荐 tab 起步。
    pub fn new() -> Self {
        Self {
            tab: signal(crate::player::TAB_FEED.to_owned()),
        }
    }

    /// 从 context 取（`App` 在根部 `provide_context`）。
    pub fn from_context() -> Self {
        use_context::<Self>().expect("FeedNav 由 App 在根部 provide_context")
    }

    /// 当前选中（读信号，视图里直接读它）。
    pub fn current(&self) -> Signal<String> {
        self.tab
    }

    /// 切 tab。
    pub fn pick(&self, tab: &str) {
        if self.tab.with(|now| now != tab) {
            log::info!("[Home] 切分类 tab {tab}");
            self.tab.set(tab.to_owned());
        }
    }
}

impl Default for FeedNav {
    fn default() -> Self {
        Self::new()
    }
}

/// 小窗播放态（Tauri `miniScreen` 同款）：壳藏掉侧栏/顶栏，播放页自己给
/// 一个「退出小屏」入口。
#[derive(Clone, Copy)]
pub struct MiniMode {
    on: Signal<bool>,
}

impl MiniMode {
    /// 常态起步。
    pub fn new() -> Self {
        Self { on: signal(false) }
    }

    /// 从 context 取（`App` 在根部 `provide_context`）。
    pub fn from_context() -> Self {
        use_context::<Self>().expect("MiniMode 由 App 在根部 provide_context")
    }

    /// 当前是否小窗。
    pub fn get(&self) -> bool {
        self.on.get()
    }

    /// 切形态（同时改窗口几何，几何在平台层）。
    pub fn set(&self, mini: bool) {
        self.on.set(mini);
        if mini {
            crate::platform_ext::enter_mini();
        } else {
            crate::platform_ext::exit_mini();
        }
    }
}

impl Default for MiniMode {
    fn default() -> Self {
        Self::new()
    }
}

/// 导航（`Copy` 句柄，可随意捕获进闭包与任务）。
#[derive(Clone, Copy)]
pub struct Nav {
    stack: Signal<Vec<Page>>,
}

impl Nav {
    /// 新栈，栈顶是首页。
    pub fn new() -> Self {
        Self {
            stack: signal(vec![Page::Home]),
        }
    }

    /// 栈顶页（视图里读它决定渲染哪一页）。
    pub fn current(&self) -> Page {
        self.stack
            .with(|stack| *stack.last().unwrap_or(&Page::Home))
    }

    /// 切侧栏页：栈重置成「首页 + 目标页」两级，保证返回键回得到首页。
    pub fn goto(&self, page: Page) {
        log::info!("[Nav] 切页 {page:?}");
        self.stack.set(vec![Page::Home, page]);
    }

    /// 弹回上一页；已在栈底则无操作。
    pub fn pop(&self) {
        self.stack.update(|stack| {
            if stack.len() > 1 {
                stack.pop();
            }
        });
    }
}

impl Default for Nav {
    fn default() -> Self {
        Self::new()
    }
}
