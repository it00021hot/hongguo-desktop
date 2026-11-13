use glow::HasContext;

// Khronos GL_OES_EGL_image_external token; never a regular texture_2d input.
pub(crate) const EXTERNAL_OES: u32 = 0x8D65;

#[derive(Default)]
pub(crate) struct Conversion {
    pub external: Option<glow::NativeTexture>,
    pub rgba: Option<glow::NativeTexture>,
    framebuffer: Option<glow::NativeFramebuffer>,
    vertices: Option<glow::NativeVertexArray>,
    program: Option<glow::NativeProgram>,
}

impl Conversion {
    pub fn new(gl: &glow::Context, width: i32, height: i32) -> Result<Self, String> {
        if !gl
            .supported_extensions()
            .contains("GL_OES_EGL_image_external_essl3")
        {
            return Err("GLES external-OES ESSL3 sampling is unavailable".into());
        }
        let mut state = Self::default();
        // SAFETY: The owning WGPU adapter lock has made this context current.
        let result = unsafe {
            (|| {
                state.program = Some(program(gl)?);
                state.external = Some(gl.create_texture()?);
                gl.bind_texture(EXTERNAL_OES, state.external);
                gl.tex_parameter_i32(EXTERNAL_OES, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(EXTERNAL_OES, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(
                    EXTERNAL_OES,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    EXTERNAL_OES,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                state.rgba = Some(gl.create_texture()?);
                gl.bind_texture(glow::TEXTURE_2D, state.rgba);
                gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::RGBA8, width, height);
                state.framebuffer = Some(gl.create_framebuffer()?);
                gl.bind_framebuffer(glow::FRAMEBUFFER, state.framebuffer);
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    state.rgba,
                    0,
                );
                if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                    return Err("GLES RGBA conversion framebuffer is incomplete".into());
                }
                gl.disable(glow::SCISSOR_TEST);
                gl.color_mask(true, true, true, true);
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                state.vertices = Some(gl.create_vertex_array()?);
                let error = gl.get_error();
                if error != glow::NO_ERROR {
                    return Err(format!("GLES video initialization failed: 0x{error:x}"));
                }
                Ok(())
            })()
        };
        if let Err(error) = result {
            state.destroy(gl);
            return Err(error);
        }
        Ok(state)
    }

    pub fn draw(
        &self,
        gl: &glow::Context,
        width: i32,
        height: i32,
        matrix: &[f32; 16],
    ) -> Result<(), String> {
        // SAFETY: All objects belong to this currently locked EGL context and remain live through draw.
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, self.framebuffer);
            gl.viewport(0, 0, width, height);
            for capability in [
                glow::SCISSOR_TEST,
                glow::DEPTH_TEST,
                glow::STENCIL_TEST,
                glow::BLEND,
                glow::CULL_FACE,
            ] {
                gl.disable(capability);
            }
            gl.color_mask(true, true, true, true);
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertices);
            gl.active_texture(glow::TEXTURE0);
            // WGPU's previous presentation sampler uses mipmap filtering.
            // External-OES has no mip chain: that sampler makes later decoder
            // images incomplete/black even though the first frame was valid.
            gl.bind_sampler(0, None);
            gl.bind_texture(EXTERNAL_OES, self.external);
            let program = self.program.expect("conversion program initialized");
            gl.uniform_1_i32(gl.get_uniform_location(program, "sourceFrame").as_ref(), 0);
            gl.uniform_matrix_4_f32_slice(
                gl.get_uniform_location(program, "sourceTransform").as_ref(),
                false,
                matrix,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_texture(EXTERNAL_OES, None);
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!(
                    "GLES external frame conversion failed: 0x{error:x}"
                ));
            }
        }
        // WGPU GLES resets its state at the start of every command buffer, explicitly
        // supporting external commands on the same adapter context (queue.rs::submit).
        Ok(())
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        // SAFETY: Called only with the owning context locked. Each Option is consumed once.
        unsafe {
            if let Some(framebuffer) = self.framebuffer.take() {
                gl.delete_framebuffer(framebuffer);
            }
            if let Some(vertices) = self.vertices.take() {
                gl.delete_vertex_array(vertices);
            }
            if let Some(program) = self.program.take() {
                gl.delete_program(program);
            }
            if let Some(texture) = self.external.take() {
                gl.delete_texture(texture);
            }
            if let Some(texture) = self.rgba.take() {
                gl.delete_texture(texture);
            }
        }
    }
}

unsafe fn shader(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> Result<glow::NativeShader, String> {
    // SAFETY: This helper is used only while the owning context is current.
    unsafe {
        let shader = gl.create_shader(kind)?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if gl.get_shader_compile_status(shader) {
            Ok(shader)
        } else {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            Err(error)
        }
    }
}

unsafe fn program(gl: &glow::Context) -> Result<glow::NativeProgram, String> {
    // SAFETY: The caller holds the context; intermediate shaders are always deleted.
    unsafe {
        let vertex = shader(gl, glow::VERTEX_SHADER, include_str!("oes.vert"))?;
        let fragment = match shader(gl, glow::FRAGMENT_SHADER, include_str!("oes.frag")) {
            Ok(fragment) => fragment,
            Err(error) => {
                gl.delete_shader(vertex);
                return Err(error);
            }
        };
        let program = match gl.create_program() {
            Ok(program) => program,
            Err(error) => {
                gl.delete_shader(vertex);
                gl.delete_shader(fragment);
                return Err(error);
            }
        };
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.link_program(program);
        gl.detach_shader(program, vertex);
        gl.detach_shader(program, fragment);
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        if gl.get_program_link_status(program) {
            Ok(program)
        } else {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            Err(error)
        }
    }
}
