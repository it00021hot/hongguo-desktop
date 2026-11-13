/**
 * 画质增强核心：锐化核与 Anime4K WebGPU 渲染器（纯逻辑，无 React）。显示层叠加，不碰播放管线、不改下载文件；
 * 失败纪律：任何一环失败都以 EnhancementError(code) 抛出，由叠加层统一映射 i18n 文案并退回原画。
 */

/** 增强失败的稳定错误码（叠加层映射词条；本模块不硬编码人类语言）。 */
export type EnhancementErrorCode =
  | 'noWebgpu' // 运行环境未开放 WebGPU（旧 WKWebView 等）
  | 'noAdapter' // 拿不到显卡适配器
  | 'gpuUnsupported' // validation / device lost / uncaptured error——显卡跑不动这套运算
  | 'inputTooLarge' // 输入超过 1080p（显存与耗时上限）
  | 'canvasUnavailable' // canvas 拿不到 webgpu 上下文
  | 'slow' // 连续慢帧，处理跟不上播放节奏
  | 'restricted' // 片源限制画面处理（跨源 taint，换任何路径都一样）
  | 'sizeChanged' // 播放中分辨率变化，需重建渲染器
  | 'notReady' // 视频画面尚未就绪（无尺寸）
  | 'generic';

export class EnhancementError extends Error {
  constructor(
    readonly code: EnhancementErrorCode,
    cause?: unknown,
  ) {
    super(`enhancement: ${code}`, { cause });
    this.name = 'EnhancementError';
  }
}

/** 锐化强度（0–100）→ 3×3 unsharp 卷积核；amount 上限 0.7（再高振铃从「更锐」劣化成「更脏」）。 */
export function sharpenKernel(strength: number): number[] {
  const amount = (Math.min(Math.max(strength, 0), 100) / 100) * 0.7;
  return [0, -amount, 0, -amount, 1 + 4 * amount, -amount, 0, -amount, 0];
}

/** Anime4K 输入像素上限：1080p。更大输入的显存与耗时不可控，先守住。 */
const MAX_INPUT_PIXELS = 1920 * 1080;
/** 单帧预算（ms）：超过记一次慢帧，正常帧清零。 */
const SLOW_FRAME_MS = 150;
/** 连续慢帧上限：低端显卡硬扛只会卡成幻灯片，到限主动放弃退原画。 */
const SLOW_FRAME_LIMIT = 5;
/** 显示 pass：全屏三角形 + 纹理采样，附可选 3×3 unsharp 锐化（与 SVG 档同一套核）——Anime4K 输出偏软，锐化叠在 2× 输出上一次采样完成。 */
const displayShader = `
struct Params { amount: f32, texel: vec2f }
struct Vertex { @builtin(position) position: vec4f, @location(0) uv: vec2f }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
  let positions = array<vec2f, 3>(vec2f(-1, -1), vec2f(3, -1), vec2f(-1, 3));
  let p = positions[index];
  return Vertex(vec4f(p, 0, 1), vec2f((p.x + 1) * 0.5, (1 - p.y) * 0.5));
}
@group(0) @binding(0) var imageSampler: sampler;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;
@fragment fn fragment(input: Vertex) -> @location(0) vec4f {
  let c = textureSample(image, imageSampler, input.uv).rgb;
  if (params.amount <= 0.0) { return vec4f(c, 1.0); }
  let l = textureSample(image, imageSampler, input.uv + vec2f(-params.texel.x, 0.0)).rgb;
  let r = textureSample(image, imageSampler, input.uv + vec2f(params.texel.x, 0.0)).rgb;
  let t = textureSample(image, imageSampler, input.uv + vec2f(0.0, -params.texel.y)).rgb;
  let b = textureSample(image, imageSampler, input.uv + vec2f(0.0, params.texel.y)).rgb;
  let sharp = c * (1.0 + 4.0 * params.amount) - (l + r + t + b) * params.amount;
  return vec4f(clamp(sharp, vec3f(0.0), vec3f(1.0)), 1.0);
}`;

export interface Anime4KRenderer {
  dispose(): void;
  /** 渲一帧。返回 false 表示这一帧没有画（未就绪/被跳过）。 */
  draw(): Promise<boolean>;
  /** 实时调锐化强度（0–100），写 uniform 不重建管线。 */
  setSharpen(strength: number): void;
}

export interface CreateRendererOptions {
  /** 生命周期信号：源切换/关档时 abort，渲染器随之释放全部 GPU 资源 */
  signal: AbortSignal;
  /** 设备级异步失败（device lost / uncaptured error）的上报口 */
  onError: (error: EnhancementError) => void;
  /** 初始锐化强度 0–100（Anime4K 与锐化同开时由显示 pass 应用） */
  sharpen?: number;
}

/** 锐化 UI 强度 → 卷积核 amount（与 sharpenKernel 同一映射，上限 0.7 防振铃）。 */
function sharpenAmountOf(strength: number): number {
  const clamped = Math.min(Math.max(strength, 0), 100);
  return (clamped / 100) * 0.7;
}

export function sharpenUniforms(amount: number, width: number, height: number): Float32Array {
  return new Float32Array([amount, 0, 1 / width, 1 / height]);
}

/**
 * 建 Anime4K CNNx2M 2× 渲染器：device 独占（源切换时全部库纹理随之销毁），输入 rgba16float（CNN 中间值精度），输出 2× 尺寸；
 * 建管线包在 validation error scope 里——不支持的显卡在这里显式失败而不是画出黑帧。
 */
