//! 找剧页筛选：面板行标签、`BrowseFilters` 读写、筛选面板与行内胶囊。

use super::*;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::{Role, Toggled};

/// 面板行类型 → 行头标签（Tauri FILTER_LABEL_KEYS 同款；未知类型回落
/// 服务端行名去掉「全部」前缀）。
fn row_label(row_type: &str, row_name: &str) -> String {
    match row_type {
        "genre" => crate::i18n::t("browse.fGenre"),
        "category_dim_theme" => crate::i18n::t("browse.fTheme"),
        "category_dim_role" => crate::i18n::t("browse.fRole"),
        "category_dim_epoch" => crate::i18n::t("browse.fEpoch"),
        "sort" => crate::i18n::t("browse.fSort"),
        "gender" => crate::i18n::t("browse.fGender"),
        "online_time" => crate::i18n::t("browse.fOnlineTime"),
        "duration" => crate::i18n::t("browse.fDuration"),
        other => {
            let stripped = row_name.trim_start_matches("全部");
            if stripped.is_empty() {
                other.to_string()
            } else {
                stripped.to_string()
            }
        }
    }
}

/// 行类型 → `BrowseFilters` 的当前值。
fn filter_get(filters: &BrowseFilters, row_type: &str) -> String {
    match row_type {
        "genre" => filters.genre.clone(),
        "category_dim_theme" => filters.theme.clone(),
        "category_dim_role" => filters.role.clone(),
        "category_dim_epoch" => filters.epoch.clone(),
        "sort" => filters.sort.clone(),
        "gender" => filters.gender.clone(),
        "online_time" => filters.online_time.clone(),
        "duration" => filters.duration.clone(),
        _ => String::new(),
    }
}

/// 行类型 → `BrowseFilters` 的选中值写入。
fn filter_set(filters: &mut BrowseFilters, row_type: &str, value: String) {
    match row_type {
        "genre" => filters.genre = value,
        "category_dim_theme" => filters.theme = value,
        "category_dim_role" => filters.role = value,
        "category_dim_epoch" => filters.epoch = value,
        "sort" => filters.sort = value,
        "gender" => filters.gender = value,
        "online_time" => filters.online_time = value,
        "duration" => filters.duration = value,
        _ => {}
    }
}

impl super::BrowseView {
    pub(super) fn active_filter_count(&self) -> usize {
        [
            &self.filters.genre,
            &self.filters.theme,
            &self.filters.role,
            &self.filters.epoch,
            &self.filters.sort,
            &self.filters.gender,
            &self.filters.online_time,
            &self.filters.duration,
            &self.filters.creation_status,
        ]
        .iter()
        .filter(|v| !v.is_empty())
        .count()
    }

