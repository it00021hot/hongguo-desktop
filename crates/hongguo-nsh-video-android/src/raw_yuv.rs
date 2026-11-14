use crate::{
    decoder_surface::{DecoderSurface, ImageLease},
    gl_state::{EXTERNAL, State},
};
use glow::HasContext;
use hongguo_media_types::FrameDescriptor;
use hongguo_video_gpu::YuvInput;
use std::{
    ffi::{c_char, c_void},
    ptr,
    sync::Arc,
};

const VERTEX: &str = "#version 300 es\nvoid main(){ vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2)); gl_Position=vec4(p*2.0-1.0,0,1); }";
const FRAGMENT: &str = "#version 300 es
#extension GL_EXT_YUV_target : require
precision highp float;
precision highp int;
uniform highp __samplerExternal2DY2YEXT source;
uniform mat4 transform;
uniform vec2 size;
uniform float maximum;
layout(location=0) out highp uint planeY;
layout(location=1) out highp uint planeU;
layout(location=2) out highp uint planeV;
void main(){
    vec2 uv=(transform*vec4(gl_FragCoord.xy/size,0,1)).xy;
    uvec3 value=uvec3(round(clamp(texture(source,uv).xyz,0.0,1.0)*maximum));
    planeY=value.x; planeU=value.y; planeV=value.z;
}";

pub(crate) struct RawYuv {
    device: Arc<wgpu::Device>,
    program: glow::Program,
}
impl RawYuv {
    pub fn new(device: Arc<wgpu::Device>) -> Result<Self, String> {
        // SAFETY: Android renderer exclusively creates GLES devices.
        let hal =
            unsafe { device.as_hal::<wgpu::hal::gles::Api>() }.ok_or("GLES device required")?;
        let gl = hal.context().lock();
        // SAFETY: Current context; shader objects remain local until successful link.
        let program = unsafe { compile(&gl)? };
        drop(gl);
        drop(hal);
        Ok(Self { device, program })
    }

