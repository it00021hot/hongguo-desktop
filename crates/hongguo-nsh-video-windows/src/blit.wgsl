@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var imageSampler: sampler;
@group(0) @binding(2) var<uniform> color: HgColorParams;
@group(0) @binding(3) var colorLut: texture_2d<f32>;
struct VertexOut { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> VertexOut {
    var result: VertexOut;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    result.uv = uv;
    result.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return result;
}
@fragment fn fragment(input: VertexOut) -> @location(0) vec4<f32> {
    let working = textureSampleLevel(image, imageSampler, input.uv, 0.0);
    var mapped = hg_working_to_output(working.rgb, color, colorLut);
    if color.display.z == 0.0 {
        var hash = u32(input.position.x) * 1664525u + u32(input.position.y) * 1013904223u;
        hash ^= hash >> 16u;
        mapped += vec3<f32>((f32(hash & 255u) / 256.0 - 0.5) / 255.0);
    }
    return vec4<f32>(mapped, working.a);
}
