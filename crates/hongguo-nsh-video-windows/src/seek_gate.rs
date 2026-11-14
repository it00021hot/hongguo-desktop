//! The shared session owns audio gating; the consumer confirms the visible epoch.
#[derive(Default)]
pub(crate) struct SeekGate {
    generation: Option<u64>,
    complete: bool,
}
impl SeekGate {
    pub fn begin(&mut self, generation: u64) {
        self.generation = Some(generation);
        self.complete = false;
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation.is_none_or(|current| current == generation)
    }
    pub fn presented(&mut self, generation: u64) {
        if self.generation == Some(generation) {
            self.complete = true;
        }
    }
    pub fn empty_edit(&mut self) {
        if self.generation.is_some() {
            self.complete = true;
        }
    }
    pub fn take_complete(&mut self) -> bool {
        let complete = self.complete;
        if complete {
            self.reset();
        }
        complete
    }
    pub fn reset(&mut self) {
        self.generation = None;
        self.complete = false;
    }
}
