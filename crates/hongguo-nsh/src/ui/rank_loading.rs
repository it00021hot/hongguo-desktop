use super::{browse_loading::skeleton, radii};
use nsh_core::prelude::*;

const ROWS: usize = 8;

pub(crate) fn rank_loading() -> View {
    view().flex_1().min_h_0().test_id("rank-loading").child(
        scroll().flex_1().child(
            view()
                .gap_3()
                .children((0..ROWS).map(|_| skeleton().h_24().w_full().rounded(radii::card()))),
        ),
    )
}
