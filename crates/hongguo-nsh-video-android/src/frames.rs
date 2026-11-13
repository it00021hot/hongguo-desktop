use crate::convert::Conversion;
use hongguo_nsh_video_api::{ErrorCode, FrameMetadata};
use jni::{
    JavaVM,
    objects::{GlobalRef, JValue},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub struct VideoError {
    code: ErrorCode,
    detail: String,
}
impl VideoError {
    pub fn code(&self) -> ErrorCode {
        self.code
    }
    pub fn detail(&self) -> &str {
        &self.detail
    }
}
impl std::fmt::Display for VideoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}
impl std::error::Error for VideoError {}
impl From<jni::errors::Error> for VideoError {
    fn from(error: jni::errors::Error) -> Self {
        failure(error)
    }
}
fn failure(error: impl std::fmt::Display) -> VideoError {
    VideoError {
        code: ErrorCode::FrameUnavailable,
        detail: error.to_string(),
    }
}

/// Owns the MediaPlayer output Surface and an external-OES → normal RGBA input pass.
/// Create/use/drop on the same render worker; no Java/QuickJS playback state lives here.
pub struct SurfaceFrames {
    device: Arc<wgpu::Device>,
    vm: Arc<JavaVM>,
    surface: GlobalRef,
    surface_texture: GlobalRef,
    conversion: Conversion,
    texture: wgpu::Texture,
    size: wgpu::Extent3d,
    busy: Arc<AtomicBool>,
    last_timestamp: Option<i64>,
}

