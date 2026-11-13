use crate::{
    i18n::{Language, tr},
    ui,
};
use nsh_base::theme;
use nsh_core::{platform::Appearance, prelude::*};

#[derive(Clone, Copy)]
pub(super) struct Preferences {
    busy: Memo<bool>,
    value: Memo<String>,
    change: Stored<Box<dyn Fn(Appearance)>>,
    language: Language,
}

impl Preferences {
    pub fn new(
        busy: impl IntoReactive<bool>,
        value: impl Fn() -> String + 'static,
        change: impl Fn(Appearance) + 'static,
        language: Language,
    ) -> Self {
        let busy = busy.into_reactive();
        Self {
            busy: computed(move || busy.get()),
            value: computed(value),
            change: stored(Box::new(change)),
            language,
        }
    }

    pub fn footer(self) -> View {
        view()
            .test_id("mobile-menu-preferences")
            .flex_shrink_0()
            .pt_3()
            .border_t_1()
            .border_color(move || theme().border)
            .gap_2()
            .child(
                row("settings.appearance", move || {
                    if dark() {
                        ui::Icon::Moon
                    } else {
                        ui::Icon::Sun
                    }
                })
                .child(
                    ui::select(
                        move || tr("common.toggleTheme"),
                        || {
                            ui::toolbar_menu::THEMES
                                .iter()
                                .map(|&(value, key)| ui::SelectOption::new(value, tr(key)))
                                .collect::<Vec<_>>()
                        },
                        move || Some(SharedString::from(self.value.get())),
                    )
                    .flex_1()
                    .min_w_32()
                    .disabled(self.busy)
                    .on_change(move |value: &Option<SharedString>| {
                        let next = match value.as_deref() {
                            Some("auto") => Appearance::System,
                            Some("light") => Appearance::Light,
                            Some("dark") => Appearance::Dark,
                            _ => return,
                        };
                        self.change.with(|change| change(next));
                    }),
                ),
            )
            .child(
                row("search.langLabel", || ui::Icon::Languages).child(
                    ui::select(
                        move || tr("search.langLabel"),
                        || {
                            ui::toolbar_menu::LANGUAGES
                                .iter()
                                .map(|&(value, key)| ui::SelectOption::new(value, tr(key)))
                                .collect::<Vec<_>>()
                        },
                        move || Some(SharedString::from(self.language.value())),
                    )
                    .flex_1()
                    .min_w_32()
                    .on_change(move |value: &Option<SharedString>| {
                        let next = match value.as_deref() {
                            Some("zh-CN") => "zh-CN",
                            Some("en-US") => "en-US",
                            _ => return,
                        };
                        ui::toolbar_menu::change_language(self.language, next);
                    }),
                ),
            )
    }
}

fn row(label: &'static str, glyph: impl Fn() -> ui::Icon + 'static) -> View {
    view()
        .w_full()
        .min_h(px(48.0))
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(dynamic(move || ui::icon(glyph()).size_5()))
                .child(
                    text(move || tr(label))
                        .text_base()
                        .font_weight(FontWeight::NORMAL),
                ),
        )
}
