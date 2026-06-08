//! WebGPU API stub (W3C WebGPU spec).
//!
//! Exposes `navigator.gpu` as a `GPU` object:
//! - `navigator.gpu.requestAdapter(opts?)` → `Promise<GPUAdapter>`
//! - `adapter.requestDevice(desc?)` → `Promise<GPUDevice>`
//! - `GPUDevice`: `createBuffer`, `createTexture`, `createRenderPipeline`,
//!   `createCommandEncoder`, `createShaderModule`, `createSampler`,
//!   `createBindGroup`, `createBindGroupLayout`, `createPipelineLayout`,
//!   `createComputePipeline`, `queue` (GPUQueue).
//! - `GPUBuffer`: `mapAsync`, `getMappedRange`, `unmap`, `destroy`.
//! - `GPUTexture`: `createView`, `destroy`.
//! - `GPURenderPipeline`, `GPUComputePipeline`: opaque stubs.
//! - `GPUCommandEncoder`: `beginRenderPass`, `beginComputePass`, `copyBufferToBuffer`,
//!   `copyTextureToBuffer`, `finish`.
//! - `GPURenderPassEncoder`: `setPipeline`, `setVertexBuffer`, `setIndexBuffer`,
//!   `setBindGroup`, `draw`, `drawIndexed`, `end`.
//! - `GPUComputePassEncoder`: `setPipeline`, `setBindGroup`, `dispatchWorkgroups`, `end`.
//! - `GPUQueue`: `submit`, `writeBuffer`, `writeTexture`.
//! - `GPUCanvasContext`: `configure`, `getCurrentTexture`, `unconfigure`.
//!
//! **Phase 0**: no GPU — all operations in-memory only.  All `create*` calls
//! return opaque stub objects; `submit`/`draw`/`dispatch` are no-ops.
//! Phase 1 (future): wire to `wgpu` backend.

use rquickjs::Ctx;