    pub fn extract(
        &self,
        source: &DecoderSurface,
        descriptor: FrameDescriptor,
        matrix: [f32; 16],
        image: Option<ImageLease>,
    ) -> Result<(YuvInput, Option<ImportedImage>), String> {
        if !matches!(descriptor.bit_depth, 8 | 10) || (descriptor.bit_depth == 10) != source.p010 {
            return Err("opaque external texture does not prove a 10-bit buffer".into());
        }
        // SAFETY: A retained GLES device owns every external and destination texture.
        let hal = unsafe { self.device.as_hal::<wgpu::hal::gles::Api>() }
            .ok_or("GLES device required")?;
        let gl = hal.context().lock();
        let width = u32::try_from(descriptor.width).map_err(|_| "invalid video width")?;
        let height = u32::try_from(descriptor.height).map_err(|_| "invalid video height")?;
        let mut textures = Vec::with_capacity(3);
        // SAFETY: Exclusive current context, and RAII restores WGPU's cached GL state.
        let imported = unsafe {
            let _state = State::capture(&gl);
            let imported = image
                .map(|image| ImportedImage::new(self.device.clone(), &gl, source.texture, image))
                .transpose()?;
            let framebuffer = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            let result = (|| {
                for attachment in 0..3 {
                    let texture = gl.create_texture()?;
                    textures.push(texture);
                    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                    gl.tex_storage_2d(
                        glow::TEXTURE_2D,
                        1,
                        glow::R16UI,
                        width as i32,
                        height as i32,
                    );
                    gl.framebuffer_texture_2d(
                        glow::FRAMEBUFFER,
                        glow::COLOR_ATTACHMENT0 + attachment,
                        glow::TEXTURE_2D,
                        Some(texture),
                        0,
                    );
                }
                gl.draw_buffers(&[
                    glow::COLOR_ATTACHMENT0,
                    glow::COLOR_ATTACHMENT1,
                    glow::COLOR_ATTACHMENT2,
                ]);
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("raw YUV integer render target is incomplete".into());
                }
                for switch in [
                    glow::BLEND,
                    glow::DEPTH_TEST,
                    glow::STENCIL_TEST,
                    glow::SCISSOR_TEST,
                    glow::CULL_FACE,
                    glow::RASTERIZER_DISCARD,
                    glow::DITHER,
                ] {
                    gl.disable(switch);
                }
                gl.color_mask(true, true, true, true);
                gl.viewport(0, 0, width as i32, height as i32);
                gl.bind_vertex_array(None);
                gl.use_program(Some(self.program));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_sampler(0, None);
                gl.bind_texture(EXTERNAL, Some(source.texture));
                gl.uniform_1_i32(gl.get_uniform_location(self.program, "source").as_ref(), 0);
                gl.uniform_2_f32(
                    gl.get_uniform_location(self.program, "size").as_ref(),
                    width as f32,
                    height as f32,
                );
                gl.uniform_1_f32(
                    gl.get_uniform_location(self.program, "maximum").as_ref(),
                    ((1u32 << descriptor.bit_depth) - 1) as f32,
                );
                gl.uniform_matrix_4_f32_slice(
                    gl.get_uniform_location(self.program, "transform").as_ref(),
                    false,
                    &matrix,
                );
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                gl.memory_barrier(glow::TEXTURE_FETCH_BARRIER_BIT | glow::FRAMEBUFFER_BARRIER_BIT);
                gl.flush();
                let status = gl.get_error();
                if status != glow::NO_ERROR {
                    return Err(format!("raw YUV extraction failed: GL {status:#x}"));
                }
                Ok::<(), String>(())
            })();
            gl.delete_framebuffer(framebuffer);
            if let Err(error) = result {
                for texture in textures {
                    gl.delete_texture(texture);
                }
                return Err(error);
            }
            imported
        };
        // Do not enter WGPU while holding its context lock: an error/drop may
        // destroy HAL textures and require that same lock.
        drop(gl);
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let desc = wgpu::hal::TextureDescriptor {
            label: Some("android-raw-yuv-plane"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUses::RESOURCE,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: Vec::new(),
        };
        let mut planes = Vec::with_capacity(3);
        for texture in textures {
            // SAFETY: Exact size/format match the allocation. HAL receives sole
            // ownership of each name, and WGPU retains it until its GPU work retires.
            let plane = unsafe {
                let native = hal.texture_from_raw(texture.0, &desc, None);
                self.device.create_texture_from_hal::<wgpu::hal::gles::Api>(
                    native,
                    &wgpu::TextureDescriptor {
                        label: desc.label,
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: desc.dimension,
                        format: desc.format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                )
            };
            planes.push(plane);
        }
        Ok((
            YuvInput {
                planes: planes.try_into().map_err(|_| "invalid plane count")?,
                descriptor,
                chroma_divisor: [1, 1],
                interleaved: false,
                high_shift: 0,
                monochrome: false,
            },
            imported,
        ))
    }
}
impl Drop for RawYuv {
    fn drop(&mut self) {
        // SAFETY: Device/context retained for this program's lifetime.
        if let Some(hal) = unsafe { self.device.as_hal::<wgpu::hal::gles::Api>() } {
            let gl = hal.context().lock();
            unsafe {
                gl.delete_program(self.program);
            }
        }
    }
}

unsafe fn compile(gl: &glow::Context) -> Result<glow::Program, String> {
    // SAFETY: Called only with this GL context current.
    unsafe {
        let program = gl.create_program()?;
        for (kind, text) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, FRAGMENT),
        ] {
            let shader = match gl.create_shader(kind) {
                Ok(value) => value,
                Err(error) => {
                    gl.delete_program(program);
                    return Err(error);
                }
            };
            gl.shader_source(shader, text);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let error = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
                return Err(error);
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(error);
        }
        Ok(program)
    }
}

#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetCurrentDisplay() -> *mut c_void;
    fn eglGetCurrentContext() -> *mut c_void;
    fn eglGetProcAddress(name: *const c_char) -> Option<unsafe extern "C" fn()>;
}
#[link(name = "android")]
unsafe extern "C" {
    fn AHardwareBuffer_fromHardwareBuffer(
        env: *mut jni::sys::JNIEnv,
        object: jni::sys::jobject,
    ) -> *mut c_void;
}
type CreateImage =
    unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void, *const i32) -> *mut c_void;
type DestroyImage = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32;
type ClientBuffer = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type BindImage = unsafe extern "C" fn(u32, *mut c_void);

