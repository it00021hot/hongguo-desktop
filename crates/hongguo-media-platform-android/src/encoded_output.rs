use hongguo_media::mux::AnnexbFrame;

#[derive(Default)]
pub(crate) struct EncodedOutput {
    configuration: Vec<u8>,
    pending: Vec<AnnexbFrame>,
}

impl EncodedOutput {
    pub fn configure(&mut self, bytes: &[u8]) {
        self.configuration.extend_from_slice(bytes);
    }
    pub fn frame(&mut self, pts: f64, bytes: &[u8], is_key: bool) {
        let mut annexb = Vec::new();
        if is_key {
            annexb.extend_from_slice(&self.configuration);
        }
        annexb.extend_from_slice(bytes);
        self.pending.push((pts, annexb, is_key));
    }
    pub fn take(&mut self) -> Vec<AnnexbFrame> {
        std::mem::take(&mut self.pending)
    }
}
