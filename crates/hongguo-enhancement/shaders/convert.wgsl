@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_image: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: vec4f;

fn perceptual(value: f32) -> f32 {
    let linear = abs(value);
    return sign(value) * select(1.055 * pow(linear, 1.0 / 2.4) - 0.055, linear * 12.92, linear <= 0.0031308);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output_image);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let input = textureLoad(source, vec2i(id.xy), 0);
    var color = input.rgb;
    if params.y != 0.0 { color = vec3f(perceptual(color.r), perceptual(color.g), perceptual(color.b)); }
    textureStore(output_image, id.xy, vec4f(color, input.a));
}