/// Both Android Image and HardwareBuffer, plus the imported EGL image, stay leased
/// until WGPU's submitted-work completion fence has retired every GPU read.
pub(crate) struct ImportedImage {
    device: Arc<wgpu::Device>,
    display: usize,
    context: usize,
    image: usize,
    destroy: DestroyImage,
    _lease: ImageLease,
}
impl ImportedImage {
    unsafe fn new(
        device: Arc<wgpu::Device>,
        gl: &glow::Context,
        texture: glow::Texture,
        lease: ImageLease,
    ) -> Result<Self, String> {
        // SAFETY: Entry points are used only when advertised and non-null. JNI's
        // borrowed AHardwareBuffer is retained by the Java Image/HardwareBuffer lease.
        unsafe {
            let client: ClientBuffer = std::mem::transmute(
                eglGetProcAddress(c"eglGetNativeClientBufferANDROID".as_ptr())
                    .ok_or("EGL native buffer import unavailable")?,
            );
            let create: CreateImage = std::mem::transmute(
                eglGetProcAddress(c"eglCreateImageKHR".as_ptr())
                    .ok_or("EGL image creation unavailable")?,
            );
            let destroy: DestroyImage = std::mem::transmute(
                eglGetProcAddress(c"eglDestroyImageKHR".as_ptr())
                    .ok_or("EGL image destruction unavailable")?,
            );
            let bind: BindImage = std::mem::transmute(
                eglGetProcAddress(c"glEGLImageTargetTexture2DOES".as_ptr())
                    .ok_or("GLES EGL image import unavailable")?,
            );
            let env = lease
                .vm
                .attach_current_thread()
                .map_err(crate::bridge::error)?;
            let buffer = AHardwareBuffer_fromHardwareBuffer(
                env.get_native_interface(),
                lease.buffer.as_obj().as_raw(),
            );
            if buffer.is_null() {
                return Err("P010 image has no AHardwareBuffer".into());
            }
            let display = eglGetCurrentDisplay();
            let image = create(
                display,
                ptr::null_mut(),
                0x3140,
                client(buffer),
                [0x3038].as_ptr(),
            );
            if image.is_null() {
                return Err("P010 EGLImage import failed".into());
            }
            drop(env);
            let imported = Self {
                device,
                display: display as usize,
                context: eglGetCurrentContext() as usize,
                image: image as usize,
                destroy,
                _lease: lease,
            };
            gl.bind_texture(EXTERNAL, Some(texture));
            bind(EXTERNAL, image);
            if gl.get_error() != glow::NO_ERROR {
                return Err("P010 raw external texture import failed".into());
            }
            Ok(imported)
        }
    }
}
impl Drop for ImportedImage {
    fn drop(&mut self) {
        // SAFETY: Completion-retained device owns the context; EGLImage has one owner.
        if unsafe { eglGetCurrentContext() as usize } == self.context {
            // Error cleanup may run inside the import's existing context lock.
            // The normal path already has WGPU's completion fence. This also
            // covers an extraction/conversion error before WGPU can submit one.
            // No pixel data is read back to the CPU.
            unsafe {
                glow_finish_current();
            }
            unsafe {
                (self.destroy)(self.display as *mut c_void, self.image as *mut c_void);
            }
        } else if let Some(hal) = unsafe { self.device.as_hal::<wgpu::hal::gles::Api>() } {
            let _gl = hal.context().lock();
            // Normally a no-op because the submission fence has already fired.
            // ImageReader does not know about our GPU reads when Image.close is
            // called, so error-path retirement must complete those reads too.
            unsafe {
                _gl.finish();
            }
            unsafe {
                (self.destroy)(self.display as *mut c_void, self.image as *mut c_void);
            }
        }
    }
}

unsafe fn glow_finish_current() {
    type Finish = unsafe extern "system" fn();
    // SAFETY: Called only under the existing GLES context lock. glFinish is a
    // core GLES function; using its checked EGL pointer avoids locking twice.
    if let Some(function) = unsafe { eglGetProcAddress(c"glFinish".as_ptr()) } {
        let finish: Finish = unsafe { std::mem::transmute(function) };
        unsafe {
            finish();
        }
    }
}
