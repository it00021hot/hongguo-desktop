use crate::i18n::tr;
use nsh_core::{
    element::{
        ImageSrc,
        desktop::{self, MenuItem},
    },
    prelude::*,
};

pub(super) fn install(exit_requested: Signal<bool>) {
    effect(move || {
        let window = desktop::main_window();
        let show = window.clone();
        let exit = window.clone();
        desktop::set_tray(Some(
            desktop::tray()
                .icon(ImageSrc::svg_markup(include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/icons/tray.svg"
                ))))
                .tooltip(tr("tray.name").to_string())
                .menu([
                    MenuItem::new(tr("tray.showWindow").to_string())
                        .on_select(move || show.focus()),
                    MenuItem::separator(),
                    MenuItem::new(tr("window.exitConfirm").to_string()).on_select(move || {
                        exit.focus();
                        exit_requested.set(true);
                    }),
                ])
                .on_click(move |_| window.focus()),
        ));
    });
}
