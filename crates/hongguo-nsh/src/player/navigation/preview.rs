use super::{Controller, FeedNavigation, State};
use crate::ui;
use nsh_core::{keymap::When, prelude::*};

pub(crate) fn build() -> View {
    let count = signal(0_i32);
    let episode = signal(1_i32);
    let taps = signal(0_u32);
    let activity = signal(0_u32);
    let panel = signal(false);
    let active = signal(true);
    let feed = FeedNavigation::new(move |delta| {
        count.update(|count| *count += delta);
        true
    });
    let controller = Controller::new(
        State {
            active: computed(move || active.get()),
            blocked: computed(move || panel.get()),
            series: computed(move || format!("offline-{}", count.get())),
        },
        Some(feed),
        move |delta| episode.update(|episode| *episode += delta),
    )
    .on_activity(move || activity.update(|activity| *activity += 1));
    let locked = controller.in_binge();
    effect(move || {
        if active.get() && !panel.get() {
            for (key, direction) in [("up", -1), ("down", 1)] {
                on_key_with(
                    key,
                    KeyOptions {
                        when: When::NotEditing,
                        repeat: false,
                    },
                    move |_| controller.step_keyboard(direction),
                );
            }
            on_key_with(
                "escape",
                KeyOptions {
                    when: When::NotEditing,
                    repeat: false,
                },
                move |_| controller.exit_binge(),
            );
        }
    });
    view()
        .flex_1()
        .p_4()
        .gap_3()
        .test_id("navigation-preview")
        .child(text("导航手势离线验证"))
        .child(
            text(move || {
                format!(
                    "feed:{} episode:{} taps:{}",
                    count.get(),
                    episode.get(),
                    taps.get()
                )
            })
            .test_id("navigation-counts"),
        )
        .child(text(move || {
            format!("locked:{} activity:{}", locked.get(), activity.get())
        }))
        .child(
            controller.bind(
                ui::player_stage()
                    .flex_grow(0.0)
                    .h(px(400.0))
                    .w_full()
                    .test_id("navigation-video")
                    .pointer_events(PointerEvents::BoxOnly)
                    .accessible(true)
                    .role(nsh_core::AccessibilityRole::Button)
                    .label("离线视频画面")
                    .on_press(move || taps.update(|taps| *taps += 1))
                    .items_center()
                    .justify_center()
                    .child(text("在此滚轮或纵向滑动")),
            ),
        )
        .child(
            view()
                .flex_row()
                .gap_2()
                .child(
                    ui::button("锁定连播", ui::ButtonVariant::Secondary)
                        .on_press(move || controller.enter_binge()),
                )
                .child(
                    ui::button("退出连播", ui::ButtonVariant::Secondary)
                        .on_press(move || controller.exit_binge()),
                )
                .child(
                    ui::button("打开验证面板", ui::ButtonVariant::Secondary)
                        .on_press(move || panel.set(true)),
                )
                .child(
                    ui::button("模拟离页", ui::ButtonVariant::Quiet)
                        .on_press(move || active.set(false)),
                ),
        )
        .child(ui::sheet(panel, "导航屏蔽验证", || {
            view().child(text("面板开启时，视频导航保持不变"))
        }))
}
