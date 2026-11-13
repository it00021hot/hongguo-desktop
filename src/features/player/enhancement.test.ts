import { describe, expect, it } from 'vitest';
import { sharpenKernel, sharpenUniforms } from './enhancement';

describe('sharpenKernel', () => {
  it('强度 0 时是恒等核：不改变画面', () => {
    const k = sharpenKernel(0);
    k.forEach((v, i) => expect(v).toBeCloseTo(i === 4 ? 1 : 0));
  });

  it('满强度是标准 unsharp 形状：中心 1+4a=3.8、四邻 -0.7', () => {
    expect(sharpenKernel(100)).toEqual([0, -0.7, 0, -0.7, 3.8, -0.7, 0, -0.7, 0]);
  });

  it('越界强度被夹回 0–100', () => {
    expect(sharpenKernel(-5)).toEqual(sharpenKernel(0));
    expect(sharpenKernel(500)).toEqual(sharpenKernel(100));
  });

  it('核行列对称（卷积核不对称会产生方向性伪影）', () => {
    const k = sharpenKernel(60);
    expect(k).toEqual([k[8], k[7], k[6], k[5], k[4], k[3], k[2], k[1], k[0]]);
  });
});

describe('sharpen uniform buffer', () => {
  it('aligns vec2f texel at byte 8 and writes both axes of a non-square frame', () => {
    const uniforms = sharpenUniforms(0.35, 640, 360);
    expect(uniforms.byteLength).toBe(16);
    const bytes = new DataView(uniforms.buffer, uniforms.byteOffset, uniforms.byteLength);
    expect(bytes.getFloat32(0, true)).toBeCloseTo(0.35);
    expect(bytes.getUint32(4, true)).toBe(0);
    expect(bytes.getFloat32(8, true) * 640).toBeCloseTo(1);
    expect(bytes.getFloat32(12, true) * 360).toBeCloseTo(1);
  });
});
