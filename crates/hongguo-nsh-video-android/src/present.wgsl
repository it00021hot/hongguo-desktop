struct Fit { scale: vec2f, offset: vec2f, output: vec4f }
struct Vertex { @builtin(position) position: vec4f, @location(0) uv: vec2f }
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var<uniform> fit: Fit;

@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array<vec2f, 3>(vec2f(-1, -1), vec2f(3, -1), vec2f(-1, 3));
    let p = positions[index];
    return Vertex(vec4f(p, 0, 1), vec2f((p.x + 1) * 0.5, (1 - p.y) * 0.5));
}
@fragment fn fragment(input: Vertex) -> @location(0) vec4f {
    let uv = input.uv * fit.scale + fit.offset;
    if any(uv < vec2f(0)) || any(uv > vec2f(1)) { return vec4f(0, 0, 0, 1); }
    let color = textureSampleLevel(image, image_sampler, uv, 0).rgb;
    if fit.output.x > 0.5 { return vec4f(color, 1); }
    let noise = fract(sin(dot(floor(input.position.xy), vec2f(12.9898, 78.233))) * 43758.5453) - 0.5;
    return vec4f(clamp(color + vec3f(noise / 255.0), vec3f(0), vec3f(1)), 1);
}
