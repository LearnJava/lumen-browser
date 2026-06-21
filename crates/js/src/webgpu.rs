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
//! **Phase 0 (default build, no `webgpu` feature):** no GPU — all operations in-memory
//! only.  All `create*` calls return opaque stub objects; `submit`/`draw`/`dispatch`
//! are no-ops.
//!
//! **Stage 2 (feature `webgpu`, sub-step 1 — buffers):** `GPUBuffer` is backed by a real
//! `wgpu::Buffer` (`lumen_paint::webgpu_compute`). `queue.writeBuffer`,
//! `commandEncoder.copyBufferToBuffer` + `queue.submit`, and `mapAsync`/`getMappedRange`
//! round-trip through real GPU memory. Each native call degrades gracefully to the Phase 0
//! in-memory path when no adapter is present. Compute/render pipelines + `dispatch`/`draw`
//! and canvas present remain Phase 0 stubs (next sub-step).

use rquickjs::Ctx;

/// Install the WebGPU API bindings into the JS context.
///
/// With the `webgpu` feature the real native bridge (`_lumen_webgpu_*`) is registered
/// **before** the shim is evaluated, so the shim's `typeof _lumen_webgpu_... === 'function'`
/// probes see it and route adapter-info / WGSL-validation to the real GPU device.
/// Without the feature only the in-memory JS shim (Phase 0) is installed.
pub fn install_webgpu_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    #[cfg(feature = "webgpu")]
    install_webgpu_natives(ctx)?;
    ctx.eval::<(), _>(WEBGPU_SHIM)?;
    Ok(())
}

/// Native backing for `_lumen_webgpu_buffer_read`: maps the GPU buffer range and returns
/// its bytes as a `Uint8Array`, or JS `null` when the read fails / no GPU is present.
///
/// A free function (not a closure) so the single `'js` lifetime ties `ctx` to the
/// returned [`rquickjs::Value`] — inferred closure HRTB lifetimes cannot express this.
#[cfg(feature = "webgpu")]
fn webgpu_buffer_read_native<'js>(
    ctx: Ctx<'js>,
    id: f64,
    offset: f64,
    size: f64,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    match lumen_paint::webgpu_compute::buffer_read(id as u64, offset as u64, size as u64) {
        Some(bytes) => Ok(rquickjs::TypedArray::new(ctx, bytes)?.into_value()),
        None => Ok(rquickjs::Value::new_null(ctx)),
    }
}