export async function createAnime4KRenderer(
  source: HTMLCanvasElement,
  canvas: HTMLCanvasElement,
  { signal, onError, sharpen = 0 }: CreateRendererOptions,
): Promise<Anime4KRenderer> {
  if (!navigator.gpu) throw new EnhancementError('noWebgpu');
  const { width, height } = source;
  if (!width || !height) throw new EnhancementError('notReady');
  if (width * height > MAX_INPUT_PIXELS) throw new EnhancementError('inputTooLarge');
  const { CNNx2M } = await import('anime4k-webgpu');
  signal.throwIfAborted();
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
  signal.throwIfAborted();
  if (!adapter) throw new EnhancementError('noAdapter');
  const device = await adapter.requestDevice();

  let context: GPUCanvasContext | null = null;
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal.removeEventListener('abort', dispose);
    context?.unconfigure();
    device.destroy();
  };
  if (signal.aborted) {
    dispose();
    signal.throwIfAborted();
  }
  signal.addEventListener('abort', dispose, { once: true });
  // 设备级异步失败在 draw 循环之外发生（驱动重置等），上报由叠加层收口
  const report = (error: EnhancementError) => {
    if (!disposed) onError(error);
  };
  void device.lost.then((info) => report(new EnhancementError('gpuUnsupported', info.reason)));
  device.addEventListener('uncapturederror', (event) => {
    event.preventDefault();
    report(new EnhancementError('gpuUnsupported', event.error));
  });

  try {
    const ctx = canvas.getContext('webgpu');
    if (!ctx) throw new EnhancementError('canvasUnavailable');
    context = ctx;
    device.pushErrorScope('validation');
    const input = device.createTexture({
      size: [width, height],
      format: 'rgba16float',
      usage:
        GPUTextureUsage.TEXTURE_BINDING |
        GPUTextureUsage.COPY_DST |
        GPUTextureUsage.RENDER_ATTACHMENT,
    });
    const anime = new CNNx2M({ device, inputTexture: input });
    const output = anime.getOutputTexture();
    canvas.width = output.width;
    canvas.height = output.height;
    const format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: 'opaque' });
    const module = device.createShaderModule({ code: displayShader });
    const pipeline = device.createRenderPipeline({
      layout: 'auto',
      vertex: { module, entryPoint: 'vertex' },
      fragment: { module, entryPoint: 'fragment', targets: [{ format }] },
      primitive: { topology: 'triangle-list' },
    });
    // 锐化参数 uniform（amount + 2× 输出的纹元尺寸；对齐 vec2f 后 16 字节）
    const uniform = device.createBuffer({
      size: 16,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });
    const writeSharpen = (amount: number) => {
      device.queue.writeBuffer(uniform, 0, sharpenUniforms(amount, output.width, output.height));
    };
    writeSharpen(sharpenAmountOf(sharpen));
    const bindings = device.createBindGroup({
      layout: pipeline.getBindGroupLayout(0),
      entries: [
        {
          binding: 0,
          resource: device.createSampler({ magFilter: 'linear', minFilter: 'linear' }),
        },
        { binding: 1, resource: output.createView() },
        { binding: 2, resource: { buffer: uniform } },
      ],
    });
    const validationError = await device.popErrorScope();
    signal.throwIfAborted();
    if (validationError) throw new EnhancementError('gpuUnsupported', validationError.message);

    let slowFrames = 0;
    // 冷启动头几帧要现编译 shader + 驱动管线优化，几乎必然慢——直接判死会把
    // warm-up 尖峰误杀成设备不支持（第一次点必退级、第二次才成功）。给一次宽限，真慢的设备第二轮照旧计满判死
    let slowGraceUsed = false;
    return {
      dispose,
      setSharpen(strength: number) {
        if (disposed) return;
        writeSharpen(sharpenAmountOf(strength));
      },
      async draw() {
        if (disposed) {
          return false;
        }
        const { width: frameWidth, height: frameHeight } = source;
        if (frameWidth !== width || frameHeight !== height) {
          throw new EnhancementError('sizeChanged');
        }
        const started = performance.now();
        try {
          device.queue.copyExternalImageToTexture({ source }, { texture: input }, [width, height]);
        } catch (error) {
          if (error instanceof DOMException && error.name === 'SecurityError') {
            throw new EnhancementError('restricted', error);
          }
          throw error;
        }
        const encoder = device.createCommandEncoder();
        anime.pass(encoder);
        const pass = encoder.beginRenderPass({
          colorAttachments: [
            {
              view: ctx.getCurrentTexture().createView(),
              clearValue: { r: 0, g: 0, b: 0, a: 1 },
              loadOp: 'clear',
              storeOp: 'store',
            },
          ],
        });
        pass.setPipeline(pipeline);
        pass.setBindGroup(0, bindings);
        pass.draw(3);
        pass.end();
        device.queue.submit([encoder.finish()]);
        // 等真实 GPU 耗时落地再判慢帧，否则低端卡会连续超时后才被察觉
        await device.queue.onSubmittedWorkDone();
        const frameMs = performance.now() - started;
        slowFrames = frameMs > SLOW_FRAME_MS ? slowFrames + 1 : 0;
        if (slowFrames >= SLOW_FRAME_LIMIT) {
          if (!slowGraceUsed) {
            slowGraceUsed = true;
            slowFrames = 0;
          } else {
            // 耗时进 cause 供排查（fail 路径会打出），用户看到的仍是固定文案
            throw new EnhancementError('slow', `frame ${Math.round(frameMs)}ms`);
          }
        }
        return !disposed;
      },
    };
  } catch (error) {
    dispose();
    throw error;
  }
}
