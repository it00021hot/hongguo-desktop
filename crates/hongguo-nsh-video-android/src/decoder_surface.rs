use crate::{
    bridge::{Bridge, error},
    gl_state::{EXTERNAL, State},
};
use glow::HasContext;
use hongguo_media_types::{DecodeDevice, HardwareKind};
use jni::{
    JavaVM,
    objects::{GlobalRef, JValue},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct DecoderSurface {
    pub device: Arc<wgpu::Device>,
    pub vm: Arc<JavaVM>,
    helper: GlobalRef,
    surface: GlobalRef,
    pub texture: glow::Texture,
    pub p010: bool,
    generation: u64,
}

// SAFETY: JavaVM/global refs are cross-thread leases. GL access is exclusively
// protected by this device's EGL context lock, including destruction.
unsafe impl DecodeDevice for DecoderSurface {
    fn kind(&self) -> HardwareKind {
        HardwareKind::MediaCodec
    }
    fn device(&self) -> usize {
        self.vm.get_java_vm_pointer() as usize
    }
    fn surface(&self) -> usize {
        self.surface.as_obj().as_raw() as usize
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}

impl DecoderSurface {
    pub fn new(
        device: Arc<wgpu::Device>,
        bridge: &Bridge,
        size: (u32, u32),
        p010: bool,
    ) -> Result<Arc<Self>, String> {
        // SAFETY: This renderer always requests the GLES backend.
        let hal =
            unsafe { device.as_hal::<wgpu::hal::gles::Api>() }.ok_or("GLES device required")?;
        let gl = hal.context().lock();
        if !gl.supported_extensions().contains("GL_EXT_YUV_target") {
            return Err("raw YUV external sampling is unavailable".into());
        }
        // SAFETY: The GLES context is current for texture creation and Java attach.
        let texture = unsafe {
            let _state = State::capture(&gl);
            let texture = gl.create_texture()?;
            gl.bind_texture(EXTERNAL, Some(texture));
            gl.tex_parameter_i32(EXTERNAL, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(EXTERNAL, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(EXTERNAL, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(EXTERNAL, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            texture
        };
        let created = bridge.decode_surface(texture.0.get(), size, p010);
        let (helper, surface) = match created {
            Ok(value) => value,
            Err(detail) => {
                // SAFETY: This unshared texture belongs to the current context.
                unsafe {
                    gl.delete_texture(texture);
                }
                bridge.clear_exception();
                return Err(detail);
            }
        };
        drop(gl);
        drop(hal);
        Ok(Arc::new(Self {
            device,
            vm: bridge.vm.clone(),
            helper,
            surface,
            texture,
            p010,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
        }))
    }

    pub fn timestamp(&self) -> Result<i64, String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(&self.helper, "nextTimestamp", "()J", &[])
            .and_then(|value| value.j())
            .map_err(error)
    }

    /// Caller holds this device's current EGL context.
    pub fn update(&self) -> Result<Option<(i64, [f32; 16])>, String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.with_local_frame(4, |env| -> jni::errors::Result<_> {
            let matrix = env.new_float_array(16)?;
            let timestamp = env
                .call_method(&self.helper, "update", "([F)J", &[JValue::Object(&matrix)])?
                .j()?;
            let mut values = [0.; 16];
            if timestamp != 0 {
                env.get_float_array_region(&matrix, 0, &mut values)?;
            }
            Ok((timestamp != 0).then_some((timestamp, values)))
        })
        .map_err(error)
    }

    pub fn acquire(&self) -> Result<Option<ImageLease>, String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        let acquired = env
            .with_local_frame(8, |env| -> jni::errors::Result<_> {
                let image = env
                    .call_method(&self.helper, "acquireImage", "()Landroid/media/Image;", &[])
                    .and_then(|v| v.l())?;
                if image.is_null() {
                    return Ok(None);
                }
                let image = env.new_global_ref(image)?;
                let buffer = match env
                    .call_method(
                        &image,
                        "getHardwareBuffer",
                        "()Landroid/hardware/HardwareBuffer;",
                        &[],
                    )
                    .and_then(|v| v.l())
                {
                    Ok(buffer) => buffer,
                    Err(error) => {
                        let _ = env.exception_clear();
                        let _ = env.call_method(&image, "close", "()V", &[]);
                        return Err(error);
                    }
                };
                let buffer = env.new_global_ref(buffer)?;
                let lease = ImageLease {
                    vm: self.vm.clone(),
                    image,
                    buffer,
                };
                Ok(Some(lease))
            })
            .map_err(error)?;
        if let Some(lease) = &acquired {
            let format = env
                .call_method(&lease.buffer, "getFormat", "()I", &[])
                .and_then(|v| v.i())
                .map_err(error)?;
            // HardwareBuffer.YCBCR_P010 is 0x36. A decoder ignoring the requested
            // format cannot pass this admission check, even if it reports Main10.
            if format != 0x36 {
                return Err(format!("decoder output is not P010: {format:#x}"));
            }
        }
        Ok(acquired)
    }
}

impl Drop for DecoderSurface {
    fn drop(&mut self) {
        // SAFETY: Retained device outlives this lease and all native decoder frames.
        if let Some(hal) = unsafe { self.device.as_hal::<wgpu::hal::gles::Api>() } {
            let gl = hal.context().lock();
            // SAFETY: A raw external-YUV draw can precede a later conversion or
            // timestamp error, before a WGPU retirement fence was registered.
            // Finish those reads before SurfaceTexture.release returns its buffer.
            unsafe {
                gl.finish();
            }
            release_helper(&self.vm, &self.helper);
            // SAFETY: The external texture is exclusively owned here.
            unsafe {
                gl.delete_texture(self.texture);
            }
        } else {
            release_helper(&self.vm, &self.helper);
        }
    }
}

fn release_helper(vm: &JavaVM, helper: &GlobalRef) {
    if let Ok(mut env) = vm.attach_current_thread() {
        let _ = env.exception_clear();
        if let Err(error) = env.call_method(helper, "release", "()V", &[]) {
            let _ = env.exception_clear();
            log::warn!("[Android video] decoder surface release: {error}");
        }
    }
}

pub(crate) struct ImageLease {
    pub vm: Arc<JavaVM>,
    pub image: GlobalRef,
    pub buffer: GlobalRef,
}
impl ImageLease {
    pub fn dimensions(&self) -> Result<[usize; 2], String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        let width = env
            .call_method(&self.buffer, "getWidth", "()I", &[])
            .and_then(|value| value.i())
            .map_err(error)?;
        let height = env
            .call_method(&self.buffer, "getHeight", "()I", &[])
            .and_then(|value| value.i())
            .map_err(error)?;
        if width <= 0 || height <= 0 {
            return Err("invalid P010 buffer dimensions".into());
        }
        Ok([width as usize, height as usize])
    }
    pub fn timestamp(&self) -> Result<i64, String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.call_method(&self.image, "getTimestamp", "()J", &[])
            .and_then(|v| v.j())
            .map_err(error)
    }
    pub fn crop(&self) -> Result<[i32; 4], String> {
        let mut env = self.vm.attach_current_thread().map_err(error)?;
        env.with_local_frame(2, |env| -> jni::errors::Result<_> {
            let rect = env
                .call_method(&self.image, "getCropRect", "()Landroid/graphics/Rect;", &[])?
                .l()?;
            Ok([
                env.get_field(&rect, "left", "I")?.i()?,
                env.get_field(&rect, "top", "I")?.i()?,
                env.get_field(&rect, "right", "I")?.i()?,
                env.get_field(&rect, "bottom", "I")?.i()?,
            ])
        })
        .map_err(error)
    }
}
impl Drop for ImageLease {
    fn drop(&mut self) {
        if let Ok(mut env) = self.vm.attach_current_thread() {
            for object in [&self.buffer, &self.image] {
                let _ = env.exception_clear();
                if let Err(error) = env.call_method(object, "close", "()V", &[]) {
                    let _ = env.exception_clear();
                    log::warn!("[Android video] image lease release: {error}");
                }
            }
        }
    }
}
