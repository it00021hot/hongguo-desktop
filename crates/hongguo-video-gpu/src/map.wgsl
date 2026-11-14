@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_image: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> color: HgColorParams;
@group(0) @binding(3) var color_lut: texture_2d<f32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3u) {
    if any(id.xy >= textureDimensions(output_image)) { return; }
    let input = textureLoad(source, vec2i(id.xy), 0);
    textureStore(output_image, vec2i(id.xy), vec4f(hg_working_to_output(input.rgb, color, color_lut), input.a));
}
