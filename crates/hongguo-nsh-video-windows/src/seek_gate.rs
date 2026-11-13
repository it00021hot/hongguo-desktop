//! Audio completion and visible video completion must agree on one seek generation.
#[derive(Default)]
pub(crate) struct SeekGate {
    generation: Option<u64>,
    audio: bool,
    video: bool,
}

impl SeekGate {
    pub fn begin(&mut self, generation: u64) {
        self.generation = Some(generation);
        self.audio = false;
        self.video = false;
    }

    pub fn audio_completed(&mut self) {
        self.audio = true;
    }

    pub fn video_completed(&mut self, generation: u64) {
        if self.generation == Some(generation) {
            self.video = true;
        }
    }

    pub fn waiting(&self) -> bool {
        self.generation.is_some() && !(self.audio && self.video)
    }

    pub fn needs_video(&self) -> bool {
        self.generation.is_some() && !self.video
    }

    pub fn accepts(&self, generation: u64) -> bool {
        self.generation.is_none_or(|current| current == generation)
    }

    pub fn complete(&self) -> bool {
        self.generation.is_some() && self.audio && self.video
    }
}
