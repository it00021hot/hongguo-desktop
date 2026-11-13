use super::{cancellation::Cancellation, source::Source};

#[derive(Clone)]
pub(crate) struct MediaReadiness {
    source: Source,
}

impl MediaReadiness {
    pub(super) fn new(source: Source) -> Self {
        Self { source }
    }

    pub(crate) async fn wait_for_available(&self, target: f64) -> Result<(), String> {
        if !target.is_finite() || target < 0.0 {
            return Err("invalid media seek target".into());
        }
        let cancellation = Cancellation::default();
        let flag = cancellation.flag();
        let source = self.source.clone();
        let result = hongguo_service::runtime::spawn_blocking(move || {
            source.wait_for_available(target, &flag)
        })
        .await
        .map_err(|error| error.to_string())?;
        drop(cancellation);
        result
    }
}
