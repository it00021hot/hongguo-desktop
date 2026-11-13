use super::*;
use nsh_core::time::Clock;
use nsh_core::{EventKind, EventPayload, PayloadValue};
use nsh_engine::{EngineConfig, Environment, Root, Size};
use nsh_testing::Headless;
use std::time::{Duration, SystemTime};

#[component]
fn Draft() -> impl IntoElement {
    let draft = signal("A[微笑]B".to_owned());
    let editor = Editor::new(draft, 40);
    let sent = signal(String::new());
    view()
        .w(px(500.0))
        .h(px(300.0))
        .child(
            view()
                .test_id("rich-field")
                .child(editor.field(FieldOptions {
                    placeholder: computed(|| "Draft".into()),
                    disabled: computed(|| false),
                    multiline: true,
                    skin: FieldSkin::Review,
                    submit: Box::new(move || sent.set(editor.text.get_untracked())),
                    on_tab: Box::new(|_| {}),
                })),
        )
        .child(text(move || format!("draft:{:?}", editor.text.get())))
        .child(text(move || format!("sent:{:?}", sent.get())))
        .child(
            crate::ui::button("Insert smile", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    editor.insert("[偷笑]").unwrap();
                },
            ),
        )
        .child(
            crate::ui::button("Clear external", crate::ui::ButtonVariant::Quiet)
                .on_press(move || draft.set(String::new())),
        )
}

fn start() -> Headless {
    Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(600.0), px(400.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<Draft>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
        )
    })
    .unwrap()
}

fn assert_draft(app: &mut Headless, value: &str) {
    app.wait_for(&format!("text:draft:{value:?}")).unwrap();
}
fn focus(app: &mut Headless) {
    app.type_text("id:rich-field", "").unwrap();
}
fn compose(app: &mut Headless, composing: bool, text: &str) {
    // Public engine events, following NSH's own RichEditor IME regression harness.
    for id in 1..1024 {
        app.engine().control().dispatch_event(
            id,
            EventKind::CompositionChange,
            EventPayload::Map(vec![
                ("composing", PayloadValue::Bool(composing)),
                ("text", PayloadValue::Str(text.to_owned())),
            ]),
        );
    }
    app.run_for(Duration::from_millis(50)).unwrap();
}

#[test]
fn actual_image_atoms_delete_as_one_unit_and_submission_keeps_wire_codes() {
    let mut app = start();
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    focus(&mut app);
    app.press_key("end").unwrap();
    app.press_key("left").unwrap();
    app.press_key("backspace").unwrap();
    assert_draft(&mut app, "AB");
    app.press("Insert smile").unwrap();
    assert_draft(&mut app, "A[偷笑]B");
    app.press_key("enter").unwrap();
    app.wait_for("text:sent:\"A[偷笑]B\"").unwrap();
}

#[test]
fn pasted_newlines_and_external_clear_preserve_the_single_draft_owner() {
    let mut app = start();
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    app.press("Clear external").unwrap();
    assert_draft(&mut app, "");
    let mut paste = nsh_core::PasteEvent::default();
    paste.text = "第一行[微笑]\n第二行\n".into();
    app.paste("id:rich-field", &paste).unwrap();
    assert_draft(&mut app, "第一行[微笑]\n第二行\n");
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    app.press("Clear external").unwrap();
    assert_draft(&mut app, "");
    assert!(
        !app.screen()
            .matches("id:rich-field >> label:[微笑]")
            .unwrap()
    );
}

#[test]
fn chinese_ime_composition_is_not_submitted_before_the_commit() {
    let mut app = start();
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    focus(&mut app);
    app.press_key("end").unwrap();
    compose(&mut app, true, "ni");
    app.press_key("enter").unwrap();
    app.wait_for("text:sent:\"\"").unwrap();
    assert_draft(&mut app, "A[微笑]B");
    app.type_text("id:rich-field", "你").unwrap();
    compose(&mut app, false, "");
    assert_draft(&mut app, "A[微笑]B你");
    app.press_key("enter").unwrap();
    app.wait_for("text:sent:\"A[微笑]B你\"").unwrap();
}

#[test]
fn overlong_paste_is_rejected_without_changing_existing_atoms() {
    let mut app = start();
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    focus(&mut app);
    app.press_key("end").unwrap();
    let mut paste = nsh_core::PasteEvent::default();
    paste.text = "你".repeat(40);
    app.paste("id:rich-field", &paste).unwrap();
    assert_draft(&mut app, "A[微笑]B");
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
}

