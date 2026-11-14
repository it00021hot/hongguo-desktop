Texture2D<uint> luma : register(t0);
Texture2D<uint2> chroma_u : register(t1);
Texture2D<uint> chroma_v : register(t2);
Texture2D<float4> color_lut : register(t3);
cbuffer Geometry : register(b0) {
    float4 crop;
    float4 dimensions;
    uint4 plane_layout;
    float4 chroma_position;
};
cbuffer Color : register(b1) { HgColorParams color_params; };

struct VertexOutput { float4 position : SV_Position; float2 uv : TEXCOORD0; };
VertexOutput vertex(uint index : SV_VertexID) {
    VertexOutput result;
    float2 uv = float2((index << 1) & 2, index & 2);
    result.uv = uv;
    result.position = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0, 1);
    return result;
}

float2 chroma_code(int2 at) {
    at = clamp(at, int2(0, 0), int2(dimensions.zw) - 1);
    uint2 code = chroma_u.Load(int3(at, 0)) >> plane_layout.y;
    return plane_layout.x == 1 ? float2(code) : float2(code.x, chroma_v.Load(int3(at, 0)));
}

struct PixelOutput { float4 mapped : SV_Target0; float4 working : SV_Target1; };
PixelOutput pixel(VertexOutput input) {
    float2 uv = input.uv;
    if (plane_layout.w == 1) uv = float2(uv.y, 1.0 - uv.x);
    else if (plane_layout.w == 2) uv = 1.0 - uv;
    else if (plane_layout.w == 3) uv = float2(1.0 - uv.y, uv.x);
    float2 source = crop.xy + uv * crop.zw - 0.5;
    int2 pixel_at = clamp(int2(floor(source + 0.5)), int2(0, 0), int2(dimensions.xy) - 1);
    float y = float(luma.Load(int3(pixel_at, 0)) >> plane_layout.y);
    float2 cbcr = float2(plane_layout.z, plane_layout.z);
    if (plane_layout.x != 2) {
        float2 at = (source - chroma_position.zw) / chroma_position.xy;
        int2 low = int2(floor(at));
        float2 weight = frac(at);
        cbcr = lerp(lerp(chroma_code(low), chroma_code(low + int2(1, 0)), weight.x),
                    lerp(chroma_code(low + int2(0, 1)), chroma_code(low + int2(1, 1)), weight.x), weight.y);
    }
    float3 working = hg_source_to_working(float3(y, cbcr), color_params, color_lut);
    PixelOutput output;
    output.working = float4(working, 1.0);
    output.mapped = float4(hg_working_to_output(working, color_params, color_lut), 1.0);
    if (color_params.display.z == 0.0) {
        uint hash = uint(input.position.x) * 1664525u + uint(input.position.y) * 1013904223u;
        hash ^= hash >> 16;
        output.mapped.rgb += (float(hash & 255u) / 256.0 - 0.5) / 255.0;
    }
    return output;
}
