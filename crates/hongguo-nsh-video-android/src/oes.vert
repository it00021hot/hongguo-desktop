#version 300 es
precision highp float;
out vec2 frameUv;
void main() {
    vec2 uv = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    // GL framebuffer row zero is WGPU texture row zero (the image's top).
    // Normalize that origin before applying the decoder's SurfaceTexture matrix.
    frameUv = vec2(uv.x, 1.0 - uv.y);
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}
