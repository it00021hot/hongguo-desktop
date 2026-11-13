use nsh_base::rich_editor::{
    extensions::history::{HistoryOptions, history},
    state::{EditorState, EditorStateConfig},
    view::{EditorSession, NodeViews},
};
use nsh_core::prelude::*;
use std::{
    rc::Rc,
    sync::{Arc, atomic::Ordering},
};
#[path = "rich_document.rs"]
mod document;
#[path = "rich_field.rs"]
mod field;
#[path = "rich_plugin.rs"]
mod plugin;
#[cfg(test)]
#[path = "rich_tests.rs"]
mod rich_tests;
pub(crate) use crate::ui::EmojiEditorSkin as FieldSkin;

pub(crate) struct FieldOptions {
    pub placeholder: Memo<SharedString>,
    pub disabled: Memo<bool>,
    /// Controls the field's layout; pasted/external hard breaks remain lossless.
    pub multiline: bool,
    pub skin: FieldSkin,
    pub submit: Box<dyn Fn()>,
    /// The composer's actual adjacent focus stops; `true` means Shift+Tab.
    pub on_tab: Box<dyn Fn(bool)>,
}

struct Handlers {
    submit: Rc<dyn Fn()>,
    on_tab: Rc<dyn Fn(bool)>,
    reveal: Rc<dyn Fn()>,
}

struct Inner {
    session: EditorSession,
    controls: Arc<plugin::Controls>,
    handler: Option<(u64, Rc<Handlers>)>,
    next_binding: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Editor {
    pub text: Signal<String>,
    pub selection: Signal<Option<Selection>>,
    pub reference: ElementRef,
    pub maximum: u32,
    inner: Stored<Inner>,
}

impl Editor {
    pub fn new(text: Signal<String>, maximum: u32) -> Self {
        let controls = Arc::new(plugin::Controls::default());
        let (submit, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let schema = document::schema();
        let doc = document::document(&schema, &text.get_untracked());
        let selection = document::document_selection(
            &doc,
            Selection {
                start: text.get_untracked().encode_utf16().count() as u32,
                end: text.get_untracked().encode_utf16().count() as u32,
            },
        );
        let state = EditorState::create(
            EditorStateConfig::new(schema)
                .doc(doc)
                .selection(selection)
                .plugins(vec![
                    Arc::new(plugin::Editing {
                        maximum,
                        controls: controls.clone(),
                        submit,
                    }),
                    history(HistoryOptions::default()),
                ]),
        )
        .expect("valid initial emoji editor state");
        let editor = Self {
            text,
            selection: signal(None),
            reference: element_ref(),
            maximum,
            inner: stored(Inner {
                session: EditorSession::new(state, NodeViews::new()),
                controls,
                handler: None,
                next_binding: 0,
            }),
        };
        effect(move || {
            let external = text.get();
            let session = editor.session();
            if !session.composing()
                && document::wire(session.state().doc()) != external
                && let Err(error) = editor.replace(&external, None)
            {
                log::error!("[NSH editor] external document update rejected: {error}");
            }
        });
        spawn(async move {
            while let Some((binding, action)) = receiver.recv().await {
                let handler = editor.inner.with(|inner| {
                    if inner.controls.disabled.load(Ordering::Acquire) {
                        return None;
                    }
                    inner
                        .handler
                        .as_ref()
                        .filter(|(id, _)| *id == binding)
                        .map(|(_, handler)| handler.clone())
                });
                if let Some(handler) = handler {
                    editor.sync();
                    match action {
                        plugin::Action::Submit => (handler.submit)(),
                        plugin::Action::Tab(back) => (handler.on_tab)(back),
                        plugin::Action::Reveal => (handler.reveal)(),
                    }
                }
            }
        });
        editor
    }

    fn session(self) -> EditorSession {
        self.inner.with(|inner| inner.session.clone())
    }

    fn sync(self) {
        let state = self.session().state();
        self.text.set(document::wire(state.doc()));
        self.selection.set(Some(document::wire_selection(
            state.doc(),
            state.selection(),
        )));
    }

    fn replace(self, wire: &str, caret: Option<u32>) -> Result<(), String> {
        let session = self.session();
        let state = session.state();
        let doc = document::document(state.schema(), wire);
        let at = caret.unwrap_or_else(|| wire.encode_utf16().count() as u32);
        let mut tr = state.tr();
        tr.replace_with(0, state.doc().content().size(), doc.content().clone())
            .map_err(|error| error.to_string())?;
        tr.set_selection(document::document_selection(
            tr.doc(),
            Selection { start: at, end: at },
        ));
        session.dispatch(tr).map_err(|error| error.to_string())
    }

    pub fn insert(self, code: &str) -> Result<(), SharedString> {
        let Some((next, caret)) = insert(
            &self.text.get_untracked(),
            self.selection.get_untracked(),
            code,
            self.maximum,
        ) else {
            return Err(crate::i18n::tr("player.emoticon.tooLong")
                .replace("{n}", &self.maximum.to_string())
                .into());
        };
        self.replace(&next, Some(caret)).map_err(|error| {
            log::error!("[NSH editor] insertion rejected: {error}");
            crate::i18n::tr("error.internal")
        })?;
        self.sync();
        self.reference.focus();
        Ok(())
    }
    pub fn field(self, options: FieldOptions) -> View {
        field::field(self, options)
    }
}

fn insert(
    value: &str,
    selection: Option<Selection>,
    code: &str,
    maximum: u32,
) -> Option<(String, u32)> {
    let length = value.encode_utf16().count() as u32;
    let range = selection
        .and_then(|selection| {
            let start = selection.start.min(selection.end).min(length);
            let end = selection.start.max(selection.end).min(length);
            Some((byte_at(value, start)?, byte_at(value, end)?))
        })
        .unwrap_or((value.len(), value.len()));
    let mut next = String::with_capacity(value.len() + code.len());
    next.push_str(&value[..range.0]);
    next.push_str(code);
    let caret = next.encode_utf16().count() as u32;
    next.push_str(&value[range.1..]);
    if next.encode_utf16().count() > maximum as usize {
        return None;
    }
    Some((next, caret))
}

fn byte_at(value: &str, index: u32) -> Option<usize> {
    let mut units = 0;
    for (at, ch) in value.char_indices() {
        if units == index {
            return Some(at);
        }
        units += ch.len_utf16() as u32;
    }
    (units == index).then_some(value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selected_astral_character_is_replaced_using_native_utf16_offsets() {
        assert_eq!(
            insert(
                "A😀中B",
                Some(Selection { start: 1, end: 3 }),
                "[微笑]",
                100
            ),
            Some(("A[微笑]中B".into(), 5))
        );
        assert_eq!(
            insert("中B", Some(Selection { start: 0, end: 0 }), "[爱慕]", 100),
            Some(("[爱慕]中B".into(), 4))
        );
    }

    #[test]
    fn no_selection_appends_but_overlong_insertions_leave_the_draft_unchanged() {
        assert_eq!(insert("中", None, "[笑]", 4), Some(("中[笑]".into(), 4)));
        assert_eq!(insert("😀", None, "[笑]", 4), None);
        assert_eq!(insert("", None, "[笑]", 3), Some(("[笑]".into(), 3)));
    }
}
