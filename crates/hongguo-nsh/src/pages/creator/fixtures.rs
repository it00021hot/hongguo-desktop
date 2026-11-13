use hongguo_api::{
    danmaku::{CreatorProfile, CreatorWorksPage},
    detail::SeriesCreator,
};

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../preview/fixtures/creator.json"))
        .expect("valid offline creator fixture")
}

pub(super) fn profile() -> CreatorProfile {
    let mut profile: CreatorProfile =
        serde_json::from_value(fixture()["profile"].clone()).expect("fixture creator profile");
    profile.avatar = super::super::fixtures::FRAME_PATH.into();
    profile
}

pub(super) fn works() -> CreatorWorksPage {
    let mut works: CreatorWorksPage =
        serde_json::from_value(fixture()["works"].clone()).expect("fixture creator works");
    for item in &mut works.works {
        item.cover = super::super::fixtures::FRAME_PATH.into();
    }
    works
}

pub(super) fn creators() -> Vec<SeriesCreator> {
    fixture()["creators"]
        .as_array()
        .expect("fixture creators")
        .iter()
        .map(|item| SeriesCreator {
            user_id: item["userId"].as_str().expect("fixture creator id").into(),
            name: item["name"].as_str().expect("fixture creator name").into(),
            avatar: if item["avatar"]
                .as_str()
                .expect("fixture avatar marker")
                .is_empty()
            {
                String::new()
            } else {
                super::super::fixtures::FRAME_PATH.into()
            },
            role: item["role"].as_str().expect("fixture creator role").into(),
        })
        .collect()
}
