use super::MetaData;
use crate::ui;
use nsh_core::prelude::*;

pub(super) fn content(meta: MetaData, open: impl Fn(String) + Copy + 'static) -> View {
    let creators = computed(move || {
        meta.with(|meta| {
            meta.map(|meta| {
                meta.creators
                    .iter()
                    .map(|creator| ui::CreatorChip {
                        id: creator.user_id.clone(),
                        name: creator.name.clone(),
                        avatar: crate::image_url::cdn_image_url(&creator.avatar),
                        role: creator.role.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        })
    });
    view()
        .test_id("detail-creators")
        .mt_4()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_3()
        .style_when(move || creators.with(Vec::is_empty), |style| style.hidden())
        .child(each(
            move || creators.get(),
            |creator| format!("{}:{}{}", creator.id.len(), creator.id, creator.name),
            move |creator, _| {
                ui::creator_chip(computed(move || creator.get())).on_press(move || {
                    let id = creator.with(|creator| creator.id.clone());
                    if !id.is_empty() {
                        open(id);
                    }
                })
            },
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Chips() -> impl IntoElement {
        let picked = signal(String::new());
        let meta: MetaData = resource(
            || Some(()),
            |_, _| {
                std::future::ready(Ok(hongguo_api::detail::SeriesMeta {
                    creators: crate::pages::preview_creators(),
                    ..Default::default()
                }))
            },
        );
        view()
            .child(content(meta, move |id| picked.set(id)))
            .child(text(move || format!("creator:{}", picked.get())))
            .child(
                crate::ui::button("Clear creators", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || meta.mutate(hongguo_api::detail::SeriesMeta::default())),
            )
    }

    fn start() -> Headless {
        Headless::start(EngineConfig::new(Root::of::<Chips>().tokens(
            nsh_base::tokens_of(Some(include_str!("../../../theme.json"))).unwrap(),
        )))
        .unwrap()
    }

    #[test]
    fn real_creator_ids_are_actionable_and_name_only_credits_are_disabled() -> Result<(), String> {
        let mut app = start();
        app.wait_for("id:detail-creators")?;
        assert!(app.shows("主演"));
        assert!(app.shows("出品方"));
        assert!(!app.screen().enabled("role:button[幕后团队's homepage]")?);
        app.press("role:button[离线创作者's homepage]")?;
        app.wait_for("text:creator:offline-creator-main")?;
        Ok(())
    }

    #[test]
    fn removing_all_credits_hides_the_section_instead_of_leaving_an_empty_gap() -> Result<(), String>
    {
        let mut app = start();
        app.wait_for("id:detail-creators")?;
        app.press("role:button[Clear creators]")?;
        assert!(!app.screen().matches("id:detail-creators")?);
        Ok(())
    }
}
