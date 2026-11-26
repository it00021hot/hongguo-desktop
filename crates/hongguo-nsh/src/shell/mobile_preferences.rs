use crate::{i18n::tr, ui};
use nsh_base::theme;
use nsh_core::{platform::Appearance, prelude::*};

#[derive(Clone, Copy)]
pub(crate) struct Preferences {
    busy: Memo<bool>,
    value: Memo<String>,
    change: Stored<Box<dyn Fn(Appearance)>>,
}

impl Preferences {
    pub(super) fn theme_button() -> View {
        let preferences = use_context::<Self>();
        view().child(when(
            move || preferences.is_some(),
            move || {
                let preferences = preferences.expect("mobile shell provides preferences");
                ui::controls::surface(ui::ButtonVariant::Quiet)
                    .accessibility_label(move || tr("common.toggleTheme"))
                    .size_12()
                    .p_0()
                    .disabled(preferences.busy)
                    .child(dynamic(move || {
                        ui::icon(if dark() {
                            ui::Icon::Sun
                        } else {
                            ui::Icon::Moon
                        })
                    }))
                    .on_press(move || {
                        preferences.change.with(|change| {
                            change(if dark() {
                                Appearance::Light
                            } else {
                                Appearance::Dark
                            })
                        })
                    })
            },
        ))
    }

    pub fn new(
        busy: impl IntoReactive<bool>,
        value: impl Fn() -> String + 'static,
        change: impl Fn(Appearance) + 'static,
    ) -> Self {
        let busy = busy.into_reactive();
        Self {
            busy: computed(move || busy.get()),
            value: computed(value),
            change: stored(Box::new(change)),
        }
    }

    pub fn footer(self) -> View {
        view()
            .flex_shrink_0()
            .pt_3()
            .border_t_1()
            .border_color(move || theme().border)
            .gap_2()
            .child(
                row("settings.appearance", move || {
                    match self.value.get().as_str() {
                        "auto" => ui::Icon::SunMoon,
                        "dark" => ui::Icon::Moon,
                        "light" => ui::Icon::Sun,
                        _ if dark() => ui::Icon::Moon,
                        _ => ui::Icon::Sun,
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
                        move || Some(SharedString::from(crate::i18n::language())),
                    )
                    .flex_1()
                    .min_w_32()
                    .on_change(move |value: &Option<SharedString>| {
                        let next = match value.as_deref() {
                            Some("zh-CN") => "zh-CN",
                            Some("en-US") => "en-US",
                            _ => return,
                        };
                        ui::toolbar_menu::change_language(next);
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
