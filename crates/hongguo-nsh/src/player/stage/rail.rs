use crate::{
    i18n::tr,
    ui::{self, Icon},
};
use nsh_core::prelude::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct RailState {
    pub(crate) liked: bool,
    pub(crate) collected: bool,
    pub(crate) following: bool,
    pub(crate) show_follow: bool,
    pub(crate) likes: i64,
    pub(crate) collections: i64,
    pub(crate) comments: i64,
    pub(crate) like_pending: bool,
    pub(crate) collect_pending: bool,
    pub(crate) follow_pending: bool,
}
#[derive(Clone, Copy)]
pub(crate) enum RailAction {
    Collect,
    Comments,
    Like,
    Follow,
    Share,
}

pub(crate) fn rail(
    model: impl IntoReactive<RailState>,
    action: impl Fn(RailAction) + 'static,
) -> View {
    let model = model.into_reactive();
    let action = stored(Box::new(action) as Box<dyn Fn(RailAction)>);
    let model = computed(move || model.get());
    view()
        .gap_4()
        .items_center()
        .child(
            ui::player_rail_item(
                Icon::Collections,
                move || tr("player.interact.collect"),
                move || count(model.get().collections),
            )
            .selected(move || model.get().collected)
            .disabled(move || model.get().collect_pending)
            .on_press(move || action.with(|action| action(RailAction::Collect))),
        )
        .child(
            ui::player_rail_item(
                Icon::Comments,
                move || tr("player.interact.comments"),
                move || count(model.get().comments),
            )
            .on_press(move || action.with(|action| action(RailAction::Comments))),
        )
        .child(
            ui::player_rail_item(
                Icon::Liked,
                move || tr("player.interact.like"),
                move || count(model.get().likes),
            )
            .selected(move || model.get().liked)
            .disabled(move || model.get().like_pending)
            .on_press(move || action.with(|action| action(RailAction::Like))),
        )
        .child(when(
            move || model.get().show_follow,
            move || {
                ui::player_rail_item(
                    Icon::Follows,
                    move || tr("player.interact.follow"),
                    move || {
                        tr(if model.get().following {
                            "player.interact.following"
                        } else {
                            "player.interact.follow"
                        })
                    },
                )
                .selected(move || model.get().following)
                .disabled(move || model.get().follow_pending)
                .on_press(move || action.with(|action| action(RailAction::Follow)))
            },
        ))
        .child(
            ui::player_rail_item(
                Icon::Forward,
                move || tr("player.interact.share"),
                move || tr("player.interact.share"),
            )
            .on_press(move || action.with(|action| action(RailAction::Share))),
        )
}
fn count(value: i64) -> SharedString {
    if value <= 0 {
        return SharedString::default();
    }
    let (number, key) = if crate::i18n::is_chinese() {
        if value >= 1_000_000 {
            (value as f64 / 1_000_000.0, "number.baiwan")
        } else if value >= 10_000 {
            (value as f64 / 10_000.0, "number.wan")
        } else {
            return value.to_string().into();
        }
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "number.million")
    } else if value >= 10_000 {
        (value as f64 / 1000.0, "number.thousand")
    } else {
        return value.to_string().into();
    };
    let number = format!("{number:.1}");
    let number = number.strip_suffix(".0").unwrap_or(&number);
    tr(key).replace("{value}", number).into()
}
