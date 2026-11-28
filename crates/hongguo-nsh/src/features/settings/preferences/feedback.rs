use crate::{features::settings::presentation, i18n::tr, ui::ButtonVariant};
use nsh_core::prelude::*;

const NEW_ISSUE_BASE: &str =
    "https://github.com/it00021hot/hongguo-desktop/issues/new?template=bug_report.yml";

pub(super) fn section(status: Signal<SharedString>) -> View {
    let owner = Owner::current();
    let copy_logs = move || {
        let excerpt = crate::logging::read_diagnostic_excerpt();
        owner.run(move || {
            spawn(async move {
                if !matches!(set_clipboard(excerpt).await, ClipboardWrite::Failed) {
                    status.set(tr("feedback.copied"));
                } else {
                    status.set(tr("feedback.copyFailed"));
                }
            });
        });
    };
    let report_issue = move || {
        let excerpt = crate::logging::read_diagnostic_excerpt();
        let url = issue_url();
        owner.run(move || {
            spawn(async move {
                let copied = !matches!(set_clipboard(excerpt).await, ClipboardWrite::Failed);
                let opened = open_url(&url).await;
                status.set(match (opened, copied) {
                    (true, true) => tr("feedback.issueOpenedWithLogs"),
                    (true, false) => tr("feedback.issueOpened"),
                    (false, _) => tr("feedback.issueOpenFailed"),
                });
            });
        });
    };
    let open_log_dir = move || {
        let app_log = crate::logging::app_log_path();
        let target = if app_log.exists() {
            app_log
        } else {
            hongguo_store::paths::data_dir()
        };
        owner.run(move || {
            spawn(async move {
                if let Err(error) = reveal_path(target).await {
                    status.set(error.into());
                }
            });
        });
    };
    presentation::form_surface(
        view()
            .gap_3()
            .child(
                text(move || tr("feedback.title"))
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(presentation::form_hint(move || tr("feedback.hint")))
            .child(presentation::form_description(move || {
                tr("feedback.logPath").replace(
                    "{path}",
                    &crate::logging::app_log_path().display().to_string(),
                )
            }))
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        presentation::form_action(
                            move || tr("feedback.reportIssue"),
                            ButtonVariant::Primary,
                        )
                        .on_press(report_issue),
                    )
                    .child(
                        presentation::form_action(
                            move || tr("feedback.copyLogs"),
                            ButtonVariant::Quiet,
                        )
                        .on_press(copy_logs),
                    )
                    .child(when(is_desktop, move || {
                        presentation::form_action(
                            move || tr("feedback.openLogFolder"),
                            ButtonVariant::Quiet,
                        )
                        .on_press(open_log_dir)
                    })),
            ),
    )
}

fn issue_url() -> String {
    let version = percent_encode(&crate::metadata().version);
    let os = percent_encode(&format!(
        "{} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    format!("{NEW_ISSUE_BASE}&version={version}&os={os}")
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}
