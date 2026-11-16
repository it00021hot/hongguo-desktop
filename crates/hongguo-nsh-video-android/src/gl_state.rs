use glow::HasContext;

pub(crate) const EXTERNAL: u32 = 0x8d65;
const EXTERNAL_BINDING: u32 = 0x8d67;

/// Restore every state touched outside WGPU; its GLES backend caches this state.
pub(crate) struct State<'a> {
    gl: &'a glow::Context,
    program: Option<glow::Program>,
    draw: Option<glow::Framebuffer>,
    read: Option<glow::Framebuffer>,
    vao: Option<glow::VertexArray>,
    active: u32,
    texture: Option<glow::Texture>,
    external: Option<glow::Texture>,
    sampler: Option<glow::Sampler>,
    viewport: [i32; 4],
    mask: [i32; 4],
    switches: [(u32, bool); 7],
}

impl<'a> State<'a> {
    // SAFETY: All calls run with this device's EGL context locked and current.
    pub unsafe fn capture(gl: &'a glow::Context) -> Self {
        unsafe {
            let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
            gl.active_texture(glow::TEXTURE0);
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut mask = [0; 4];
            gl.get_parameter_i32_slice(glow::COLOR_WRITEMASK, &mut mask);
            Self {
                gl,
                program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
                draw: gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                read: gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
                vao: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
                active,
                texture: gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
                external: gl.get_parameter_texture(EXTERNAL_BINDING),
                sampler: gl.get_parameter_sampler(glow::SAMPLER_BINDING),
                viewport,
                mask,
                switches: [
                    glow::BLEND,
                    glow::DEPTH_TEST,
                    glow::STENCIL_TEST,
                    glow::SCISSOR_TEST,
                    glow::CULL_FACE,
                    glow::RASTERIZER_DISCARD,
                    glow::DITHER,
                ]
                .map(|key| (key, gl.is_enabled(key))),
            }
        }
    }
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        // SAFETY: State is scoped inside the caller's EGL context lock.
        unsafe {
            let gl = self.gl;
            gl.use_program(self.program);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read);
            gl.bind_vertex_array(self.vao);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.bind_texture(EXTERNAL, self.external);
            gl.bind_sampler(0, self.sampler);
            gl.active_texture(self.active);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.color_mask(
                self.mask[0] != 0,
                self.mask[1] != 0,
                self.mask[2] != 0,
                self.mask[3] != 0,
            );
            for (key, enabled) in self.switches {
                if enabled {
                    gl.enable(key);
                } else {
                    gl.disable(key);
                }
            }
        }
    }
}