impl SurfaceFrames {
    pub fn new(
        device: Arc<wgpu::Device>,
        vm: Arc<JavaVM>,
        width: u32,
        height: u32,
    ) -> Result<Self, VideoError> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(failure("invalid Android video dimensions"));
        }
        // SAFETY: Only accesses the live GLES backend; the adapter guard owns current-context access.
        let native = unsafe { device.as_hal::<wgpu::hal::api::Gles>() }
            .ok_or_else(|| failure("Android pixel bridge requires WGPU GLES"))?;
        let context = native.context();
        let gl = context.lock();
        let mut conversion = Conversion::new(&gl, width as i32, height as i32).map_err(failure)?;
        let result = (|| {
            let mut env = vm.attach_current_thread().map_err(failure)?;
            let external = conversion
                .external
                .expect("conversion texture initialized")
                .0
                .get();
            let surface_texture = env
                .new_object(
                    "android/graphics/SurfaceTexture",
                    "(I)V",
                    &[JValue::Int(external as i32)],
                )
                .map_err(failure)?;
            env.call_method(
                &surface_texture,
                "setDefaultBufferSize",
                "(II)V",
                &[JValue::Int(width as i32), JValue::Int(height as i32)],
            )
            .map_err(failure)?;
            let surface = env
                .new_object(
                    "android/view/Surface",
                    "(Landroid/graphics/SurfaceTexture;)V",
                    &[JValue::Object(&surface_texture)],
                )
                .map_err(failure)?;
            Ok::<_, VideoError>((
                env.new_global_ref(surface).map_err(failure)?,
                env.new_global_ref(surface_texture).map_err(failure)?,
            ))
        })();
        let (surface, surface_texture) = match result {
            Ok(result) => result,
            Err(error) => {
                conversion.destroy(&gl);
                clear_exception(&vm);
                return Err(error);
            }
        };
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        // SAFETY: Conversion cleared the entire RGBA8 texture on this device's current EGL context.
        // Ownership of that ordinary 2D texture is transferred to HAL, not the external-OES texture.
        let raw = unsafe {
            native.texture_from_raw(
                conversion
                    .rgba
                    .take()
                    .expect("conversion output initialized")
                    .0,
                &wgpu::hal::TextureDescriptor {
                    label: Some("android-video-rgba"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUses::RESOURCE,
                    memory_flags: wgpu::hal::MemoryFlags::empty(),
                    view_formats: vec![],
                },
                None,
            )
        };
        drop(gl);
        drop(native);
        // SAFETY: The initialized HAL texture comes from this exact device and matches the descriptor.
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Gles>(
                raw,
                &wgpu::TextureDescriptor {
                    label: Some("android-video-rgba"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        Ok(Self {
            device,
            vm,
            surface,
            surface_texture,
            conversion,
            texture,
            size,
            busy: Arc::new(AtomicBool::new(false)),
            last_timestamp: None,
        })
    }

    /// Attach this output to the same application-owned MediaPlayer. Stop/detach that
    /// player before dropping the bridge, and keep its ordinary display Surface for failure fallback.
    pub fn surface(&self) -> &GlobalRef {
        &self.surface
    }
    pub(crate) fn surface_texture(&self) -> &GlobalRef {
        &self.surface_texture
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Reprocess the last real decoder image after a setting/viewport change,
    /// including while paused. Never manufactures a frame before the first one.
    pub(crate) fn retained_frame(&self) -> Option<FrameLease> {
        let timestamp = self.last_timestamp?;
        if self.busy.swap(true, Ordering::AcqRel) {
            return None;
        }
        let mut metadata = FrameMetadata::default();
        metadata.pts_ms = u64::try_from(timestamp / 1_000_000).unwrap_or(0);
        metadata.width = self.size.width;
        metadata.height = self.size.height;
        Some(FrameLease {
            texture: self.texture.clone(),
            metadata,
            busy: self.busy.clone(),
        })
    }

    /// Call only after SurfaceTexture reports a frame; no pixel bytes cross JNI.
    pub fn capture(&mut self) -> Result<Option<FrameLease>, VideoError> {
        if self.busy.load(Ordering::Acquire) {
            return Ok(None);
        }
        // SAFETY: This guard and lock make the same EGL context current for updateTexImage and the conversion draw.
        let native = unsafe { self.device.as_hal::<wgpu::hal::api::Gles>() }
            .ok_or_else(|| failure("GLES device unavailable"))?;
        let gl = native.context().lock();
        let result = (|| {
            let mut env = self.vm.attach_current_thread().map_err(failure)?;
            // The worker may already be attached by the host. Explicitly pop every
            // per-frame Java array/reference instead of relying on thread detach.
            env.with_local_frame(8, |env| {
                let source = self.surface_texture.as_obj();
                env.call_method(source, "updateTexImage", "()V", &[])
                    .map_err(failure)?;
                let timestamp = env
                    .call_method(source, "getTimestamp", "()J", &[])
                    .map_err(failure)?
                    .j()
                    .map_err(failure)?;
                if self.last_timestamp == Some(timestamp) {
                    return Ok(None);
                }
                let matrix = env.new_float_array(16).map_err(failure)?;
                env.call_method(
                    source,
                    "getTransformMatrix",
                    "([F)V",
                    &[JValue::Object(&matrix)],
                )
                .map_err(failure)?;
                let mut transform = [0_f32; 16];
                env.get_float_array_region(&matrix, 0, &mut transform)
                    .map_err(failure)?;
                if transform.iter().any(|value| !value.is_finite()) {
                    return Err(failure("SurfaceTexture returned a non-finite transform"));
                }
                self.conversion
                    .draw(
                        &gl,
                        self.size.width as i32,
                        self.size.height as i32,
                        &transform,
                    )
                    .map_err(failure)?;
                self.last_timestamp = Some(timestamp);
                self.busy.store(true, Ordering::Release);
                let mut metadata = FrameMetadata::default();
                metadata.pts_ms = u64::try_from(timestamp / 1_000_000).unwrap_or(0);
                metadata.width = self.size.width;
                metadata.height = self.size.height;
                Ok(Some(FrameLease {
                    texture: self.texture.clone(),
                    metadata,
                    busy: self.busy.clone(),
                }))
            })
        })();
        if result.is_err() {
            clear_exception(&self.vm);
        }
        result
    }
}

impl Drop for SurfaceFrames {
    fn drop(&mut self) {
        if let Ok(mut env) = self.vm.attach_current_thread() {
            for object in [&self.surface, &self.surface_texture] {
                if let Err(error) = env.call_method(object.as_obj(), "release", "()V", &[]) {
                    let _ = env.exception_clear();
                    log::warn!("Android video Surface release failed: {error}");
                }
            }
        }
        // SAFETY: Drop runs on the owning render worker; the guard makes its original context current.
        if let Some(native) = unsafe { self.device.as_hal::<wgpu::hal::api::Gles>() } {
            self.conversion.destroy(&native.context().lock());
        }
    }
}

pub struct FrameLease {
    texture: wgpu::Texture,
    metadata: FrameMetadata,
    busy: Arc<AtomicBool>,
}
impl FrameLease {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn metadata(&self) -> FrameMetadata {
        self.metadata
    }
    /// Keeps the single converted texture busy until all GPU reads really complete.
    pub fn submit(
        self,
        encoder: wgpu::CommandEncoder,
        queue: &wgpu::Queue,
    ) -> wgpu::SubmissionIndex {
        let submission = queue.submit([encoder.finish()]);
        queue.on_submitted_work_done(move || {
            self.busy.store(false, Ordering::Release);
            drop(self);
        });
        submission
    }
}

fn clear_exception(vm: &JavaVM) {
    if let Ok(env) = vm.attach_current_thread() {
        let _ = env.exception_clear();
    }
}
