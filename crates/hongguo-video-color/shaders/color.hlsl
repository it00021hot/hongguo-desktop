struct HgColorParams {
    float4 range;
    float4 yuv[3];
    float4 source_to_display[3];
    float4 display_to_output[3];
    float4 output_to_display[3];
    float4 display;
    float4 source_luma;
    float4 transfer;
};

float4 hg_lookup(Texture2D<float4> lut, float coordinate) {
    float index = clamp(coordinate, 0.0, 1.0) * 4095.0;
    int low = (int)index;
    int high = min(low + 1, 4095);
    return lerp(lut.Load(int3(low % 64, low / 64, 0)), lut.Load(int3(high % 64, high / 64, 0)), frac(index));
}

float3 hg_matrix(float4 rows[3], float3 value) {
    return float3(dot(rows[0].xyz, value), dot(rows[1].xyz, value), dot(rows[2].xyz, value));
}

float3 hg_source_to_working(float3 code, HgColorParams p, Texture2D<float4> lut) {
    float3 yuv = float3((code.x - p.range.x) / p.range.y, (code.yz - p.range.z) / p.range.w);
    float3 signal = hg_matrix(p.yuv, yuv);
    float3 scene_linear = float3(hg_lookup(lut, signal.r).r, hg_lookup(lut, signal.g).r, hg_lookup(lut, signal.b).r);
    if (p.transfer.x != 0.0) {
        float scene_luma = max(dot(scene_linear, p.source_luma.xyz), 0.0);
        scene_linear *= pow(scene_luma, p.source_luma.w - 1.0) * p.transfer.y;
    }
    float3 display = hg_matrix(p.source_to_display, scene_linear);
    return hg_matrix(p.display_to_output, display) / p.display.w;
}

float3 hg_working_to_output(float3 working, HgColorParams p, Texture2D<float4> lut) {
    float3 display = hg_matrix(p.output_to_display, working * p.display.w);
    float peak = max(max(display.r, display.g), max(display.b, 0.0));
    float mapped = hg_lookup(lut, log2(1.0 + peak) * p.transfer.z).g;
    if (peak > 0.0) display *= mapped / peak;
    float luma = dot(display, float3(p.display.xy, 1.0 - p.display.x - p.display.y));
    float low = min(min(display.r, display.g), min(display.b, 0.0));
    if (low < 0.0 && luma > 0.0 && p.transfer.w != 0.0) display = luma + (display - luma) * luma / (luma - low);
    float3 output = hg_matrix(p.display_to_output, display);
    if (p.display.z != 0.0) return output / p.display.w;
    float3 coordinates = log2(1.0 + max(output, 0.0)) * p.transfer.z;
    return float3(hg_lookup(lut, coordinates.r).b, hg_lookup(lut, coordinates.g).b, hg_lookup(lut, coordinates.b).b);
}

float3 hg_color(float3 code, HgColorParams p, Texture2D<float4> lut) {
    return hg_working_to_output(hg_source_to_working(code, p, lut), p, lut);
}
