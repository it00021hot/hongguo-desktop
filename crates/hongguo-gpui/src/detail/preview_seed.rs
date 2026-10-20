//! 详情页离屏预览种子（`screenshot` feature）。

use super::*;

use super::load::synthesize_upcoming_series;

/// 预览种子（screenshot feature）：离屏矩阵的详情页场景——灌固定档案 +
/// meta + 剧评 + 相关推荐 + 假封面，全程零网络。`tab` 选下半区初始 tab
///（"detail" = 选集，"detail-related" = 相关推荐；剧评 tab 与选集共用
/// 一张基准图即可覆盖，不单独出图）。
#[cfg(feature = "screenshot")]
impl DetailView {
    pub(crate) fn debug_preview_seed(&mut self, tab: &str, cx: &mut Context<Self>) {
        use crate::preview::fixtures;
        use hongguo_api::detail::{RelatedItem, RelatedSeries, SeriesMeta};

        self.loading = false;
        self.load_failed = false;
        if tab == "detail-upcoming" {
            // 未上线降级视图：prefill 合成 0 集档案（预约态起「预约」按钮）
            let prefill = DetailPrefill {
                title: "长风渡口".into(),
                cover: "preview://detail-cover".into(),
                tags: vec!["古装".into(), "权谋".into()],
                desc: "战乱年代，一介孤女在渡口撑起商行，卷入朝堂风云。".into(),
                reserved: true,
            };
            self.upcoming = true;
            self.reserved = prefill.reserved;
            self.prefill = Some(prefill.clone());
            self.series_id = "7200000".into();
            self.series = Some(Arc::new(synthesize_upcoming_series("7200000", &prefill)));
            crate::preview::fixtures::put_preview_image(
                "preview://detail-cover",
                fixtures::cover(6),
            );
            self.meta = None;
            self.tab = DetailTab::Episodes;
            cx.notify();
            return;
        }
        self.tab = if tab == "detail-related" {
            DetailTab::Related
        } else {
            DetailTab::Episodes
        };
        let series = Series {
            series_id: "7100000".into(),
            title: "命运契约".into(),
            cover: "preview://detail-cover".into(),
            episode_count: 80,
            tags: vec!["玄幻".into(), "逆袭".into(), "修真".into()],
            episodes: (1..=80)
                .map(|i| hongguo_common::model::Episode {
                    vid_index: i,
                    vid: format!("7100000:{i}"),
                    duration: 60 + (i as i64 % 7) * 9,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        self.series_id = series.series_id.clone();
        self.series = Some(Arc::new(series));
        let meta = SeriesMeta {
            hot_score: 37_860_000,
            followed_cnt: 271_000,
            play_cnt: 4_030_000,
            season: "第1季".into(),
            record_number: "京网文〔2026〕0000-000号".into(),
            intro: "落魄少年在一场契约游戏里觉醒了读档能力：每一次死亡都回到十分钟前。\
                      他靠着这个能力在豪门倾轧与修真秘境之间反复横跳，把仇人一个个熬成了路人。\
                      可他渐渐发现，所谓契约游戏的庄家，正是十分钟后的自己。"
                .into(),
            ..Default::default()
        };
        self.meta = Some(meta);
        crate::preview::fixtures::put_preview_image("preview://detail-cover", fixtures::cover(0));
        let mut reviews: Vec<CommentItem> = fixtures::comments();
        if let Some(last) = reviews.last_mut() {
            last.score = "9".into();
            last.score_suffix_text = "观看2小时后点评".into();
        }
        self.reviews = Arc::new(reviews.clone());
        self.reviews_total = 3716;
        self.reviews_score = "8.6".into();
        self.reviews_score_cnt = 12_000;
        self.reviews_tag_stats = vec![
            hongguo_api::danmaku::CommentTagStat {
                tag_name: "修仙世界观宏大".into(),
                count: 26,
            },
            hongguo_api::danmaku::CommentTagStat {
                tag_name: "爽".into(),
                count: 18,
            },
        ];
        let mut works = fixtures::related_seasons();
        for (i, item) in works.iter_mut().enumerate() {
            let url = format!("preview://related-{i}");
            crate::preview::fixtures::put_preview_image(&url, fixtures::cover(i + 1));
            item.cover = url;
        }
        let mut guess = vec![RelatedItem {
            series_id: "7200001".into(),
            title: "重生之商海风云".into(),
            score: 7.9,
            play_cnt: 8_800_000,
            episode_cnt: 66,
            ..Default::default()
        }];
        guess.push(RelatedItem {
            series_id: "7200002".into(),
            title: "闪婚老公是豪门".into(),
            play_cnt: 12_400_000,
            episode_cnt: 99,
            ..Default::default()
        });
        for (i, item) in guess.iter_mut().enumerate() {
            let url = format!("preview://guess-{i}");
            crate::preview::fixtures::put_preview_image(&url, fixtures::cover(i + 4));
            item.cover = url;
        }
        self.related = Some(Arc::new(RelatedSeries { works, guess }));
        self.interact_items = Arc::new(vec![InteractionItem {
            series_id: "7100000".into(),
            followed: true,
            ..Default::default()
        }]);
        cx.notify();
    }
}
