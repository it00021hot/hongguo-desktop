use super::mobile_preferences::Preferences;
use super::{DESKTOP_SECTIONS, Navigation, Route};
use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct MainMenu {
    nav: Navigation,
    open: Signal<bool>,
    origin: Stored<Option<Route>>,
    opener: Stored<Option<ElementRef>>,
    restore_focus: Stored<bool>,
}

impl MainMenu {
    pub fn provide(nav: Navigation) -> Self {
        let menu = Self {
            nav,
            open: signal(false),
            origin: stored(None),
            opener: stored(None),
            restore_focus: stored(true),
        };
        provide_context(menu);
        effect(move || {
            let current = nav.routes().with(|routes| routes.last().cloned());
            if menu.open.get() && current != menu.origin.get() {
                menu.restore_focus.replace(false);
                menu.open.set(false);
            }
        });
        menu
    }

    pub fn current() -> Option<Self> {
        use_context()
    }

    pub fn trigger(self) -> nsh_base::ButtonElement {
        let reference = element_ref();
        let extent = move || {
            px(if os() == nsh_core::platform::Os::Ios {
                44.0
            } else {
                48.0
            })
        };
        ui::controls::surface(ui::ButtonVariant::Quiet)
            .element_ref(&reference)
            .accessibility_label(move || tr("nav.mainMenu"))
            .selected(self.open)
            .size(extent)
            .min_h(extent)
            .p_0()
            .child(ui::icon(ui::Icon::Menu).size_5())
            .on_press(move || {
                if self.open.get_untracked() {
                    self.open.set(false);
                } else {
                    self.origin
                        .replace(self.nav.routes().with(|routes| routes.last().cloned()));
                    self.opener.replace(Some(reference));
                    self.restore_focus.replace(true);
                    self.open.set(true);
                }
            })
    }

    pub fn sheet(self, preferences: Preferences) -> nsh_base::SheetElement {
        ui::sheet(
            self.open,
            move || tr("nav.mainMenu"),
            move || {
                let navigation = scroll()
                    .test_id("mobile-menu-scroll")
                    .flex_1()
                    .min_h_0()
                    .keyboard_taps(KeyboardTaps::Always)
                    .child(
                        view().test_id("mobile-main-menu").gap_1().children(
                            DESKTOP_SECTIONS
                                .into_iter()
                                .map(move |(key, glyph, route)| {
                                    let selected = route.clone();
                                    let destination = route.clone();
                                    ui::controls::surface(ui::ButtonVariant::Quiet)
                                        .accessibility_label(move || tr(key))
                                        .w_full()
                                        .min_h(px(48.0))
                                        .px_3()
                                        .py_2()
                                        .flex_row()
                                        .items_center()
                                        .justify_start()
                                        .gap_3()
                                        .selected(move || self.nav.is_selected(&selected))
                                        .child(ui::icon(glyph).size_5())
                                        .child(
                                            text(move || tr(key))
                                                .text_base()
                                                .font_weight(FontWeight::NORMAL),
                                        )
                                        .on_press(move || {
                                            self.restore_focus.replace(false);
                                            self.open.set(false);
                                            self.nav.push(destination.clone());
                                        })
                                }),
                        ),
                    );
                view()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(navigation)
                    .child(preferences.footer())
            },
        )
        .on_close(move || {
            if self.restore_focus.get()
                && let Some(reference) = self.opener.get()
            {
                after_answer(move || reference.focus());
            }
        })
    }
}
