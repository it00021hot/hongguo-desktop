import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);
const packageFile = require.resolve('anime4k-webgpu/package.json');
const packageRoot = path.dirname(packageFile);
const version = '1.0.0';
const metadata = JSON.parse(await readFile(packageFile, 'utf8'));
if (metadata.version !== version) throw new Error(`Expected anime4k-webgpu ${version}`);

globalThis.GPUTextureUsage = { TEXTURE_BINDING: 1, STORAGE_BINDING: 2, RENDER_ATTACHMENT: 4 };
globalThis.GPUShaderStage = { COMPUTE: 1, FRAGMENT: 2 };

function capture(width, height) {
  const shaders = [];
  const textures = [{ scale: [1, 1] }];
  const passes = [];
  const texture = (id, w, h) => ({ width: w, height: h, createView: () => ({ texture: id }) });
  const device = {
    createTexture({ size, format }) {
      if (format !== 'rgba16float') throw new Error(`Unexpected model format ${format}`);
      const scale = [size[0] / width, size[1] / height];
      if (!scale.every((value) => Number.isInteger(value) && value > 0)) {
        throw new Error('Non-integral model scale');
      }
      const id = textures.length;
      textures.push({ scale });
      return texture(id, size[0], size[1]);
    },
    createShaderModule({ code }) {
      const id = shaders.length;
      shaders.push(code.replaceAll('\r\n', '\n').replace(/[\t ]+$/gm, ''));
      return id;
    },
    createBindGroupLayout: (value) => value,
    createPipelineLayout: (value) => value,
    createBindGroup: ({ entries }) =>
      entries.map(({ binding, resource }) => ({ binding, ...resource })),
    createComputePipeline: ({ compute }) => ({
      kind: 'compute',
      shader: compute.module,
      entry: compute.entryPoint,
    }),
    createRenderPipeline: ({ vertex, fragment }) => ({
      kind: 'render',
      vertex: vertex.module,
      vertexEntry: vertex.entryPoint,
      fragment: fragment.module,
      fragmentEntry: fragment.entryPoint,
    }),
    createSampler: () => ({ sampler: true }),
  };
  const encoder = {
    beginComputePass: () => pass(),
    beginRenderPass: ({ colorAttachments }) => pass(colorAttachments[0].view.texture),
  };
  function pass(output) {
    const recorded = {};
    return {
      setPipeline: (pipeline) => Object.assign(recorded, pipeline),
      setBindGroup(index, bindings) {
        if (index !== 0) throw new Error('Unexpected model bind group');
        recorded.bindings = bindings;
      },
      dispatchWorkgroups(x, y) {
        const outputId = recorded.bindings.at(-1).texture;
        const scale = textures[outputId].scale;
        const divisor = [(width * scale[0]) / x, (height * scale[1]) / y];
        recorded.workgroup = divisor.map((value) => Math.ceil(value));
        output = outputId;
      },
      draw: (vertices) => {
        recorded.vertices = vertices;
      },
      end: () => passes.push({ ...recorded, output }),
    };
  }
  const { CNNx2M } = require('anime4k-webgpu');
  const model = new CNNx2M({ device, inputTexture: texture(0, width, height) });
  model.pass(encoder);
  return { shaders, textures, passes, output: textures.length - 1 };
}

const captured = capture(256, 256);
const repeat = capture(512, 512);
if (JSON.stringify(captured) !== JSON.stringify(repeat))
  throw new Error('Model graph depends on export resolution');
const sha256 = (value) => createHash('sha256').update(value).digest('hex');
const shaders = captured.shaders.map((code, index) => ({
  file: `${String(index).padStart(2, '0')}.wgsl`,
  sha256: sha256(code),
  code,
}));
const graph = {
  package: metadata.name,
  version,
  sourceSha256: sha256(await readFile(require.resolve('anime4k-webgpu'))),
  textures: captured.textures,
  passes: captured.passes,
  output: captured.output,
  shaders: shaders.map(({ file, sha256 }) => ({ file, sha256 })),
};
const destination = 'crates/hongguo-enhancement/shaders/anime4k';
const files = [
  ...shaders.map(({ file, code }) => [`${destination}/${file}`, code]),
  [`${destination}/graph.json`, `${JSON.stringify(graph, null, 2)}\n`],
  [`${destination}/LICENSE.md`, await readFile(path.join(packageRoot, 'LICENSE.md'), 'utf8')],
];
process.stdout.write('*** Begin Patch\n');
for (const [file, content] of files) {
  process.stdout.write(`*** Add File: ${file}\n`);
  process.stdout.write(
    content
      .replaceAll('\r\n', '\n')
      .replace(/\n$/, '')
      .split('\n')
      .map((line) => `+${line}`)
      .join('\n'),
  );
  process.stdout.write('\n');
}
process.stdout.write('*** End Patch\n');