/// Registers the native WebGPU bridge functions backed by a real wgpu device
/// (`lumen_paint::webgpu_compute`). Stage 1: adapter info + WGSL validation.
/// Stage 2 (sub-step 1): GPUBuffer create/write/read/destroy + copy submit.
#[cfg(feature = "webgpu")]
fn install_webgpu_natives(ctx: &Ctx) -> rquickjs::Result<()> {
    use lumen_paint::webgpu_compute;
    let g = ctx.globals();

    // _lumen_webgpu_adapter_info() → JSON `{vendor,architecture,device,description}` for
    // a real GPU adapter, or "" when no GPU is available (shim then keeps the stub info).
    g.set(
        "_lumen_webgpu_adapter_info",
        rquickjs::Function::new(ctx.clone(), || -> String {
            match webgpu_compute::adapter_info() {
                Some(i) => serde_json::json!({
                    "vendor": i.vendor,
                    "architecture": i.architecture,
                    "device": i.device,
                    "description": i.description,
                })
                .to_string(),
                None => String::new(),
            }
        }),
    )?;

    // _lumen_webgpu_validate_shader(code) → "" if the WGSL is valid (or no GPU), else the
    // real compilation error text for GPUShaderModule.getCompilationInfo().
    g.set(
        "_lumen_webgpu_validate_shader",
        rquickjs::Function::new(ctx.clone(), |code: String| -> String {
            webgpu_compute::validate_wgsl(&code).unwrap_or_default()
        }),
    )?;

    // ── GPUBuffer bridge (Stage 2, sub-step 1) ───────────────────────────────
    // These back the JS GPUBuffer with a real wgpu::Buffer addressed by an opaque
    // numeric handle. Each returns a sentinel (0 / false / null) when no GPU is
    // available, so the shim transparently falls back to the Phase 0 in-memory path.

    // _lumen_webgpu_buffer_create(size, usage, mappedAtCreation) → handle (0 = failed/no GPU).
    g.set(
        "_lumen_webgpu_buffer_create",
        rquickjs::Function::new(ctx.clone(), |size: f64, usage: u32, mapped: bool| -> f64 {
            webgpu_compute::buffer_create(size as u64, usage, mapped).unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_buffer_write(handle, offset, bytes) → true on success.
    g.set(
        "_lumen_webgpu_buffer_write",
        rquickjs::Function::new(
            ctx.clone(),
            |id: f64, offset: f64, data: rquickjs::TypedArray<'_, u8>| -> bool {
                match data.as_bytes() {
                    Some(bytes) => webgpu_compute::buffer_write(id as u64, offset as u64, bytes),
                    None => false,
                }
            },
        ),
    )?;

    // _lumen_webgpu_buffer_read(handle, offset, size) → Uint8Array of bytes, or null.
    // Free function (not a closure) so a single `'js` ties `ctx` to the returned Value.
    g.set(
        "_lumen_webgpu_buffer_read",
        rquickjs::Function::new(ctx.clone(), webgpu_buffer_read_native)?,
    )?;

    // _lumen_webgpu_buffer_destroy(handle).
    g.set(
        "_lumen_webgpu_buffer_destroy",
        rquickjs::Function::new(ctx.clone(), |id: f64| {
            webgpu_compute::buffer_destroy(id as u64);
        }),
    )?;

    // _lumen_webgpu_submit(opsJson) → true on success. opsJson is a JSON array of
    // command-encoder ops recorded on the JS side; currently only copyBufferToBuffer.
    g.set(
        "_lumen_webgpu_submit",
        rquickjs::Function::new(ctx.clone(), |ops_json: String| -> bool {
            let Ok(ops) = serde_json::from_str::<Vec<serde_json::Value>>(&ops_json) else {
                return false;
            };
            let mut decoded = Vec::with_capacity(ops.len());
            for op in &ops {
                let kind = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
                match kind {
                    "copyB2B" => {
                        let f = |k: &str| op.get(k).and_then(serde_json::Value::as_u64);
                        let (Some(src), Some(src_offset), Some(dst), Some(dst_offset), Some(size)) =
                            (f("src"), f("srcOffset"), f("dst"), f("dstOffset"), f("size"))
                        else {
                            return false;
                        };
                        decoded.push(webgpu_compute::GpuOp::CopyBufferToBuffer {
                            src,
                            src_offset,
                            dst,
                            dst_offset,
                            size,
                        });
                    }
                    _ => return false,
                }
            }
            webgpu_compute::submit(&decoded)
        }),
    )?;

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
    // Phase 0 defaults — overridden below by the real GPU adapter when available.
    this.vendor      = 'lumen';
    this.architecture = '';
    this.device      = 'stub';
    this.description = 'Lumen WebGPU Phase 0 stub';
    // Real backend (feature `webgpu`): pull vendor/device/description from the GPU.
    if (typeof _lumen_webgpu_adapter_info === 'function') {
      try {
        var _j = _lumen_webgpu_adapter_info();
        if (_j) {
          var _o = JSON.parse(_j);
          this.vendor       = _o.vendor;
          this.architecture = _o.architecture;
          this.device       = _o.device;
          this.description  = _o.description;
        }
      } catch (_e) { /* keep stub info */ }
    }
  }
  globalThis.GPUAdapterInfo = GPUAdapterInfo;

  // ── GPUShaderModule ──────────────────────────────────────────────────────

  function GPUShaderModule(desc) {
    this.label = (desc && desc.label) || '';
    this._code = (desc && desc.code) || '';
    // Real backend (feature `webgpu`): validate WGSL on the GPU device at create time;
    // compilation diagnostics are surfaced through getCompilationInfo() like a browser.
    this._messages = [];
    if (this._code && typeof _lumen_webgpu_validate_shader === 'function') {
      try {
        var _err = _lumen_webgpu_validate_shader(this._code);
        if (_err) {
          this._messages = [{
            type: 'error', message: _err,
            lineNum: 0, linePos: 0, offset: 0, length: 0
          }];
        }
      } catch (_e) { /* validation unavailable — leave messages empty */ }
    }
  }
  GPUShaderModule.prototype.getCompilationInfo = function() {
    return Promise.resolve({ messages: this._messages });
  };
  globalThis.GPUShaderModule = GPUShaderModule;

  // ── GPUBuffer ────────────────────────────────────────────────────────────

  function GPUBuffer(desc) {
    this.label  = (desc && desc.label) || '';
    this.size   = (desc && desc.size)  || 0;
    this.usage  = (desc && desc.usage) || 0;
    this._mapped = false;
    this._mappedRange  = null;  // ArrayBuffer backing the currently mapped region
    this._mappedOffset = 0;
    this._mapWrite     = false;
    // Stage 2: back the buffer with a real wgpu::Buffer when the GPU bridge is present.
    // mappedAtCreation is emulated in-memory (passing false to native), so the real
    // buffer stays unmapped and writable through queue.writeBuffer / copy submit.
    this._id = 0;
    if (this.size > 0 && typeof _lumen_webgpu_buffer_create === 'function') {
      try { this._id = _lumen_webgpu_buffer_create(this.size, this.usage, false) || 0; }
      catch (_e) { this._id = 0; }
    }
    // In-memory store: Phase 0 fallback and the backing for mapped write ranges.
    this._data = new ArrayBuffer(this.size);
    if (desc && desc.mappedAtCreation) {
      this._mapped = true;
      this._mapWrite = true;
      this._mappedRange = this._data;
      this._mappedOffset = 0;
    }
  }
  // mapAsync: real path pulls current GPU contents for the range (MAP_READ buffers);
  // otherwise falls back to the in-memory slice. Resolves immediately (single-threaded).
  GPUBuffer.prototype.mapAsync = function(mode, offset, size) {
    var off = offset || 0;
    var sz  = (size !== undefined) ? size : this.size - off;
    this._mapped = true;
    this._mappedOffset = off;
    this._mapWrite = !!(mode & 0x2 /* GPUMapMode.WRITE */);
    if (this._id && typeof _lumen_webgpu_buffer_read === 'function') {
      try {
        var bytes = _lumen_webgpu_buffer_read(this._id, off, sz);
        if (bytes) {
          var ab = new ArrayBuffer(sz);
          new Uint8Array(ab).set(bytes);
          this._mappedRange = ab;
          return Promise.resolve();
        }
      } catch (_e) { /* fall through to in-memory */ }
    }
    this._mappedRange = this._data.slice(off, off + sz);
    return Promise.resolve();
  };
  // getMappedRange returns the live mapped ArrayBuffer (writes land here and are flushed
  // to the GPU on unmap for MAP_WRITE buffers). Sub-ranges return a copy.
  GPUBuffer.prototype.getMappedRange = function(offset, size) {
    var off = offset || 0;
    if (this._mappedRange) {
      var rel = off - this._mappedOffset;
      if (rel === 0 && size === undefined) return this._mappedRange;
      var sz = (size !== undefined) ? size : this._mappedRange.byteLength - rel;
      return this._mappedRange.slice(rel, rel + sz);
    }
    var sz2 = (size !== undefined) ? size : this.size - off;
    return this._data.slice(off, off + sz2);
  };
  // unmap: flush a write-mapped range back to the real GPU buffer (best effort).
  GPUBuffer.prototype.unmap = function() {
    if (this._mapped && this._mapWrite && this._id && this._mappedRange &&
        typeof _lumen_webgpu_buffer_write === 'function') {
      try { _lumen_webgpu_buffer_write(this._id, this._mappedOffset, new Uint8Array(this._mappedRange)); }
      catch (_e) { /* keep in-memory copy */ }
    }
    this._mapped = false;
    this._mappedRange = null;
    this._mapWrite = false;
  };
  GPUBuffer.prototype.destroy = function() {
    if (this._id && typeof _lumen_webgpu_buffer_destroy === 'function') {
      try { _lumen_webgpu_buffer_destroy(this._id); } catch (_e) {}
      this._id = 0;
    }
    this._data = new ArrayBuffer(0);
  };
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
    this._ops  = [];  // recorded operations, flushed to the GPU on queue.submit
  }
  GPUCommandEncoder.prototype.beginRenderPass = function(desc) {
    return new GPURenderPassEncoder();
  };
  GPUCommandEncoder.prototype.beginComputePass = function(desc) {
    return new GPUComputePassEncoder();
  };
  // Records a buffer→buffer copy. Supports both the legacy 5-arg signature
  // (source, sourceOffset, destination, destinationOffset, size) and the newer
  // 3-arg form (source, destination, size?).
  GPUCommandEncoder.prototype.copyBufferToBuffer = function(a, b, c, d, e) {
    var src, srcOff, dst, dstOff, size;
    if (b instanceof GPUBuffer) {
      src = a; srcOff = 0; dst = b; dstOff = 0;
      size = (c !== undefined) ? c : a.size;
    } else {
      src = a; srcOff = b || 0; dst = c; dstOff = d || 0;
      size = (e !== undefined) ? e : (src.size - srcOff);
    }
    this._ops.push({
      op: 'copyB2B',
      src: src ? src._id : 0, srcOffset: srcOff,
      dst: dst ? dst._id : 0, dstOffset: dstOff, size: size,
      _srcBuf: src, _dstBuf: dst
    });
  };
  GPUCommandEncoder.prototype.copyTextureToBuffer = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.copyBufferToTexture = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.copyTextureToTexture = function(src, dst, extent) {};
  GPUCommandEncoder.prototype.clearBuffer = function(buf, offset, size) {};
  GPUCommandEncoder.prototype.finish = function(desc) {
    var cmd = new GPUCommandBuffer((desc && desc.label) || this.label);
    cmd._ops = this._ops;
    this._ops = [];
    return cmd;
  };
  globalThis.GPUCommandEncoder = GPUCommandEncoder;

  // ── GPUQueue ─────────────────────────────────────────────────────────────

  function GPUQueue() {
    this.label = '';
  }
  // submit flushes recorded command-encoder ops to the real GPU in one batch.
  // Falls back to an in-memory copy emulation when no GPU buffer handles are present.
  GPUQueue.prototype.submit = function(cmds) {
    if (!cmds) return;
    var allOps = [];
    for (var i = 0; i < cmds.length; i++) {
      var ops = cmds[i] && cmds[i]._ops;
      if (ops) for (var j = 0; j < ops.length; j++) allOps.push(ops[j]);
    }
    if (allOps.length === 0) return;
    var allReal = (typeof _lumen_webgpu_submit === 'function');
    if (allReal) {
      for (var k = 0; k < allOps.length; k++) {
        if (!allOps[k].src || !allOps[k].dst) { allReal = false; break; }
      }
    }
    if (allReal) {
      var payload = allOps.map(function(o) {
        return { op: o.op, src: o.src, srcOffset: o.srcOffset,
                 dst: o.dst, dstOffset: o.dstOffset, size: o.size };
      });
      try { if (_lumen_webgpu_submit(JSON.stringify(payload))) return; }
      catch (_e) { /* fall through to in-memory emulation */ }
    }
    for (var m = 0; m < allOps.length; m++) {
      var op = allOps[m];
      if (op.op === 'copyB2B' && op._srcBuf && op._dstBuf) {
        var srcU8 = new Uint8Array(op._srcBuf._data, op.srcOffset, op.size);
        new Uint8Array(op._dstBuf._data).set(srcU8, op.dstOffset);
      }
    }
  };
  // writeBuffer uploads bytes to a buffer. Routes to the real GPU when the buffer has a
  // handle; otherwise writes into the in-memory store. dataOffset/size are treated as
  // byte offsets (correct for ArrayBuffer / Uint8Array sources).
  GPUQueue.prototype.writeBuffer = function(buffer, bufferOffset, data, dataOffset, size) {
    var u8;
    if (data instanceof ArrayBuffer) {
      u8 = new Uint8Array(data);
    } else if (data && data.buffer instanceof ArrayBuffer) {
      u8 = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    } else {
      return;
    }
    var dOff = dataOffset || 0;
    var bytes = (size !== undefined) ? u8.subarray(dOff, dOff + size) : u8.subarray(dOff);
    if (buffer && buffer._id && typeof _lumen_webgpu_buffer_write === 'function') {
      try { if (_lumen_webgpu_buffer_write(buffer._id, bufferOffset || 0, bytes)) return; }
      catch (_e) { /* fall through to in-memory */ }
    }
    if (buffer && buffer._data) {
      new Uint8Array(buffer._data).set(bytes, bufferOffset || 0);
    }
  };
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
    fn buffer_write_copy_map_round_trip() {
        // Exercises writeBuffer → copyBufferToBuffer → mapAsync → getMappedRange. Without
        // the `webgpu` feature this runs the Phase 0 in-memory emulation; with the feature
        // and a real adapter it round-trips through actual GPU memory. The buffer usages
        // (COPY_SRC|COPY_DST on src, COPY_DST|MAP_READ on dst) are valid for both paths.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d   = new GPUDevice({});
                    var src = d.createBuffer({ size: 8, usage: GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST });
                    var dst = d.createBuffer({ size: 8, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
                    d.queue.writeBuffer(src, 0, new Uint8Array([10,20,30,40,50,60,70,80]));
                    var enc = d.createCommandEncoder({});
                    enc.copyBufferToBuffer(src, 0, dst, 0, 8);
                    d.queue.submit([enc.finish()]);
                    dst.mapAsync(GPUMapMode.READ);
                    var v = new Uint8Array(dst.getMappedRange());
                    v[0] === 10 && v[3] === 40 && v[7] === 80
                    "#,
                )
                .unwrap();
            assert!(ok, "in-memory buffer copy round-trip must preserve bytes");
        });
    }

    #[test]
    fn buffer_mapped_at_creation_write_then_read() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var b = d.createBuffer({ size: 4, usage: GPUBufferUsage.VERTEX, mappedAtCreation: true });
                    var w = new Uint8Array(b.getMappedRange());
                    w[0] = 7; w[3] = 9;
                    b.unmap();
                    b.mapAsync(GPUMapMode.READ);
                    var r = new Uint8Array(b.getMappedRange());
                    r[0] === 7 && r[3] === 9
                    "#,
                )
                .unwrap();
            assert!(ok, "mappedAtCreation writes must persist to the buffer store");
        });
    }

    #[test]
    fn buffer_copy_three_arg_form() {
        // Newer copyBufferToBuffer(source, destination, size?) overload.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d   = new GPUDevice({});
                    var src = d.createBuffer({ size: 4, usage: GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST });
                    var dst = d.createBuffer({ size: 4, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ });
                    d.queue.writeBuffer(src, 0, new Uint8Array([1,2,3,4]));
                    var enc = d.createCommandEncoder({});
                    enc.copyBufferToBuffer(src, dst, 4);
                    d.queue.submit([enc.finish()]);
                    dst.mapAsync(GPUMapMode.READ);
                    var v = new Uint8Array(dst.getMappedRange());
                    v[0] === 1 && v[3] === 4
                    "#,
                )
                .unwrap();
            assert!(ok, "3-arg copyBufferToBuffer must copy bytes");
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

    #[test]
    fn adapter_info_has_real_backend_fields() {
        // Shape contract for the real backend: GPUAdapterInfo always exposes the four
        // W3C fields. Without the `webgpu` feature these keep the Phase 0 stub values.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var i = new GPUAdapterInfo();
                    typeof i.vendor === 'string'
                      && typeof i.architecture === 'string'
                      && typeof i.device === 'string'
                      && typeof i.description === 'string'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn shader_module_compilation_info_shape() {
        // getCompilationInfo() resolves to an object with a `messages` array. With a real
        // GPU device + the `webgpu` feature it carries WGSL diagnostics; otherwise empty.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var m = d.createShaderModule({ code: '@compute @workgroup_size(1) fn main() {}' });
                    var info = null;
                    m.getCompilationInfo().then(function(r){ info = r; });
                    // microtask not drained synchronously here; assert the module stored code
                    // and exposes the API shape.
                    typeof m.getCompilationInfo === 'function' && m._code.length > 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // Real-GPU path: only meaningful with the `webgpu` feature AND an available adapter.
    // Skips gracefully on headless CI without a GPU.
    #[cfg(feature = "webgpu")]
    #[test]
    fn real_backend_validates_bad_wgsl() {
        if !lumen_paint::webgpu_compute::is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // Bad WGSL → native validator returns a non-empty error → stored as a message.
            let has_error: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var m = d.createShaderModule({ code: 'this is not valid wgsl @@@' });
                    m._messages.length > 0 && m._messages[0].type === 'error'
                    "#,
                )
                .unwrap();
            assert!(has_error, "real backend must flag invalid WGSL");

            // Real adapter info must replace the Phase 0 stub description.
            let real_info: bool = ctx
                .eval("new GPUAdapterInfo().description.indexOf('Phase 0 stub') === -1")
                .unwrap();
            assert!(real_info, "real adapter description must not be the stub");
        });
    }
}
