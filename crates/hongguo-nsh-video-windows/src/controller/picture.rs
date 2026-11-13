use super::*;

impl Controller {
    pub(super) fn draw_picture(
        &mut self,
        shared: &Arc<Shared>,
        props: &VideoProps,
        dc: &ID2D1DeviceContext,
        size: (f32, f32),
    ) -> Result<(), VideoError> {
        if !props.active
            || !size.0.is_finite()
            || !size.1.is_finite()
            || size.0 <= 0.0
            || size.1 <= 0.0
        {
            self.bitmap = None;
            return Ok(());
        }
        // A held/seek frame must be visible before its completion is acknowledged;
        // an older enhanced picture cannot satisfy this gate.
        let needs_raw = self
            .direct
            .as_ref()
            .map(Direct::needs_raw)
            .transpose()?
            .unwrap_or(false);
        let picture = shared
            .picture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|picture| !needs_raw && shared.valid(picture.revision))
            .map(|picture| picture.frame.clone());
        if let Some(frame) = picture {
            let result = (|| {
                if self
                    .bitmap
                    .as_ref()
                    .is_none_or(|(old, _)| !old.same_slot(&frame))
                {
                    self.bitmap = Some((frame.clone(), frame.bitmap(dc)?));
                }
                frame.draw(
                    dc,
                    &self.bitmap.as_ref().expect("bitmap initialized").1,
                    size,
                    props.resize_mode,
                )
            })();
            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.bitmap = None;
                    *shared
                        .gpu_error
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
                }
            }
        } else {
            self.bitmap = None;
        }
        if let Some(raw) = &self.raw {
            raw.draw(dc, size, props.resize_mode)?;
            if raw.metadata().is_some()
                && let Some(direct) = &mut self.direct
            {
                direct.submitted(dc, self.device.as_ref().expect("raw frame drawing device"))?;
            }
        }
        Ok(())
    }
}
