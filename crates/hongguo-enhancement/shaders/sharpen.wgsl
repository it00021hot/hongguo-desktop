struct Params { value: vec4f }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_image: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: Params;

fn pixel(at: vec2i) -> vec4f {
    return textureLoad(source, clamp(at, vec2i(0), vec2i(textureDimensions(source)) - vec2i(1)), 0);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output_image);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let p = vec2i(id.xy);
    let center = pixel(p);
    let neighbors = pixel(p + vec2i(-1, 0)) + pixel(p + vec2i(1, 0))
        + pixel(p + vec2i(0, -1)) + pixel(p + vec2i(0, 1));
    let amount = params.value.x;
    let color = clamp(center.rgb * (1.0 + 4.0 * amount) - neighbors.rgb * amount, vec3f(0), vec3f(1));
    textureStore(output_image, id.xy, vec4f(color, center.a));
}
