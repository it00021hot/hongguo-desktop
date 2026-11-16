struct HgColorParams {
    range: vec4f,
    yuv: array<vec4f, 3>,
    source_to_display: array<vec4f, 3>,
    display_to_output: array<vec4f, 3>,
    output_to_display: array<vec4f, 3>,
    display: vec4f,
    source_luma: vec4f,
    transfer: vec4f,
}

fn hg_lookup(lut: texture_2d<f32>, coordinate: f32) -> vec4f {
    let index = clamp(coordinate, 0.0, 1.0) * 4095.0;
    let low = i32(index);
    let high = min(low + 1, 4095);
    return mix(textureLoad(lut, vec2i(low % 64, low / 64), 0),
        textureLoad(lut, vec2i(high % 64, high / 64), 0), fract(index));
}

fn hg_matrix(rows: array<vec4f, 3>, value: vec3f) -> vec3f {
    return vec3f(dot(rows[0].xyz, value), dot(rows[1].xyz, value), dot(rows[2].xyz, value));
}

fn hg_source_to_working(code: vec3f, p: HgColorParams, lut: texture_2d<f32>) -> vec3f {
    let yuv = vec3f((code.x - p.range.x) / p.range.y, (code.yz - p.range.z) / p.range.w);
    let signal = hg_matrix(p.yuv, yuv);
    var linear = vec3f(hg_lookup(lut, signal.r).r, hg_lookup(lut, signal.g).r, hg_lookup(lut, signal.b).r);
    if p.transfer.x != 0.0 {
        let scene_luma = max(dot(linear, p.source_luma.xyz), 0.0);
        linear *= pow(scene_luma, p.source_luma.w - 1.0) * p.transfer.y;
    }
    let display = hg_matrix(p.source_to_display, linear);
    return hg_matrix(p.display_to_output, display) / p.display.w;
}

fn hg_working_to_output(working: vec3f, p: HgColorParams, lut: texture_2d<f32>) -> vec3f {
    var display = hg_matrix(p.output_to_display, working * p.display.w);
    let peak = max(max(display.r, display.g), max(display.b, 0.0));
    let mapped = hg_lookup(lut, log2(1.0 + peak) * p.transfer.z).g;
    if peak > 0.0 { display *= mapped / peak; }
    let luma = dot(display, vec3f(p.display.xy, 1.0 - p.display.x - p.display.y));
    let low = min(min(display.r, display.g), min(display.b, 0.0));
    if low < 0.0 && luma > 0.0 && p.transfer.w != 0.0 { display = vec3f(luma) + (display - luma) * luma / (luma - low); }
    let output = hg_matrix(p.display_to_output, display);
    if p.display.z != 0.0 { return output / p.display.w; }
    let coordinates = log2(vec3f(1.0) + max(output, vec3f(0.0))) * p.transfer.z;
    return vec3f(hg_lookup(lut, coordinates.r).b, hg_lookup(lut, coordinates.g).b, hg_lookup(lut, coordinates.b).b);
}

fn hg_color(code: vec3f, p: HgColorParams, lut: texture_2d<f32>) -> vec3f {
    return hg_working_to_output(hg_source_to_working(code, p, lut), p, lut);
}
