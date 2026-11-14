use nsh_core::{
    EventKind, EventPayload, ROOT_ID,
    audio::{AudioCommand, AudioError, AudioEvent},
};
use nsh_engine::EngineControl;

use crate::image::AssetBase;

#[derive(Default)]
pub(crate) struct Speaker;

pub(crate) fn command(
    _speaker: &Speaker,
    _key: u64,
    control: &EngineControl,
    command: &AudioCommand,
    _base: Option<&AssetBase>,
) {
    if matches!(
        command,
        AudioCommand::Stop { .. } | AudioCommand::Unload { .. } | AudioCommand::Close { .. }
    ) {
        return;
    }
    control.dispatch_event(
        ROOT_ID,
        EventKind::Audio,
        EventPayload::Audio {
            id: command.id(),
            event: AudioEvent::Failed {
                error: AudioError::Unavailable,
                message: "NSH system media support is not enabled in this build".into(),
            },
        },
    );
}
