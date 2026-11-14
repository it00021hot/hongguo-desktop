//! Shared, platform-independent interpretation of diagnostic frame events.
#[derive(Default)]
pub(super) struct Frame {
    pub pts: f64,
    pub duration: f64,
    pub decoded_ms: f64,
    pub draw_done_ms: Option<f64>,
    pub present_ms: Option<f64>,
    pub dropped: bool,
}
impl Frame {
    pub fn presented(&mut self, at: f64) {
        self.present_ms.get_or_insert(at);
        self.dropped = false;
    }
    pub fn skipped(&mut self) {
        if self.present_ms.is_none() {
            self.dropped = true;
        }
    }
    pub fn due(&self, clock_start_ms: Option<f64>) -> Option<f64> {
        clock_start_ms.map(|start| start + self.pts * 1000.0)
    }
    pub fn deadline(&self, clock_start_ms: Option<f64>) -> Option<f64> {
        self.due(clock_start_ms)
            .map(|due| due + self.duration * 1000.0)
    }
    pub fn missed_deadline(&self, clock_start_ms: Option<f64>) -> Option<bool> {
        self.present_ms
            .zip(self.deadline(clock_start_ms))
            .map(|(at, end)| at > end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_gpu_draw_does_not_imply_canvas_presentation_or_a_measured_deadline() {
        let frame = Frame {
            pts: 0.1,
            duration: 0.04,
            decoded_ms: 20.0,
            draw_done_ms: Some(30.0),
            ..Default::default()
        };
        assert_eq!(frame.decoded_ms, 20.0);
        assert_eq!(frame.draw_done_ms, Some(30.0));
        assert_eq!(frame.due(Some(100.0)), Some(200.0));
        assert_eq!(frame.deadline(Some(100.0)), Some(240.0));
        assert_eq!(frame.missed_deadline(Some(100.0)), None);
        assert_eq!(frame.missed_deadline(None), None);
    }
    #[test]
    fn a_late_enhanced_frame_can_be_presented_after_its_raw_candidate_was_skipped() {
        let mut frame = Frame {
            pts: 0.1,
            duration: 0.04,
            ..Default::default()
        };
        frame.skipped();
        assert!(frame.dropped);
        frame.presented(250.0);
        assert!(!frame.dropped);
        assert_eq!(frame.missed_deadline(Some(100.0)), Some(true));
        frame.presented(300.0);
        assert_eq!(frame.present_ms, Some(250.0));
        frame.skipped();
        assert!(!frame.dropped);
    }
    #[test]
    fn presentation_inside_one_frame_budget_meets_the_deadline() {
        let mut frame = Frame {
            pts: 0.1,
            duration: 0.04,
            ..Default::default()
        };
        frame.presented(240.0);
        assert_eq!(frame.missed_deadline(Some(100.0)), Some(false));
        assert_eq!(frame.missed_deadline(None), None);
    }
}
