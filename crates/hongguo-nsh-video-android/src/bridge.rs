use hongguo_nsh_video_api::{EVENT_NAME, VideoEvent};
use jni::{
    JavaVM,
    objects::{GlobalRef, JValue},
};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct Bridge {
    pub vm: Arc<JavaVM>,
    owner: GlobalRef,
}
impl Bridge {
    pub fn direct_presented(
        &self,
        epoch: u64,
        frame: &hongguo_nsh_video_api::frames::PixelFrame,
    ) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(
            &self.owner,
            "directPresented",
            "(JJJJII)V",
            &[
                JValue::Long(epoch as i64),
                JValue::Long(frame.generation as i64),
                JValue::Long(frame.serial as i64),
                JValue::Long(frame.pts_ms as i64),
                JValue::Int(frame.width as i32),
                JValue::Int(frame.height as i32),
            ],
        )
        .map_err(error)?;
        Ok(())
    }
    pub fn direct_holding(&self, epoch: u64, holding: bool) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(
            &self.owner,
            "directHolding",
            "(JZ)V",
            &[JValue::Long(epoch as i64), JValue::Bool(holding.into())],
        )
        .map_err(error)?;
        Ok(())
    }
    pub fn new(vm: Arc<JavaVM>, owner: GlobalRef) -> Self {
        Self { vm, owner }
    }

    /// Java only schedules MediaPlayer/View operations on its main handler.
    /// Its bounded completion waits run on this render worker, never on the UI thread.
    pub fn state(&self, json: &str, generation: u64) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        let acknowledged = env
            .with_local_frame(8, |env| -> jni::errors::Result<bool> {
                let json = env.new_string(json)?;
                env.call_method(
                    &self.owner,
                    "applyState",
                    "(Ljava/lang/String;J)Z",
                    &[JValue::Object(&json), JValue::Long(generation as i64)],
                )?
                .z()
            })
            .map_err(error)?;
        if acknowledged {
            Ok(())
        } else {
            Err("MediaPlayer state update was not acknowledged".into())
        }
    }
    pub fn seek(&self, json: &str, generation: u64) -> Result<(), String> {
        self.string_call("requestSeek", json, generation)
    }
    pub fn event(&self, event: &VideoEvent) {
        let result = serde_json::to_string(event)
            .map_err(error)
            .and_then(|json| self.string_call("emitVideo", &json, 0));
        if let Err(error) = result {
            log::warn!("[Android video] {EVENT_NAME} delivery failed: {error}");
        }
    }
    fn string_call(&self, name: &str, json: &str, generation: u64) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.with_local_frame(8, |env| -> jni::errors::Result<()> {
            let json = env.new_string(json)?;
            env.call_method(
                &self.owner,
                name,
                "(Ljava/lang/String;J)V",
                &[JValue::Object(&json), JValue::Long(generation as i64)],
            )?;
            Ok(())
        })
        .map_err(error)
    }
    pub fn show(&self, generation: u64, enhanced: bool) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(
            &self.owner,
            "showFrames",
            "(JZ)V",
            &[
                JValue::Long(generation as i64),
                JValue::Bool(enhanced.into()),
            ],
        )
        .map_err(error)?;
        Ok(())
    }
    pub fn prepare_output(&self, generation: u64) -> Result<(), String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(
            &self.owner,
            "prepareFrameOutput",
            "(J)V",
            &[JValue::Long(generation as i64)],
        )
        .map_err(error)?;
        Ok(())
    }
    pub fn clear_exception(&self) {
        if let Ok(env) = self.vm.attach_current_thread() {
            let _ = env.exception_clear();
        }
    }
}
pub(crate) fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}
pub(crate) const FRAME_TICK: Duration = Duration::from_millis(16);
