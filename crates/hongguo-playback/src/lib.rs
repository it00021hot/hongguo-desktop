//! Shared playback state, indexed source and native decoder orchestration.

mod aac_seek;
mod budget;
mod clock;
mod index;
mod pcm;
mod session;

pub use clock::MonotonicClock;
pub use index::{IndexedSource, IndexedTrack, TrackCodec};
pub use pcm::{PcmChunk, PcmRing};
pub use session::{
    PlaybackSession, PlaybackState, ReadAt, RecoveryLimits, SessionConfig, SessionInfo,
    SessionPosition,
};
