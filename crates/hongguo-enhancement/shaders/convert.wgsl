@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output_image: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output_image);
    if (id.x >= size.x || id.y >= size.y) { return; }
    textureStore(output_image, id.xy, textureLoad(source, vec2i(id.xy), 0));
}
