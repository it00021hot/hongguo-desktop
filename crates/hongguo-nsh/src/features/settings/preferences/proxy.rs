use super::{
    controls::{choices, group},
    model::Draft,
};
use crate::i18n::tr;
use hongguo_common::model::settings::ProxyMode;
use nsh_core::prelude::*;

pub(super) fn section(
    draft: Signal<Draft>,
    busy: Signal<bool>,
    message: Signal<SharedString>,
) -> View {
    group("settings.proxy").child(
        view()
            .gap_3()
            .child(choices(
                "settings.proxy",
                draft,
                busy,
                vec![
                    (ProxyMode::System, "system", "settings.proxySystem"),
                    (ProxyMode::Manual, "manual", "settings.proxyManual"),
                    (ProxyMode::Direct, "direct", "settings.proxyDirect"),
                ],
                |draft| draft.proxy.mode,
                |draft, value| draft.proxy.mode = value,
            ))
            .child(when(
                move || draft.with(|draft| draft.proxy.mode == ProxyMode::Manual),
                move || {
                    crate::features::settings::presentation::directory_field(move || {
                        tr("nsh.proxyAddress")
                    })
                    .value(move || draft.with(|draft| draft.proxy.url.clone()))
                    .disabled(busy)
                    .auto_correct(false)
                    .on_change(move |value: &str| {
                        draft.update(|draft| draft.proxy.url = value.to_owned())
                    })
                },
            ))
            .child(
                crate::features::settings::presentation::form_icon_action(
                    move || tr("settings.testProxy"),
                    crate::features::settings::presentation::FormAction::TestProxy,
                )
                .self_start()
                .disabled(busy)
                .on_press(move || {
                    let proxy = draft.with_untracked(|draft| draft.proxy.clone());
                    busy.set(true);
                    message.set(tr("settings.testing"));
                    spawn(async move {
                        let result = crate::bridge::request(async move {
                            hongguo_service::settings_service::proxy::test_proxy(&proxy).await
                        })
                        .await;
                        message.set(match result {
                            Ok(result) if result.ok => tr("settings.testOk")
                                .replace("{ms}", &result.elapsed_ms.to_string())
                                .into(),
                            Ok(result) => tr("settings.testFail")
                                .replace("{message}", &result.message)
                                .into(),
                            Err(error) => error.text(),
                        });
                        busy.set(false);
                    });
                }),
            ),
    )
}
