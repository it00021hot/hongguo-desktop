use nsh_base::theme;
use nsh_core::prelude::*;

// Matches Tauri's radius=10, rounded-md=8 and rounded-xl=14 scale.
pub(super) fn control() -> Points {
    px((theme().radius.0 - 2.0).max(0.0))
}
pub(crate) fn card() -> Points {
    px(theme().radius.0 + 4.0)
}