    pub(super) fn render_filter_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let theme = cx.theme();
        let rows = self.panel_rows.as_ref()?;
        let mut elements: Vec<gpui_kit::AnyElement> = Vec::new();
        for row in rows {
            let current = filter_get(&self.filters, &row.row_type);
            let mut pills: Vec<gpui_kit::AnyElement> = vec![pill(
                cx,
                ElementId::from(format!("browse-all-{}", row.row_type)),
                crate::i18n::t("browse.all"),
                current.is_empty(),
                {
                    let row_type = row.row_type.clone();
                    cx.listener(move |this, _, _, cx| {
                        if filter_get(&this.filters, &row_type).is_empty() {
                            return;
                        }
                        filter_set(&mut this.filters, &row_type, String::new());
                        this.reload_browse(cx);
                    })
                },
            )];
            for item in &row.items {
                let selected = current == item.id;
                let row_type = row.row_type.clone();
                let id = item.id.clone();
                pills.push(pill(
                    cx,
                    ElementId::from(format!("browse-{}-{}", row.row_type, item.id)),
                    item.name.clone(),
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        if filter_get(&this.filters, &row_type) == id {
                            return;
                        }
                        filter_set(&mut this.filters, &row_type, id.clone());
                        this.reload_browse(cx);
                    }),
                ));
            }
            elements.push(
                h_flex()
                    .items_start()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .w(gpui_kit::px(40.0))
                            .flex_shrink_0()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(row_label(&row.row_type, &row.row_name))),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .py_0p5()
                            .id(ElementId::from(format!("browse-group-{}", row.row_type)))
                            .role(Role::RadioGroup)
                            .children(pills),
                    )
                    .into_any_element(),
            );
        }
        // 客户端合成「状态」行（服务端 rows 不含；值走请求体顶层）
        {
            let current = self.filters.creation_status.clone();
            let mut pills: Vec<gpui_kit::AnyElement> = vec![pill(
                cx,
                "browse-all-creation".into(),
                crate::i18n::t("browse.all"),
                current.is_empty(),
                cx.listener(|this, _, _, cx| {
                    if this.filters.creation_status.is_empty() {
                        return;
                    }
                    this.filters.creation_status.clear();
                    this.reload_browse(cx);
                }),
            )];
            for (id, label) in [
                (
                    "creation_status_0",
                    crate::i18n::t("browse.creationFinished"),
                ),
                (
                    "creation_status_1",
                    crate::i18n::t("browse.creationOngoing"),
                ),
            ] {
                let selected = current == id;
                let id_owned = id.to_string();
                pills.push(pill(
                    cx,
                    ElementId::from(format!("browse-creation-{id}")),
                    label,
                    selected,
                    cx.listener(move |this, _, _, cx| {
                        if this.filters.creation_status == id_owned {
                            return;
                        }
                        this.filters.creation_status = id_owned.clone();
                        this.reload_browse(cx);
                    }),
                ));
            }
            elements.push(
                h_flex()
                    .items_start()
                    .gap_3()
                    .text_sm()
                    .child(
                        div()
                            .w(gpui_kit::px(40.0))
                            .flex_shrink_0()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::i18n::t("browse.fCreation")),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .py_0p5()
                            .id("browse-group-creation")
                            .role(Role::RadioGroup)
                            .children(pills),
                    )
                    .into_any_element(),
            );
        }
        Some(
            v_flex()
                .w_full()
                .gap_1p5()
                .children(elements)
                .into_any_element(),
        )
    }
}

/// 行内筛选胶囊（Tauri ToggleGroupItem 形态：圆角全、选中主色描边）。
///
/// 载体是 gpui-kit 的 base Button 而不是 div：div 不带焦点句柄，键盘用户
/// 根本够不到这些筛选项；base Button 自带焦点句柄与 Enter/Space 激活。
fn pill(
    cx: &App,
    id: ElementId,
    label: String,
    selected: bool,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let label = SharedString::from(label);
    BaseButton::new(id)
        .role(Role::RadioButton)
        .aria_toggled(if selected {
            Toggled::True
        } else {
            Toggled::False
        })
        .accessibility_label(label.clone())
        .rounded_full()
        .border_1()
        .when(selected, |s| {
            s.border_color(theme.primary)
                .bg(theme.secondary)
                .text_color(theme.primary)
                .font_weight(gpui_kit::FontWeight::MEDIUM)
        })
        .when(!selected, |s| {
            s.border_color(theme.border)
                .text_color(theme.muted_foreground)
        })
        .px_3()
        .py_0p5()
        .text_xs()
        .cursor_pointer()
        // base Button 固定 justify_center，这里改回左对齐，和原来的 div 一致。
        .justify_start()
        // base Button 强制 line_height(relative(1.))，文字行盒会比默认矮一截、
        // 胶囊整体变矮；钉回窗口默认行高（黄金比例）才与原来的 div 像素一致。
        .line_height(gpui_kit::phi())
        // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
        .focus_visible(|s| {
            s.border_color(theme.ring)
                .shadow(crate::tokens::focus_ring(theme))
        })
        .hover(|s| s.bg(theme.secondary))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}