/// Install the WebGPU API bindings into the JS context.
pub fn install_webgpu_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBGPU_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C WebGPU (Phase 0).
const WEBGPU_SHIM: &str = r#"(function() {
  'use strict';

  // ── GPUSupportedFeatures ─────────────────────────────────────────────────

  function GPUSupportedFeatures() {
    this._set = new Set();
  }
  GPUSupportedFeatures.prototype.has = function(f) { return this._set.has(f); };
  GPUSupportedFeatures.prototype.forEach = function(cb, t) { this._set.forEach(cb, t); };
  GPUSupportedFeatures.prototype[Symbol.iterator] = function() { return this._set[Symbol.iterator](); };
  Object.defineProperty(GPUSupportedFeatures.prototype, 'size', {
    get: function() { return this._set.size; }
  });
  globalThis.GPUSupportedFeatures = GPUSupportedFeatures;

  // ── GPUSupportedLimits ───────────────────────────────────────────────────

  function GPUSupportedLimits() {
    this.maxTextureDimension1D          = 8192;
    this.maxTextureDimension2D          = 8192;
    this.maxTextureDimension3D          = 2048;
    this.maxTextureArrayLayers          = 256;
    this.maxBindGroups                  = 4;
    this.maxBindingsPerBindGroup        = 1000;
    this.maxDynamicUniformBuffersPerPipelineLayout = 8;
    this.maxDynamicStorageBuffersPerPipelineLayout = 4;
    this.maxSampledTexturesPerShaderStage = 16;
    this.maxSamplersPerShaderStage      = 16;
    this.maxStorageBuffersPerShaderStage = 8;
    this.maxStorageTexturesPerShaderStage = 4;
    this.maxUniformBuffersPerShaderStage = 12;
    this.maxUniformBufferBindingSize    = 65536;
    this.maxStorageBufferBindingSize    = 134217728;
    this.minUniformBufferOffsetAlignment = 256;
    this.minStorageBufferOffsetAlignment = 256;
    this.maxVertexBuffers               = 8;
    this.maxBufferSize                  = 268435456;
    this.maxVertexAttributes            = 16;
    this.maxVertexBufferArrayStride     = 2048;
    this.maxInterStageShaderComponents  = 60;
    this.maxColorAttachments            = 8;
    this.maxComputeWorkgroupStorageSize = 16384;
    this.maxComputeInvocationsPerWorkgroup = 256;
    this.maxComputeWorkgroupSizeX       = 256;
    this.maxComputeWorkgroupSizeY       = 256;
    this.maxComputeWorkgroupSizeZ       = 64;
    this.maxComputeWorkgroupsPerDimension = 65535;
  }
  globalThis.GPUSupportedLimits = GPUSupportedLimits;

  // ── GPUAdapterInfo ───────────────────────────────────────────────────────

  function GPUAdapterInfo() {
    this.vendor      = 'lumen';
    this.architecture = '';
    this.device      = 'stub';
    this.description = 'Lumen WebGPU Phase 0 stub';
  }
  globalThis.GPUAdapterInfo = GPUAdapterInfo;

  // ── GPUShaderModule ──────────────────────────────────────────────────────

  function GPUShaderModule(desc) {
    this.label = (desc && desc.label) || '';
  }
  GPUShaderModule.prototype.getCompilationInfo = function() {
    return Promise.resolve({ messages: [] });
  };
  globalThis.GPUShaderModule = GPUShaderModule;

  // ── GPUBuffer ────────────────────────────────────────────────────────────

  function GPUBuffer(desc) {
    this.label  = (desc && desc.label) || '';
    this.size   = (desc && desc.size)  || 0;
    this.usage  = (desc && desc.usage) || 0;
    this._mapped = false;
    this._data   = new ArrayBuffer(this.size);
  }
  // Phase 0: mapAsync resolves immediately; getMappedRange returns a zero buffer.
  GPUBuffer.prototype.mapAsync = function(mode, offset, size) {
    this._mapped = true;
    return Promise.resolve();
  };
  GPUBuffer.prototype.getMappedRange = function(offset, size) {
    var off = offset || 0;
    var sz  = (size !== undefined) ? size : this.size - off;
    return this._data.slice(off, off + sz);
  };
  GPUBuffer.prototype.unmap   = function() { this._mapped = false; };
  GPUBuffer.prototype.destroy = function() { this._data = new ArrayBuffer(0); };
  globalThis.GPUBuffer = GPUBuffer;

  // ── GPUTextureView ───────────────────────────────────────────────────────

  function GPUTextureView(label) {
    this.label = label || '';
  }
  globalThis.GPUTextureView = GPUTextureView;

  // ── GPUTexture ───────────────────────────────────────────────────────────

  function GPUTexture(desc) {
    this.label       = (desc && desc.label)       || '';
    this.width       = (desc && desc.size && desc.size.width)  || 1;
    this.height      = (desc && desc.size && desc.size.height) || 1;
    this.depthOrArrayLayers = (desc && desc.size && desc.size.depthOrArrayLayers) || 1;
    this.mipLevelCount = (desc && desc.mipLevelCount) || 1;
    this.sampleCount = (desc && desc.sampleCount) || 1;
    this.dimension   = (desc && desc.dimension)   || '2d';
    this.format      = (desc && desc.format)      || 'rgba8unorm';
    this.usage       = (desc && desc.usage)       || 0;
  }
  GPUTexture.prototype.createView = function(desc) {
    return new GPUTextureView((desc && desc.label) || this.label + '-view');
  };
  GPUTexture.prototype.destroy = function() {};
  globalThis.GPUTexture = GPUTexture;

  // ── GPUSampler ───────────────────────────────────────────────────────────

  function GPUSampler(desc) {
    this.label = (desc && desc.label) || '';
  }
  globalThis.GPUSampler = GPUSampler;

  // ── GPUBindGroupLayout ───────────────────────────────────────────────────

  function GPUBindGroupLayout(desc) {
    this.label = (desc && desc.label) || '';
  }
  globalThis.GPUBindGroupLayout = GPUBindGroupLayout;

  // ── GPUPipelineLayout ────────────────────────────────────────────────────

  function GPUPipelineLayout(desc) {
    this.label = (desc && desc.label) || '';
  }
  globalThis.GPUPipelineLayout = GPUPipelineLayout;

  // ── GPUBindGroup ─────────────────────────────────────────────────────────

  function GPUBindGroup(desc) {
    this.label = (desc && desc.label) || '';
  }
  globalThis.GPUBindGroup = GPUBindGroup;

  // ── GPURenderPipeline ────────────────────────────────────────────────────

  function GPURenderPipeline(desc) {
    this.label = (desc && desc.label) || '';
  }
  GPURenderPipeline.prototype.getBindGroupLayout = function(idx) {
    return new GPUBindGroupLayout({});
  };
  globalThis.GPURenderPipeline = GPURenderPipeline;

  // ── GPUComputePipeline ───────────────────────────────────────────────────

  function GPUComputePipeline(desc) {
    this.label = (desc && desc.label) || '';
  }
  GPUComputePipeline.prototype.getBindGroupLayout = function(idx) {
    return new GPUBindGroupLayout({});
  };
  globalThis.GPUComputePipeline = GPUComputePipeline;

  // ── GPURenderPassEncoder ─────────────────────────────────────────────────

  function GPURenderPassEncoder() {}
  GPURenderPassEncoder.prototype.setPipeline       = function(pipeline) {};
  GPURenderPassEncoder.prototype.setVertexBuffer   = function(slot, buf, offset, size) {};
  GPURenderPassEncoder.prototype.setIndexBuffer    = function(buf, fmt, offset, size) {};
  GPURenderPassEncoder.prototype.setBindGroup      = function(idx, bg, dynOffsets) {};
  GPURenderPassEncoder.prototype.draw              = function(vtxCount, instCount, firstVtx, firstInst) {};
  GPURenderPassEncoder.prototype.drawIndexed       = function(idxCount, instCount, firstIdx, baseVtx, firstInst) {};
  GPURenderPassEncoder.prototype.setViewport       = function(x, y, w, h, minD, maxD) {};
  GPURenderPassEncoder.prototype.setScissorRect    = function(x, y, w, h) {};
  GPURenderPassEncoder.prototype.setBlendConstant  = function(color) {};
  GPURenderPassEncoder.prototype.setStencilReference = function(ref) {};
  // Both end() (current spec) and endPass() (older spec) supported.
  GPURenderPassEncoder.prototype.end              = function() {};
  GPURenderPassEncoder.prototype.endPass          = function() {};
  globalThis.GPURenderPassEncoder = GPURenderPassEncoder;

  // ── GPUComputePassEncoder ────────────────────────────────────────────────

  function GPUComputePassEncoder() {}
  GPUComputePassEncoder.prototype.setPipeline         = function(pipeline) {};
  GPUComputePassEncoder.prototype.setBindGroup        = function(idx, bg, dynOffsets) {};
  GPUComputePassEncoder.prototype.dispatchWorkgroups  = function(x, y, z) {};
  GPUComputePassEncoder.prototype.end                 = function() {};
  GPUComputePassEncoder.prototype.endPass             = function() {};
  globalThis.GPUComputePassEncoder = GPUComputePassEncoder;

  // ── GPUCommandBuffer ─────────────────────────────────────────────────────

  function GPUCommandBuffer(label) {
    this.label = label || '';
  }
  globalThis.GPUCommandBuffer = GPUCommandBuffer;

  // ── GPUCommandEncoder ────────────────────────────────────────────────────

  function GPUCommandEncoder(desc) {
    this.label = (desc && desc.label) || '';
  }
  GPUCommandEncoder.prototype.beginRenderPass = function(desc) {
    return new GPURenderPassEncoder();
  };
  GPUCommandEncoder.prototype.beginComputePass = function(desc) {
    return new GPUComputePassEncoder();
  };
  GPUCommandEncoder.prototype.copyBufferToBuffer = function(src, srcOff, dst, dstOff, size) {};
  GPUCommandEncoder.prototype.copyTextureToBuffer = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.copyBufferToTexture = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.copyTextureToTexture = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.clearBuffer = function(buf, offset, size) {};
  GPUCommandEncoder.prototype.finish = function(desc) {
    return new GPUCommandBuffer((desc && desc.label) || this.label);
  };
  globalThis.GPUCommandEncoder = GPUCommandEncoder;

  // ── GPUQueue ─────────────────────────────────────────────────────────────

  function GPUQueue() {
    this.label = '';
  }
  // Phase 0: submit is a no-op; command buffers carry no GPU work.
  GPUQueue.prototype.submit         = function(cmds) {};
  GPUQueue.prototype.writeBuffer    = function(buf, bufOffset, data, dataOffset, size) {};
  GPUQueue.prototype.writeTexture   = function(dest, data, layout, size) {};
  GPUQueue.prototype.copyExternalImageToTexture = function(src, dst, size) {};
  GPUQueue.prototype.onSubmittedWorkDone = function() { return Promise.resolve(); };
  globalThis.GPUQueue = GPUQueue;

  // ── GPUDevice ────────────────────────────────────────────────────────────

  function GPUDevice(desc) {
    this.label    = (desc && desc.label) || '';
    this.features = new GPUSupportedFeatures();
    this.limits   = new GPUSupportedLimits();
    this.queue    = new GPUQueue();
    this.lost     = new Promise(function() {});
    this._listeners = {};
  }
  GPUDevice.prototype.createBuffer           = function(desc) { return new GPUBuffer(desc); };
  GPUDevice.prototype.createTexture          = function(desc) { return new GPUTexture(desc); };
  GPUDevice.prototype.createSampler          = function(desc) { return new GPUSampler(desc); };
  GPUDevice.prototype.createShaderModule     = function(desc) { return new GPUShaderModule(desc); };
  GPUDevice.prototype.createBindGroupLayout  = function(desc) { return new GPUBindGroupLayout(desc); };
  GPUDevice.prototype.createPipelineLayout   = function(desc) { return new GPUPipelineLayout(desc); };
  GPUDevice.prototype.createBindGroup        = function(desc) { return new GPUBindGroup(desc); };
  GPUDevice.prototype.createRenderPipeline   = function(desc) { return new GPURenderPipeline(desc); };
  GPUDevice.prototype.createComputePipeline  = function(desc) { return new GPUComputePipeline(desc); };
  GPUDevice.prototype.createCommandEncoder   = function(desc) { return new GPUCommandEncoder(desc); };
  // Async variants resolve immediately with the sync result.
  GPUDevice.prototype.createRenderPipelineAsync  = function(desc) {
    return Promise.resolve(new GPURenderPipeline(desc));
  };
  GPUDevice.prototype.createComputePipelineAsync = function(desc) {
    return Promise.resolve(new GPUComputePipeline(desc));
  };
  GPUDevice.prototype.destroy = function() {};
  GPUDevice.prototype.pushErrorScope  = function(filter) {};
  GPUDevice.prototype.popErrorScope   = function() { return Promise.resolve(null); };
  GPUDevice.prototype.addEventListener = function(type, cb) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(cb);
  };
  GPUDevice.prototype.removeEventListener = function(type, cb) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(l) { return l !== cb; });
  };
  globalThis.GPUDevice = GPUDevice;

  // ── GPUAdapter ───────────────────────────────────────────────────────────

  function GPUAdapter() {
    this.features = new GPUSupportedFeatures();
    this.limits   = new GPUSupportedLimits();
    this.isFallbackAdapter = false;
  }
  GPUAdapter.prototype.requestDevice = function(desc) {
    return Promise.resolve(new GPUDevice(desc));
  };
  GPUAdapter.prototype.requestAdapterInfo = function() {
    return Promise.resolve(new GPUAdapterInfo());
  };
  globalThis.GPUAdapter = GPUAdapter;

  // ── GPUCanvasContext ─────────────────────────────────────────────────────

  function GPUCanvasContext(canvas) {
    this._canvas  = canvas;
    this._config  = null;
    this._texture = null;
  }
  // Configure the swap-chain format. Phase 0: stores config, no real surface.
  GPUCanvasContext.prototype.configure = function(config) {
    this._config = config || {};
    var w = (this._canvas && this._canvas.width)  || 1;
    var h = (this._canvas && this._canvas.height) || 1;
    this._texture = new GPUTexture({
      size: { width: w, height: h, depthOrArrayLayers: 1 },
      format: (config && config.format) || 'bgra8unorm',
      usage: (config && config.usage)   || 0x10 /* RENDER_ATTACHMENT */
    });
  };
  // Returns the current swap-chain texture. Phase 0: same stub texture each frame.
  GPUCanvasContext.prototype.getCurrentTexture = function() {
    if (!this._texture) {
      var w = (this._canvas && this._canvas.width)  || 1;
      var h = (this._canvas && this._canvas.height) || 1;
      this._texture = new GPUTexture({ size: { width: w, height: h } });
    }
    return this._texture;
  };
  GPUCanvasContext.prototype.unconfigure = function() {
    this._config  = null;
    this._texture = null;
  };
  GPUCanvasContext.prototype.getPreferredCanvasFormat = function() {
    return 'bgra8unorm';
  };
  globalThis.GPUCanvasContext = GPUCanvasContext;

  // ── GPU (navigator.gpu) ──────────────────────────────────────────────────

  var _gpu = {
    // Returns a stub GPUAdapter. Phase 0: options ignored.
    requestAdapter: function(opts) {
      return Promise.resolve(new GPUAdapter());
    },
    // Returns preferred swap-chain texture format (spec §canvas-configuration).
    getPreferredCanvasFormat: function() {
      return 'bgra8unorm';
    },
    // wgslLanguageFeatures — empty set for Phase 0.
    wgslLanguageFeatures: (function() {
      var s = new GPUSupportedFeatures();
      return s;
    }())
  };

  // ── navigator.gpu ────────────────────────────────────────────────────────

  if (typeof navigator !== 'undefined') {
    Object.defineProperty(navigator, 'gpu', {
      configurable: true,
      enumerable:   true,
      get: function() { return _gpu; }
    });
  }

  // ── GPU constants (GPUBufferUsage, GPUTextureUsage, etc.) ────────────────

  globalThis.GPUBufferUsage = {
    MAP_READ:      0x0001,
    MAP_WRITE:     0x0002,
    COPY_SRC:      0x0004,
    COPY_DST:      0x0008,
    INDEX:         0x0010,
    VERTEX:        0x0020,
    UNIFORM:       0x0040,
    STORAGE:       0x0080,
    INDIRECT:      0x0100,
    QUERY_RESOLVE: 0x0200
  };

  globalThis.GPUTextureUsage = {
    COPY_SRC:          0x01,
    COPY_DST:          0x02,
    TEXTURE_BINDING:   0x04,
    STORAGE_BINDING:   0x08,
    RENDER_ATTACHMENT: 0x10
  };

  globalThis.GPUShaderStage = {
    VERTEX:   0x1,
    FRAGMENT: 0x2,
    COMPUTE:  0x4
  };

  globalThis.GPUMapMode = {
    READ:  0x1,
    WRITE: 0x2
  };

  globalThis.GPUColorWrite = {
    RED:   0x1,
    GREEN: 0x2,
    BLUE:  0x4,
    ALPHA: 0x8,
    ALL:   0xF
  };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
            var navigator = globalThis.navigator || {};
            globalThis.navigator = navigator;
            "#,
        )
        .unwrap();
        install_webgpu_bindings(ctx).unwrap();
    }

    #[test]
    fn navigator_gpu_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("typeof navigator.gpu !== 'undefined' && typeof navigator.gpu.requestAdapter === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_adapter_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("navigator.gpu.requestAdapter() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_adapter_class_and_fields() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new GPUAdapter();
                    typeof a.requestDevice === 'function'
                      && typeof a.requestAdapterInfo === 'function'
                      && a.isFallbackAdapter === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_device_returns_promise_with_gpu_device() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var adapter = new GPUAdapter();
                    var p = adapter.requestDevice();
                    p instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_device_create_methods_exist() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    typeof d.createBuffer === 'function'
                      && typeof d.createTexture === 'function'
                      && typeof d.createRenderPipeline === 'function'
                      && typeof d.createComputePipeline === 'function'
                      && typeof d.createCommandEncoder === 'function'
                      && typeof d.createShaderModule === 'function'
                      && typeof d.createSampler === 'function'
                      && typeof d.createBindGroup === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_buffer_map_and_range() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var buf = d.createBuffer({ size: 64, usage: GPUBufferUsage.COPY_DST });
                    buf.size === 64
                      && buf.mapAsync(1) instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_command_encoder_render_pass() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d   = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginRenderPass({ colorAttachments: [] });
                    typeof pass.setPipeline === 'function'
                      && typeof pass.draw === 'function'
                      && typeof pass.end === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_queue_submit_is_noop() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d   = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var cmd = enc.finish({});
                    d.queue.submit([cmd]);
                    cmd instanceof GPUCommandBuffer
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_canvas_context_configure_and_get_texture() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var canvas = { width: 800, height: 600 };
                    var gpuCtx = new GPUCanvasContext(canvas);
                    gpuCtx.configure({ format: 'bgra8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT });
                    var tex = gpuCtx.getCurrentTexture();
                    tex instanceof GPUTexture && tex.format === 'bgra8unorm' && tex.width === 800
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_buffer_usage_constants_defined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    GPUBufferUsage.VERTEX === 0x0020
                      && GPUBufferUsage.UNIFORM === 0x0040
                      && GPUTextureUsage.RENDER_ATTACHMENT === 0x10
                      && GPUShaderStage.VERTEX === 0x1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn get_preferred_canvas_format() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let fmt: String = ctx.eval("navigator.gpu.getPreferredCanvasFormat()").unwrap();
            assert_eq!(fmt, "bgra8unorm");
        });
    }

    #[test]
    fn adapter_request_adapter_info_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("new GPUAdapter().requestAdapterInfo() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }
}
