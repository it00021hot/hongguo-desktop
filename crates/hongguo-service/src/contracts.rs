pub mod calendar {
    pub use hongguo_api::calendar::{
        CalendarCursor, CalendarFilteredPage, CalendarItem, CalendarPage, CalendarPanel,
        SelectorId, SelectorRow, SelectorSelection,
    };
}

pub mod danmaku {
    pub use hongguo_api::danmaku::{
        CommentTagStat, CreatorProfile, CreatorWork, CreatorWorksPage, Danmaku, DanmakuWindowPage,
    };
}

pub mod detail {
    pub use hongguo_api::detail::{RelatedItem, RelatedSeries, SeriesMeta};
}

pub mod discover {
    pub use hongguo_api::discover::{BrowseFilters, FeedItem, FeedPage, SelectorItem, SelectorRow};
}

pub mod emoticon {
    pub use hongguo_api::emoticon::{Emoticon, EmoticonPage};
}

pub mod history {
    pub use hongguo_api::history::WatchHistoryItem;
}

pub mod rank {
    pub use hongguo_api::rank::{
        RankItem, RankPage, RankPanelItem, RankPanelRow, RankReq, RankSubList, RankTab,
    };
}

pub mod recommend {
    pub use hongguo_api::recommend::FeedTabMeta;
}

pub mod search {
    pub use hongguo_api::search::SuggestItem;
}
pub mod highlight {
    pub use hongguo_api::highlight::album::AlbumMember;
}
