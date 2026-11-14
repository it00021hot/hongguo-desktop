use super::*;
use std::rc::Rc;

pub(super) fn body(
    items: Memo<Vec<SelectOption>>,
    value: Memo<Option<SharedString>>,
    choose: impl Fn(SharedString) + Copy + 'static,
    close: impl Fn() + Copy + 'static,
) -> View {
    let items = Rc::new(items.get_untracked());
    let nodes = Rc::new(items.iter().map(|_| element_ref()).collect::<Vec<_>>());
    let list = element_ref();
    let cursor = signal(
        items
            .iter()
            .position(|item| !item.disabled && Some(&item.value) == value.get_untracked().as_ref())
            .or_else(|| items.iter().position(|item| !item.disabled)),
    );
    let focus = {
        let nodes = nodes.clone();
        move |at: Option<usize>| {
            cursor.set(at);
            if let Some(at) = at {
                list.scroll_to_index(at as u32, percentage(0.5), px(0.0), false);
                if let Some(node) = nodes.get(at) {
                    node.focus();
                }
            }
        }
    };
    let focus_keys = focus.clone();
    let options = items.clone();
    for (key, step) in [("up", -1_isize), ("down", 1), ("home", 0), ("end", 0)] {
        let options = options.clone();
        let focus_keys = focus_keys.clone();
        on_key_with(
            key,
            KeyOptions {
                when: nsh_core::keymap::When::NotEditing,
                repeat: true,
                ..Default::default()
            },
            move |_| {
                let enabled = options
                    .iter()
                    .enumerate()
                    .filter_map(|(at, item)| (!item.disabled).then_some(at))
                    .collect::<Vec<_>>();
                let next = match key {
                    "home" => enabled.first().copied(),
                    "end" => enabled.last().copied(),
                    _ => cursor
                        .get_untracked()
                        .and_then(|at| enabled.iter().position(|i| *i == at))
                        .and_then(|at| {
                            (!enabled.is_empty()).then(|| {
                                enabled[(at as isize + step).rem_euclid(enabled.len() as isize)
                                    as usize]
                            })
                        }),
                };
                focus_keys(next);
            },
        );
    }
    for key in ["tab", "shift+tab"] {
        on_key(key, move |_| close());
    }
    let initialized = stored(false);
    let initial_focus = focus.clone();
    let count = items.len();
    let keys = items
        .iter()
        .map(|item| item.value.clone())
        .collect::<Vec<_>>();
    let row_height = if is_desktop() { 32.0 } else { 48.0 };
    view().child(
        virtual_list(
            move || count,
            move |position| {
                let items = items.clone();
                let nodes = nodes.clone();
                // A native cell is recycled for another index. Rebuild only its item
                // presentation, retaining selection and navigation at the list owner.
                dynamic(move || {
                    let at = position.index();
                    let Some(item) = items.get(at) else {
                        return view();
                    };
                    let node = nodes[at];
                    let item = item.clone();
                    let title = item.title.clone();
                    let disabled = item.disabled;
                    let selected_value = item.value.clone();
                    view()
                        .element_ref(&node)
                        .role(nsh_core::AccessibilityRole::Option)
                        .accessible(true)
                        .label(title.clone())
                        .focusable(!disabled)
                        .accessibility_state(move || AccessibilityState {
                            selected: value.get().as_ref() == Some(&selected_value),
                            ..Default::default()
                        })
                        .disabled(disabled)
                        .w_full()
                        .h(px(row_height))
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .justify_start()
                        .bg(nsh_core::Color::TRANSPARENT)
                        .hover(|style| style.bg(move || theme().accent))
                        .focus(|style| style.bg(move || theme().accent))
                        .disabled_style(|style| style.opacity(0.45))
                        .on_layout(move |_: &Bounds<Points>| {
                            if is_desktop() && cursor.get_untracked() == Some(at) {
                                node.focus();
                            }
                        })
                        .on_focus(move || cursor.set(Some(at)))
                        .on_press(move || choose(item.value.clone()))
                        .child(text(title).text_sm())
                })
            },
        )
        .keys(move || keys.clone())
        .element_ref(&list)
        .role(nsh_core::AccessibilityRole::Listbox)
        .estimated_item_size(px(row_height))
        .h(move || px((count as f32 * row_height).min(window().height.0 * 0.6)))
        .on_layout(move |_: &Bounds<Points>| {
            if !initialized.replace(true) && is_desktop() {
                initial_focus(cursor.get_untracked());
            }
        }),
    )
}
