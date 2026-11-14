struct Geometry { crop: vec4f, chroma: vec4f, options: vec4f }
@group(0) @binding(0) var y_plane: texture_2d<u32>;
@group(0) @binding(1) var u_plane: texture_2d<u32>;
@group(0) @binding(2) var v_plane: texture_2d<u32>;
@group(0) @binding(3) var output_image: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var<uniform> color: HgColorParams;
@group(0) @binding(5) var color_lut: texture_2d<f32>;
@group(0) @binding(6) var<uniform> geometry: Geometry;

fn load(plane: texture_2d<u32>, at: vec2i, channel: u32) -> f32 {
    let point = clamp(at, vec2i(0), vec2i(textureDimensions(plane)) - 1);
    let value = textureLoad(plane, point, 0)[channel] >> u32(geometry.options.z);
    return f32(value);
}

fn sample(plane: texture_2d<u32>, point: vec2f, channel: u32) -> f32 {
    let low = vec2i(floor(point));
    let weight = fract(point);
    return mix(mix(load(plane, low, channel), load(plane, low + vec2i(1, 0), channel), weight.x),
        mix(load(plane, low + vec2i(0, 1), channel), load(plane, low + vec2i(1, 1), channel), weight.x), weight.y);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3u) {
    let size = textureDimensions(output_image);
    if any(id.xy >= size) { return; }
    var uv = (vec2f(id.xy) + 0.5) / vec2f(size);
    let rotation = u32(geometry.options.x);
    if rotation == 90u { uv = vec2f(uv.y, 1.0 - uv.x); }
    else if rotation == 180u { uv = vec2f(1.0) - uv; }
    else if rotation == 270u { uv = vec2f(1.0 - uv.y, uv.x); }
    let pixel = geometry.crop.xy + uv * geometry.crop.zw - 0.5;
    let chroma_pixel = (pixel - geometry.chroma.zw) / geometry.chroma.xy;
    let y = sample(y_plane, pixel, 0u);
    var u = color.range.z;
    var v = color.range.z;
    if geometry.options.w == 0.0 {
        u = sample(u_plane, chroma_pixel, 0u);
        v = sample(v_plane, chroma_pixel, select(0u, 1u, geometry.options.y != 0.0));
    }
    textureStore(output_image, vec2i(id.xy), vec4f(hg_source_to_working(vec3f(y, u, v), color, color_lut), 1.0));
}