#[test]
fn pasted_mentions_delete_atomically_and_keep_the_original_wire_identity() {
    let mut app = start();
    app.wait_for("id:rich-field >> label:[微笑]").unwrap();
    app.press("Clear external").unwrap();
    let mut paste = nsh_core::PasteEvent::default();
    paste.text = "你好 @阿明\u{fffc}".into();
    app.paste("id:rich-field", &paste).unwrap();
    assert_draft(&mut app, "你好 @阿明\u{fffc}");
    app.press_key("end").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for(&format!("text:sent:{:?}", "你好 @阿明\u{fffc}"))
        .unwrap();
    app.press_key("backspace").unwrap();
    assert_draft(&mut app, "你好 ");
}

#[component]
fn Lifetime() -> impl IntoElement {
    let visible = signal(true);
    view()
        .w(px(600.0))
        .h(px(400.0))
        .child(
            crate::ui::button("Toggle draft owner", crate::ui::ButtonVariant::Quiet)
                .on_press(move || visible.set(!visible.get_untracked())),
        )
        .child(when(move || visible.get(), draft))
}

#[test]
fn disposing_and_remounting_the_draft_owner_does_not_read_disposed_editor_state() {
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(600.0), px(400.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<Lifetime>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    for _ in 0..3 {
        app.wait_for("id:rich-field >> label:[微笑]").unwrap();
        focus(&mut app);
        app.press_key("enter").unwrap();
        app.wait_for("text:sent:\"A[微笑]B\"").unwrap();
        app.press("Toggle draft owner").unwrap();
        assert!(!app.screen().matches("id:rich-field").unwrap());
        app.press("Toggle draft owner").unwrap();
    }
}

#[component]
fn LongLine() -> impl IntoElement {
    let editor = Editor::new(signal(String::new()), 1000);
    let geometry = signal(String::new());
    view()
        .w(px(300.0))
        .h(px(200.0))
        .child(
            view()
                .w(px(180.0))
                .test_id("long-field")
                .child(editor.field(FieldOptions {
                    placeholder: computed(|| "Long line".into()),
                    disabled: computed(|| false),
                    multiline: false,
                    skin: FieldSkin::Comment,
                    submit: Box::new(|| {}),
                    on_tab: Box::new(|_| {}),
                })),
        )
        .child(
            crate::ui::button("Inspect caret geometry", crate::ui::ButtonVariant::Quiet).on_press(
                move || {
                    let session = editor.session();
                    let state = session.state();
                    let first = session.coords_at_pos(1).unwrap();
                    let last = session
                        .coords_at_pos(state.doc().content().size() - 1)
                        .unwrap();
                    geometry.set(format!(
                        "same-line:{} long:{}",
                        first.origin.y == last.origin.y,
                        last.origin.x > px(180.0)
                    ));
                },
            ),
        )
        .child(text(move || geometry.get()))
}

#[test]
fn a_long_chinese_and_emoji_draft_has_real_horizontal_caret_reveal_without_wrapping() {
    use std::sync::{Arc, Mutex};
    let mut app = Headless::start(EngineConfig {
        environment: Environment {
            os: nsh_core::platform::Os::MacOs,
            ..Default::default()
        },
        viewport: Size::new(px(600.0), px(400.0)),
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(
            Root::of::<LongLine>()
                .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap()),
        )
    })
    .unwrap();
    app.wait_for("id:long-field").unwrap();
    let scrolls = Arc::new(Mutex::new(Vec::new()));
    let observed = scrolls.clone();
    app.observe(move |_, batch| {
        for item in &batch.items {
            if let nsh_core::MountItem::Command {
                command: nsh_core::Command::ScrollTo { x, y, .. },
                ..
            } = item
            {
                observed.lock().unwrap().push((*x, *y));
            }
        }
    });
    app.type_text("id:long-field", &"中文[微笑]".repeat(40))
        .unwrap();
    app.press_key("end").unwrap();
    app.run_for(Duration::from_millis(100)).unwrap();
    assert!(
        scrolls
            .lock()
            .unwrap()
            .iter()
            .any(|(x, y)| *x > px(100.0) && *y == px(0.0)),
        "end caret must move the horizontal viewport: {:?}",
        scrolls.lock().unwrap()
    );
    app.press_key("home").unwrap();
    app.run_for(Duration::from_millis(100)).unwrap();
    assert_eq!(scrolls.lock().unwrap().last().unwrap().0, px(0.0));
    app.press("Inspect caret geometry").unwrap();
    app.wait_for("text:same-line:true long:true").unwrap();
}
