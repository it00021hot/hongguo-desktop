@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_image: texture_storage_2d<rgba16float, write>;

fn linear(value: f32) -> f32 {
    let signal = abs(value);
    return sign(value) * select(pow((signal + 0.055) / 1.055, 2.4), signal / 12.92, signal <= 0.04045);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    if any(id.xy >= textureDimensions(output_image)) { return; }
    let input = textureLoad(source, vec2i(id.xy), 0);
    textureStore(output_image, id.xy, vec4f(linear(input.r), linear(input.g), linear(input.b), input.a));
}
