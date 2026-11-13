mod backend;
mod fixtures;
pub(super) mod merge;
mod model;
pub(super) mod storage;

#[cfg(test)]
mod tests;

/// Filesystem snapshots change with completed inputs, not each download progress tick.
fn completed_inputs(
    session: crate::events::SessionHandle,
) -> nsh_core::element::Memo<Vec<model::CompletedInput>> {
    use nsh_core::prelude::*;
    computed(move || {
        session.downloads();
        session
            .with(|session| session.state.queue().all())
            .into_iter()
            .filter(|task| task.is_done())
            .map(|task| model::CompletedInput {
                id: task.id,
                path: task.file_path,
                title: task.series_title,
            })
            .collect()
    })
}
