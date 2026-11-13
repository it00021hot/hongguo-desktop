//! Kernel hooks run without UI state; submission crosses to the owning UI scope.
use super::document;
use nsh_base::rich_editor::{
    model::{Fragment, ResolvedPos, Slice},
    state::{EditorState, KeyEvent, PluginSpec, Transaction},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::mpsc::UnboundedSender;

pub(super) struct Controls {
    pub disabled: AtomicBool,
    pub binding: AtomicU64,
    pub single_line: AtomicBool,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            disabled: AtomicBool::new(true),
            binding: AtomicU64::new(0),
            single_line: AtomicBool::new(false),
        }
    }
}

pub(super) enum Action {
    Submit,
    Tab(bool),
    Reveal,
}

pub(super) struct Editing {
    pub maximum: u32,
    pub controls: Arc<Controls>,
    pub submit: UnboundedSender<(u64, Action)>,
}

impl PluginSpec for Editing {
    fn handle_scroll_to_selection(&self, _state: &EditorState) -> bool {
        if !self.controls.single_line.load(Ordering::Acquire) {
            return false;
        }
        let _ = self.submit.send((
            self.controls.binding.load(Ordering::Acquire),
            Action::Reveal,
        ));
        true
    }
    // The app's wire format is plain codes. A copied rich document must not prefer
    // incidental HTML over that lossless text representation.
    fn transform_pasted_html(&self, _state: &EditorState, _html: String) -> String {
        String::new()
    }
    fn filter_transaction(&self, tr: &Transaction, _state: &EditorState) -> bool {
        document::wire(tr.doc()).encode_utf16().count() <= self.maximum as usize
    }
    fn append_transaction(
        &self,
        _trs: &[Transaction],
        _old: &EditorState,
        new: &EditorState,
    ) -> Option<Transaction> {
        let wire = document::wire(new.doc());
        let normalized = document::document(new.schema(), &wire);
        if &normalized == new.doc() {
            return None;
        }
        let selection = document::wire_selection(new.doc(), new.selection());
        let mut tr = new.tr();
        tr.replace_with(0, new.doc().content().size(), normalized.content().clone())
            .ok()?;
        tr.set_selection(document::document_selection(tr.doc(), selection));
        Some(tr)
    }
    fn handle_key_down(
        &self,
        _state: &EditorState,
        event: &KeyEvent,
        _dispatch: &mut dyn FnMut(Transaction),
    ) -> bool {
        let action = match event.key.as_str() {
            "Enter" => Action::Submit,
            "Tab" if !(event.mod_key || event.ctrl || event.meta || event.alt) => {
                Action::Tab(event.shift)
            }
            _ => return false,
        };
        // RichEditor's native controller never offers composing keys to plugins.
        if !self.controls.disabled.load(Ordering::Acquire) {
            let _ = self
                .submit
                .send((self.controls.binding.load(Ordering::Acquire), action));
        }
        true
    }
    fn clipboard_text_parser(
        &self,
        state: &EditorState,
        text: &str,
        _context: &ResolvedPos,
        _plain: bool,
    ) -> Option<Slice> {
        let doc = document::document(state.schema(), text);
        Some(Slice::new(doc.content().child(0).content().clone(), 0, 0))
    }
    fn clipboard_text_serializer(&self, _state: &EditorState, slice: &Slice) -> Option<String> {
        Some(fragment_wire(&slice.content))
    }
}

fn fragment_wire(fragment: &Fragment) -> String {
    let mut out = String::new();
    for (index, node) in fragment.iter().enumerate() {
        if node.is_text() {
            out.push_str(node.text().unwrap_or_default());
        } else if node.ty().name() == "hard_break" {
            out.push('\n');
        } else if node.is_leaf() {
            out.push_str(
                node.attrs()
                    .get("wire")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            );
        } else {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&fragment_wire(node.content()));
        }
    }
    out
}
