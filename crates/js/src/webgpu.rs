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
//! round-trip through real GPU memory.
//!
//! **Stage 2 (sub-step 2 — compute):** `createShaderModule`, `createComputePipeline`
//! (`layout: 'auto'`), `pipeline.getBindGroupLayout`, `createBindGroup`, and a real
//! `beginComputePass` → `setPipeline`/`setBindGroup`/`dispatchWorkgroups` → `end` execute
//! the WGSL shader on the GPU when `queue.submit` flushes the encoder. Each native call
//! degrades gracefully to the Phase 0 path when no adapter is present (compute becomes a
//! no-op, since WGSL cannot run on the CPU).
//!
//! **Stage 3 (sub-step 1 — render to texture):** `createTexture` is backed by a real
//! `wgpu::Texture` (offscreen render target); `createRenderPipeline` (`layout: 'auto'`) builds
//! a real `wgpu::RenderPipeline` from the vertex + fragment modules, target format and vertex
//! buffer layouts; `beginRenderPass` → `setPipeline`/`setVertexBuffer`/`setBindGroup`/`draw` →
//! `end` plus `copyTextureToBuffer` execute on the GPU at `queue.submit` and the rendered
//! pixels are read back through a real `MAP_READ` buffer.
//!
//! **Stage 3 (sub-step 2 — canvas present):** `canvas.getContext('webgpu')` returns a
//! `GPUCanvasContext` whose `configure` allocates a real render-target texture and whose
//! `getCurrentTexture` returns it. After a `queue.submit` containing a render pass into that
//! texture, the frame is read back (`texture_read_rgba`) and pushed into the page `<canvas>`'s
//! 2D buffer (`canvas:{nid}`), so the GPU-rendered image actually appears on the page.

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

    // ── Compute pipeline bridge (Stage 2, sub-step 2) ────────────────────────
    // Real wgpu shader modules / compute pipelines / bind-group layouts / bind groups,
    // each addressed by an opaque numeric handle. Each returns 0 when no GPU is available,
    // so the shim transparently falls back to the Phase 0 no-op compute path.

    // _lumen_webgpu_shader_create(code) → handle (0 = failed/no GPU).
    g.set(
        "_lumen_webgpu_shader_create",
        rquickjs::Function::new(ctx.clone(), |code: String| -> f64 {
            webgpu_compute::shader_create(&code).unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_compute_pipeline_create(shaderHandle, entryPoint) → handle (0 = failed).
    g.set(
        "_lumen_webgpu_compute_pipeline_create",
        rquickjs::Function::new(ctx.clone(), |shader: f64, entry: String| -> f64 {
            webgpu_compute::compute_pipeline_create(shader as u64, &entry).unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_pipeline_bind_group_layout(pipelineHandle, group) → layout handle.
    g.set(
        "_lumen_webgpu_pipeline_bind_group_layout",
        rquickjs::Function::new(ctx.clone(), |pipeline: f64, group: u32| -> f64 {
            webgpu_compute::pipeline_bind_group_layout(pipeline as u64, group).unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_bind_group_create(layoutHandle, entriesJson) → bind-group handle.
    // entriesJson: [{binding, buffer, offset, size}] (size 0 = whole buffer). JSON is parsed
    // here (lumen-js owns serde_json); lumen-paint receives already-decoded entries.
    g.set(
        "_lumen_webgpu_bind_group_create",
        rquickjs::Function::new(ctx.clone(), |layout: f64, entries: String| -> f64 {
            let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&entries) else {
                return 0.0;
            };
            let mut decoded = Vec::with_capacity(parsed.len());
            for e in &parsed {
                let u = |k: &str| e.get(k).and_then(serde_json::Value::as_u64);
                let (Some(binding), Some(buffer)) = (u("binding"), u("buffer")) else {
                    return 0.0;
                };
                decoded.push(webgpu_compute::BufferBindEntry {
                    binding: binding as u32,
                    buffer,
                    offset: u("offset").unwrap_or(0),
                    size: u("size").unwrap_or(0),
                });
            }
            webgpu_compute::bind_group_create(layout as u64, &decoded).unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_compute_pipeline_destroy(handle).
    g.set(
        "_lumen_webgpu_compute_pipeline_destroy",
        rquickjs::Function::new(ctx.clone(), |id: f64| {
            webgpu_compute::compute_pipeline_destroy(id as u64);
        }),
    )?;

    // ── Render pipeline + texture bridge (Stage 3, sub-step 1) ───────────────
    // Real wgpu textures (offscreen render targets) and render pipelines, each addressed by
    // an opaque numeric handle. Returns 0 when no GPU is available → the shim keeps the
    // Phase 0 stub path. Canvas present (showing the texture on the page) is the next sub-step.

    // _lumen_webgpu_texture_create(width, height, format, usage) → handle (0 = failed/no GPU).
    g.set(
        "_lumen_webgpu_texture_create",
        rquickjs::Function::new(
            ctx.clone(),
            |width: f64, height: f64, format: String, usage: u32| -> f64 {
                webgpu_compute::texture_create(width as u32, height as u32, &format, usage)
                    .unwrap_or(0) as f64
            },
        ),
    )?;

    // _lumen_webgpu_texture_destroy(handle).
    g.set(
        "_lumen_webgpu_texture_destroy",
        rquickjs::Function::new(ctx.clone(), |id: f64| {
            webgpu_compute::texture_destroy(id as u64);
        }),
    )?;

    // _lumen_webgpu_render_pipeline_create(configJson) → handle (0 = failed/no GPU).
    // configJson: { vs, vsEntry, fs, fsEntry, format, topology,
    //               buffers: [{ arrayStride, instance, attributes: [{format, offset, shaderLocation}] }] }
    g.set(
        "_lumen_webgpu_render_pipeline_create",
        rquickjs::Function::new(ctx.clone(), |config: String| -> f64 {
            let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&config) else {
                return 0.0;
            };
            let u = |k: &str| cfg.get(k).and_then(serde_json::Value::as_u64);
            let s = |k: &str| cfg.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
            let (Some(vs), Some(fs)) = (u("vs"), u("fs")) else {
                return 0.0;
            };
            // Decode the vertex buffer layouts.
            let mut buffers = Vec::new();
            if let Some(bufs) = cfg.get("buffers").and_then(serde_json::Value::as_array) {
                for b in bufs {
                    let mut attributes = Vec::new();
                    if let Some(attrs) = b.get("attributes").and_then(serde_json::Value::as_array) {
                        for a in attrs {
                            let Some(fmt) = a.get("format").and_then(serde_json::Value::as_str)
                            else {
                                return 0.0;
                            };
                            attributes.push(webgpu_compute::VertexAttr {
                                format: fmt.to_string(),
                                offset: a
                                    .get("offset")
                                    .and_then(serde_json::Value::as_u64)
                                    .unwrap_or(0),
                                shader_location: a
                                    .get("shaderLocation")
                                    .and_then(serde_json::Value::as_u64)
                                    .unwrap_or(0) as u32,
                            });
                        }
                    }
                    buffers.push(webgpu_compute::VertexBufferLayout {
                        array_stride: b
                            .get("arrayStride")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0),
                        instance_step: b
                            .get("instance")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                        attributes,
                    });
                }
            }
            webgpu_compute::render_pipeline_create(
                vs,
                s("vsEntry"),
                fs,
                s("fsEntry"),
                s("format"),
                s("topology"),
                &buffers,
            )
            .unwrap_or(0) as f64
        }),
    )?;

    // _lumen_webgpu_render_pipeline_bind_group_layout(pipelineHandle, group) → layout handle.
    g.set(
        "_lumen_webgpu_render_pipeline_bind_group_layout",
        rquickjs::Function::new(ctx.clone(), |pipeline: f64, group: u32| -> f64 {
            webgpu_compute::render_pipeline_bind_group_layout(pipeline as u64, group).unwrap_or(0)
                as f64
        }),
    )?;

    // _lumen_webgpu_render_pipeline_destroy(handle).
    g.set(
        "_lumen_webgpu_render_pipeline_destroy",
        rquickjs::Function::new(ctx.clone(), |id: f64| {
            webgpu_compute::render_pipeline_destroy(id as u64);
        }),
    )?;

    // _lumen_webgpu_submit(opsJson) → true on success. opsJson is a JSON array of
    // command-encoder ops recorded on the JS side: copyBufferToBuffer + computePass.
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
                    "computePass" => {
                        let Some(cmds) = op.get("cmds").and_then(|v| v.as_array()) else {
                            return false;
                        };
                        let mut commands = Vec::with_capacity(cmds.len());
                        for c in cmds {
                            let ck = c.get("c").and_then(|v| v.as_str()).unwrap_or("");
                            match ck {
                                "setPipeline" => {
                                    let Some(p) =
                                        c.get("pipeline").and_then(serde_json::Value::as_u64)
                                    else {
                                        return false;
                                    };
                                    commands.push(webgpu_compute::ComputeCmd::SetPipeline(p));
                                }
                                "setBindGroup" => {
                                    let (Some(index), Some(bind_group)) = (
                                        c.get("index").and_then(serde_json::Value::as_u64),
                                        c.get("bindGroup").and_then(serde_json::Value::as_u64),
                                    ) else {
                                        return false;
                                    };
                                    commands.push(webgpu_compute::ComputeCmd::SetBindGroup {
                                        index: index as u32,
                                        bind_group,
                                    });
                                }
                                "dispatch" => {
                                    let g = |k: &str| {
                                        c.get(k).and_then(serde_json::Value::as_u64).unwrap_or(1)
                                            as u32
                                    };
                                    commands.push(webgpu_compute::ComputeCmd::Dispatch {
                                        x: g("x"),
                                        y: g("y"),
                                        z: g("z"),
                                    });
                                }
                                _ => return false,
                            }
                        }
                        decoded.push(webgpu_compute::GpuOp::ComputePass { commands });
                    }
                    "renderPass" => {
                        let f = |k: &str| op.get(k).and_then(serde_json::Value::as_u64);
                        let Some(color_texture) = f("colorTexture") else {
                            return false;
                        };
                        // clear: null → LoadOp::Load; [r,g,b,a] → LoadOp::Clear.
                        let clear = op.get("clear").and_then(|c| c.as_array()).map(|arr| {
                            let g = |i: usize| arr.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                            [g(0), g(1), g(2), g(3)]
                        });
                        let Some(cmds) = op.get("cmds").and_then(|v| v.as_array()) else {
                            return false;
                        };
                        let mut commands = Vec::with_capacity(cmds.len());
                        for c in cmds {
                            let ck = c.get("c").and_then(|v| v.as_str()).unwrap_or("");
                            let cu = |k: &str| c.get(k).and_then(serde_json::Value::as_u64);
                            let ci = |k: &str| c.get(k).and_then(serde_json::Value::as_i64);
                            match ck {
                                "setPipeline" => {
                                    let Some(p) = cu("pipeline") else { return false };
                                    commands.push(webgpu_compute::RenderCmd::SetPipeline(p));
                                }
                                "setBindGroup" => {
                                    let (Some(index), Some(bind_group)) =
                                        (cu("index"), cu("bindGroup"))
                                    else {
                                        return false;
                                    };
                                    commands.push(webgpu_compute::RenderCmd::SetBindGroup {
                                        index: index as u32,
                                        bind_group,
                                    });
                                }
                                "setVertexBuffer" => {
                                    let Some(buffer) = cu("buffer") else { return false };
                                    commands.push(webgpu_compute::RenderCmd::SetVertexBuffer {
                                        slot: cu("slot").unwrap_or(0) as u32,
                                        buffer,
                                        offset: cu("offset").unwrap_or(0),
                                        size: cu("size").unwrap_or(0),
                                    });
                                }
                                "setIndexBuffer" => {
                                    let Some(buffer) = cu("buffer") else { return false };
                                    commands.push(webgpu_compute::RenderCmd::SetIndexBuffer {
                                        buffer,
                                        format_u16: c
                                            .get("u16")
                                            .and_then(serde_json::Value::as_bool)
                                            .unwrap_or(false),
                                        offset: cu("offset").unwrap_or(0),
                                        size: cu("size").unwrap_or(0),
                                    });
                                }
                                "draw" => {
                                    commands.push(webgpu_compute::RenderCmd::Draw {
                                        vertex_count: cu("vertexCount").unwrap_or(0) as u32,
                                        instance_count: cu("instanceCount").unwrap_or(1) as u32,
                                        first_vertex: cu("firstVertex").unwrap_or(0) as u32,
                                        first_instance: cu("firstInstance").unwrap_or(0) as u32,
                                    });
                                }
                                "drawIndexed" => {
                                    commands.push(webgpu_compute::RenderCmd::DrawIndexed {
                                        index_count: cu("indexCount").unwrap_or(0) as u32,
                                        instance_count: cu("instanceCount").unwrap_or(1) as u32,
                                        first_index: cu("firstIndex").unwrap_or(0) as u32,
                                        base_vertex: ci("baseVertex").unwrap_or(0) as i32,
                                        first_instance: cu("firstInstance").unwrap_or(0) as u32,
                                    });
                                }
                                _ => return false,
                            }
                        }
                        decoded.push(webgpu_compute::GpuOp::RenderPass {
                            color_texture,
                            clear,
                            commands,
                        });
                    }
                    "copyTexToBuf" => {
                        let f = |k: &str| op.get(k).and_then(serde_json::Value::as_u64);
                        let (Some(texture), Some(buffer)) = (f("texture"), f("buffer")) else {
                            return false;
                        };
                        decoded.push(webgpu_compute::GpuOp::CopyTextureToBuffer {
                            texture,
                            buffer,
                            buffer_offset: f("bufferOffset").unwrap_or(0),
                            bytes_per_row: f("bytesPerRow").unwrap_or(0) as u32,
                            rows_per_image: f("rowsPerImage").unwrap_or(1) as u32,
                            width: f("width").unwrap_or(1) as u32,
                            height: f("height").unwrap_or(1) as u32,
                        });
                    }
                    _ => return false,
                }
            }
            webgpu_compute::submit(&decoded)
        }),
    )?;

    // _lumen_webgpu_canvas_present(nid, textureHandle) → true on success. Reads the rendered
    // texture back to dense RGBA8 and pushes it into the page <canvas> `nid`'s 2D buffer, which
    // the shell uploads as `canvas:{nid}` — the GPU-rendered frame becomes visible on the page.
    g.set(
        "_lumen_webgpu_canvas_present",
        rquickjs::Function::new(ctx.clone(), |nid: u32, texture: f64| -> bool {
            let Some((w, h, rgba)) = webgpu_compute::texture_read_rgba(texture as u64) else {
                return false;
            };
            crate::canvas2d::present_rgba(nid, w, h, &rgba);
            true
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
    // Stage 2 (compute): register a real wgpu::ShaderModule for pipeline creation.
    this._id = 0;
    if (this._code && typeof _lumen_webgpu_shader_create === 'function') {
      try { this._id = _lumen_webgpu_shader_create(this._code) || 0; }
      catch (_e) { this._id = 0; }
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
    // Stage 3: handle of the backing wgpu::Texture (0 = Phase 0 stub). Set by createView.
    this._textureId = 0;
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
    // Stage 3: back the texture with a real wgpu::Texture (offscreen render target) when the
    // bridge is present and the format is supported. _id = 0 keeps the Phase 0 stub path.
    this._id = 0;
    if (this.width > 0 && this.height > 0 && typeof _lumen_webgpu_texture_create === 'function') {
      try { this._id = _lumen_webgpu_texture_create(this.width, this.height, this.format, this.usage) || 0; }
      catch (_e) { this._id = 0; }
    }
  }
  GPUTexture.prototype.createView = function(desc) {
    var v = new GPUTextureView((desc && desc.label) || this.label + '-view');
    v._textureId = this._id;
    return v;
  };
  GPUTexture.prototype.destroy = function() {
    if (this._id && typeof _lumen_webgpu_texture_destroy === 'function') {
      try { _lumen_webgpu_texture_destroy(this._id); } catch (_e) {}
      this._id = 0;
    }
  };
  globalThis.GPUTexture = GPUTexture;

  // ── GPUSampler ───────────────────────────────────────────────────────────

  function GPUSampler(desc) {
    this.label = (desc && desc.label) || '';
  }
  globalThis.GPUSampler = GPUSampler;

  // ── GPUBindGroupLayout ───────────────────────────────────────────────────

  function GPUBindGroupLayout(desc) {
    this.label = (desc && desc.label) || '';
    // Stage 2 (compute): handle of the real wgpu::BindGroupLayout (0 = Phase 0 stub).
    // Populated by GPUComputePipeline.getBindGroupLayout via the native bridge.
    this._id = (desc && desc._id) || 0;
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
    // Stage 2 (compute): create a real wgpu::BindGroup binding buffers to the layout's
    // binding indices. Falls back to a stub (_id = 0) without a real layout / GPU.
    this._id = 0;
    var layout  = desc && desc.layout;
    var entries = (desc && desc.entries) || [];
    if (layout && layout._id && typeof _lumen_webgpu_bind_group_create === 'function') {
      try {
        var payload = [];
        for (var i = 0; i < entries.length; i++) {
          var e   = entries[i] || {};
          var res = e.resource || {};
          var buf = res.buffer;
          payload.push({
            binding: e.binding | 0,
            buffer:  buf ? (buf._id || 0) : 0,
            offset:  res.offset || 0,
            size:    res.size   || 0
          });
        }
        this._id = _lumen_webgpu_bind_group_create(layout._id, JSON.stringify(payload)) || 0;
      } catch (_e) { this._id = 0; }
    }
  }
  globalThis.GPUBindGroup = GPUBindGroup;

  // ── GPURenderPipeline ────────────────────────────────────────────────────

  function GPURenderPipeline(desc) {
    this.label = (desc && desc.label) || '';
    // Stage 3: create a real wgpu::RenderPipeline (auto layout) from the vertex + fragment
    // shader modules, target format and vertex buffer layouts. _id = 0 keeps the stub path.
    this._id = 0;
    if (desc && typeof _lumen_webgpu_render_pipeline_create === 'function') {
      try {
        var v = desc.vertex || {};
        var f = desc.fragment || {};
        var vmod = v.module, fmod = f.module;
        var targets = f.targets || [];
        var fmt = (targets[0] && targets[0].format) || '';
        var prim = desc.primitive || {};
        var buffers = [];
        var vbufs = v.buffers || [];
        for (var i = 0; i < vbufs.length; i++) {
          var vb = vbufs[i] || {};
          var attrs = [];
          var vatts = vb.attributes || [];
          for (var j = 0; j < vatts.length; j++) {
            var a = vatts[j] || {};
            attrs.push({ format: a.format, offset: a.offset || 0, shaderLocation: a.shaderLocation | 0 });
          }
          buffers.push({
            arrayStride: vb.arrayStride || 0,
            instance: vb.stepMode === 'instance',
            attributes: attrs
          });
        }
        if (vmod && vmod._id && fmod && fmod._id && fmt) {
          var cfg = {
            vs: vmod._id, vsEntry: v.entryPoint || '',
            fs: fmod._id, fsEntry: f.entryPoint || '',
            format: fmt, topology: prim.topology || 'triangle-list',
            buffers: buffers
          };
          this._id = _lumen_webgpu_render_pipeline_create(JSON.stringify(cfg)) || 0;
        }
      } catch (_e) { this._id = 0; }
    }
  }
  GPURenderPipeline.prototype.getBindGroupLayout = function(idx) {
    var layoutId = 0;
    if (this._id && typeof _lumen_webgpu_render_pipeline_bind_group_layout === 'function') {
      try { layoutId = _lumen_webgpu_render_pipeline_bind_group_layout(this._id, idx) || 0; }
      catch (_e) { layoutId = 0; }
    }
    return new GPUBindGroupLayout({ _id: layoutId });
  };
  globalThis.GPURenderPipeline = GPURenderPipeline;

  // ── GPUComputePipeline ───────────────────────────────────────────────────

  function GPUComputePipeline(desc) {
    this.label = (desc && desc.label) || '';
    // Stage 2 (compute): create a real wgpu::ComputePipeline from the shader module +
    // entry point (auto layout). _id = 0 keeps the Phase 0 no-op path.
    this._id = 0;
    var comp = desc && desc.compute;
    var mod  = comp && comp.module;
    if (mod && mod._id && typeof _lumen_webgpu_compute_pipeline_create === 'function') {
      try { this._id = _lumen_webgpu_compute_pipeline_create(mod._id, comp.entryPoint || '') || 0; }
      catch (_e) { this._id = 0; }
    }
  }
  GPUComputePipeline.prototype.getBindGroupLayout = function(idx) {
    var layoutId = 0;
    if (this._id && typeof _lumen_webgpu_pipeline_bind_group_layout === 'function') {
      try { layoutId = _lumen_webgpu_pipeline_bind_group_layout(this._id, idx) || 0; }
      catch (_e) { layoutId = 0; }
    }
    return new GPUBindGroupLayout({ _id: layoutId });
  };
  globalThis.GPUComputePipeline = GPUComputePipeline;

  // ── GPURenderPassEncoder ─────────────────────────────────────────────────

  // Records setPipeline / setVertexBuffer / setBindGroup / draw into a command list; on end()
  // the whole pass (its target texture + clear/load op + commands) is appended to the parent
  // encoder, flushed to the real GPU on queue.submit. Without a GPU the recorded ids are 0 and
  // the native submit no-ops (falls back to the in-memory path).
  function GPURenderPassEncoder(encoder, desc) {
    this._enc  = encoder;
    this._cmds = [];
    // Resolve the single color attachment's target texture id and load op from the descriptor.
    this._colorTexture = 0;
    this._clear = null;  // null → loadOp 'load'; [r,g,b,a] → loadOp 'clear'
    var atts = (desc && desc.colorAttachments) || [];
    var a0 = atts[0];
    if (a0) {
      var view = a0.view;
      this._colorTexture = (view && view._textureId) || 0;
      // loadOp defaults to 'clear' unless explicitly 'load'. clearValue defaults to opaque black.
      if (a0.loadOp !== 'load') {
        var cv = a0.clearValue;
        if (cv === undefined) {
          this._clear = [0, 0, 0, 1];
        } else if (Array.isArray(cv)) {
          this._clear = [cv[0] || 0, cv[1] || 0, cv[2] || 0, (cv[3] === undefined ? 1 : cv[3])];
        } else {
          this._clear = [cv.r || 0, cv.g || 0, cv.b || 0, (cv.a === undefined ? 1 : cv.a)];
        }
      }
    }
  }
  GPURenderPassEncoder.prototype.setPipeline = function(pipeline) {
    this._cmds.push({ c: 'setPipeline', pipeline: pipeline ? (pipeline._id || 0) : 0 });
  };
  GPURenderPassEncoder.prototype.setVertexBuffer = function(slot, buf, offset, size) {
    this._cmds.push({
      c: 'setVertexBuffer', slot: slot | 0, buffer: buf ? (buf._id || 0) : 0,
      offset: offset || 0, size: size || 0
    });
  };
  GPURenderPassEncoder.prototype.setIndexBuffer = function(buf, fmt, offset, size) {
    this._cmds.push({
      c: 'setIndexBuffer', buffer: buf ? (buf._id || 0) : 0,
      u16: fmt === 'uint16', offset: offset || 0, size: size || 0
    });
  };
  GPURenderPassEncoder.prototype.setBindGroup = function(idx, bg, dynOffsets) {
    this._cmds.push({ c: 'setBindGroup', index: idx | 0, bindGroup: bg ? (bg._id || 0) : 0 });
  };
  GPURenderPassEncoder.prototype.draw = function(vtxCount, instCount, firstVtx, firstInst) {
    this._cmds.push({
      c: 'draw', vertexCount: vtxCount | 0,
      instanceCount: (instCount === undefined) ? 1 : (instCount | 0),
      firstVertex: firstVtx || 0, firstInstance: firstInst || 0
    });
  };
  GPURenderPassEncoder.prototype.drawIndexed = function(idxCount, instCount, firstIdx, baseVtx, firstInst) {
    this._cmds.push({
      c: 'drawIndexed', indexCount: idxCount | 0,
      instanceCount: (instCount === undefined) ? 1 : (instCount | 0),
      firstIndex: firstIdx || 0, baseVertex: baseVtx || 0, firstInstance: firstInst || 0
    });
  };
  GPURenderPassEncoder.prototype.setViewport       = function(x, y, w, h, minD, maxD) {};
  GPURenderPassEncoder.prototype.setScissorRect    = function(x, y, w, h) {};
  GPURenderPassEncoder.prototype.setBlendConstant  = function(color) {};
  GPURenderPassEncoder.prototype.setStencilReference = function(ref) {};
  GPURenderPassEncoder.prototype.end = function() {
    if (this._enc) {
      this._enc._ops.push({
        op: 'renderPass', colorTexture: this._colorTexture,
        clear: this._clear, cmds: this._cmds
      });
    }
    this._cmds = [];
  };
  // Both end() (current spec) and endPass() (older spec) supported.
  GPURenderPassEncoder.prototype.endPass = GPURenderPassEncoder.prototype.end;
  globalThis.GPURenderPassEncoder = GPURenderPassEncoder;

  // ── GPUComputePassEncoder ────────────────────────────────────────────────

  // Records setPipeline / setBindGroup / dispatchWorkgroups into a command list; on end()
  // the whole pass is appended to the parent command encoder, flushed to the real GPU on
  // queue.submit. Without a GPU the recorded ids are 0 and the native submit no-ops.
  function GPUComputePassEncoder(encoder) {
    this._enc  = encoder;
    this._cmds = [];
  }
  GPUComputePassEncoder.prototype.setPipeline = function(pipeline) {
    this._cmds.push({ c: 'setPipeline', pipeline: pipeline ? (pipeline._id || 0) : 0 });
  };
  GPUComputePassEncoder.prototype.setBindGroup = function(idx, bg, dynOffsets) {
    this._cmds.push({ c: 'setBindGroup', index: idx | 0, bindGroup: bg ? (bg._id || 0) : 0 });
  };
  GPUComputePassEncoder.prototype.dispatchWorkgroups = function(x, y, z) {
    this._cmds.push({
      c: 'dispatch',
      x: (x === undefined) ? 1 : (x | 0),
      y: (y === undefined) ? 1 : (y | 0),
      z: (z === undefined) ? 1 : (z | 0)
    });
  };
  GPUComputePassEncoder.prototype.dispatchWorkgroupsIndirect = function(buf, offset) {};
  GPUComputePassEncoder.prototype.end = function() {
    if (this._enc) this._enc._ops.push({ op: 'computePass', cmds: this._cmds });
    this._cmds = [];
  };
  GPUComputePassEncoder.prototype.endPass = GPUComputePassEncoder.prototype.end;
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
    return new GPURenderPassEncoder(this, desc);
  };
  GPUCommandEncoder.prototype.beginComputePass = function(desc) {
    return new GPUComputePassEncoder(this);
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
  // Records a texture→buffer readback (Stage 3). src: { texture }, dst: { buffer, offset,
  // bytesPerRow, rowsPerImage }, extent: { width, height } (or [w, h]). bytesPerRow must be a
  // multiple of 256 (wgpu) — the caller sizes the destination buffer with padded rows.
  GPUCommandEncoder.prototype.copyTextureToBuffer = function(src, dst, extent) {
    var srcTex = src && src.texture;
    var dstBuf = dst && dst.buffer;
    var w = (extent && (extent.width  !== undefined ? extent.width  : extent[0])) || 1;
    var h = (extent && (extent.height !== undefined ? extent.height : extent[1])) || 1;
    this._ops.push({
      op: 'copyTexToBuf',
      texture: srcTex ? (srcTex._id || 0) : 0,
      buffer:  dstBuf ? (dstBuf._id || 0) : 0,
      bufferOffset: (dst && dst.offset) || 0,
      bytesPerRow:  (dst && dst.bytesPerRow) || 0,
      rowsPerImage: (dst && dst.rowsPerImage) || h,
      width: w, height: h,
      _dstBuf: dstBuf
    });
  };
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
    // Try the real GPU path only if every op is real-capable: copies need both buffer
    // handles, compute passes need a non-zero pipeline handle. Otherwise fall back.
    var allReal = (typeof _lumen_webgpu_submit === 'function');
    if (allReal) {
      for (var k = 0; k < allOps.length; k++) {
        var o = allOps[k];
        if (o.op === 'copyB2B') {
          if (!o.src || !o.dst) { allReal = false; break; }
        } else if (o.op === 'computePass') {
          var hasPipeline = false;
          for (var ci = 0; ci < o.cmds.length; ci++) {
            if (o.cmds[ci].c === 'setPipeline' && o.cmds[ci].pipeline) hasPipeline = true;
          }
          if (!hasPipeline) { allReal = false; break; }
        } else if (o.op === 'renderPass') {
          // Need a real target texture and a real pipeline to run on the GPU.
          var hasRp = false;
          for (var ri = 0; ri < o.cmds.length; ri++) {
            if (o.cmds[ri].c === 'setPipeline' && o.cmds[ri].pipeline) hasRp = true;
          }
          if (!o.colorTexture || !hasRp) { allReal = false; break; }
        } else if (o.op === 'copyTexToBuf') {
          if (!o.texture || !o.buffer) { allReal = false; break; }
        } else { allReal = false; break; }
      }
    }
    if (allReal) {
      // Texture ids rendered into this submit — used to present only the canvas contexts that
      // were actually drawn (not every configured context on every unrelated submit).
      var renderedTextures = {};
      var payload = allOps.map(function(o) {
        if (o.op === 'computePass') return { op: 'computePass', cmds: o.cmds };
        if (o.op === 'renderPass') {
          if (o.colorTexture) renderedTextures[o.colorTexture] = true;
          return { op: 'renderPass', colorTexture: o.colorTexture, clear: o.clear, cmds: o.cmds };
        }
        if (o.op === 'copyTexToBuf') {
          return { op: 'copyTexToBuf', texture: o.texture, buffer: o.buffer,
                   bufferOffset: o.bufferOffset, bytesPerRow: o.bytesPerRow,
                   rowsPerImage: o.rowsPerImage, width: o.width, height: o.height };
        }
        return { op: o.op, src: o.src, srcOffset: o.srcOffset,
                 dst: o.dst, dstOffset: o.dstOffset, size: o.size };
      });
      try {
        if (_lumen_webgpu_submit(JSON.stringify(payload))) {
          // Present every configured canvas whose current texture was a render target here.
          for (var p = 0; p < _gpuCanvasContexts.length; p++) {
            var cc = _gpuCanvasContexts[p];
            if (cc._texture && cc._texture._id && renderedTextures[cc._texture._id]) {
              _presentCanvasContext(cc);
            }
          }
          return;
        }
      }
      catch (_e) { /* fall through to in-memory emulation */ }
    }
    // In-memory fallback (no GPU): emulate buffer copies; compute passes are no-ops
    // because WGSL cannot run on the CPU.
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

  // Configured canvas contexts whose current texture should be presented to the page
  // <canvas> after a render-pass submit (Stage 3 sub-step 2). queue.submit scans this list.
  var _gpuCanvasContexts = [];

  // Read back the context's current texture and push it into the page <canvas> nid's 2D
  // buffer, so the shell composites the GPU-rendered frame as canvas:{nid}. No-op without a
  // real texture handle or the native bridge (Phase 0 stub canvases stay blank, as before).
  function _presentCanvasContext(cc) {
    if (!cc || !cc._texture || !cc._texture._id) return;
    var nid = cc._canvas && cc._canvas.__nid__;
    if (nid === undefined || nid === null) return;
    if (typeof _lumen_webgpu_canvas_present === 'function') {
      try { _lumen_webgpu_canvas_present(nid, cc._texture._id); } catch (_e) {}
    }
  }

  function GPUCanvasContext(canvas) {
    this._canvas  = canvas;
    this._config  = null;
    this._texture = null;
  }
  // Configure the swap-chain: allocate a real render-target texture sized to the canvas and
  // register the context so its frames present to the page after submit. The GPUTexture
  // constructor backs it with a real wgpu::Texture when the GPU bridge is available.
  GPUCanvasContext.prototype.configure = function(config) {
    this._config = config || {};
    var w = (this._canvas && this._canvas.width)  || 1;
    var h = (this._canvas && this._canvas.height) || 1;
    this._texture = new GPUTexture({
      size: { width: w, height: h, depthOrArrayLayers: 1 },
      format: (config && config.format) || 'bgra8unorm',
      usage: (config && config.usage)   || 0x10 /* RENDER_ATTACHMENT */
    });
    if (_gpuCanvasContexts.indexOf(this) === -1) _gpuCanvasContexts.push(this);
  };
  // Returns the current swap-chain texture. Lumen reuses one render-target texture per
  // configured context (presented after each render submit) rather than rotating a real
  // swap chain — sufficient for the offscreen present path.
  GPUCanvasContext.prototype.getCurrentTexture = function() {
    if (!this._texture) {
      var w = (this._canvas && this._canvas.width)  || 1;
      var h = (this._canvas && this._canvas.height) || 1;
      this._texture = new GPUTexture({ size: { width: w, height: h } });
    }
    return this._texture;
  };
  GPUCanvasContext.prototype.unconfigure = function() {
    var i = _gpuCanvasContexts.indexOf(this);
    if (i !== -1) _gpuCanvasContexts.splice(i, 1);
    if (this._texture && typeof this._texture.destroy === 'function') {
      try { this._texture.destroy(); } catch (_e) {}
    }
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

/// V8 port of [`install_webgpu_bindings`] (Ph3 V8 migration S9).
///
/// The natives here carry no `Persistent`/`Global` GC roots (confirmed by the
/// S8 slice's finding that WebGPU is a plain data/handle bridge — every arg
/// and return type is `f64`/`u32`/`String`/`bool`/`Vec<u8>`, all already
/// covered by `v8_compat`'s `FromJsValue`/`IntoJsReturn`), so this ports
/// through the ergonomic `into_v8_fnN` compat layer unchanged, mirroring
/// `webgl_canvas`'s S8 `rt.eval(WEBGL_SHIM)` pattern for the shim itself.
/// Without the `webgpu` Cargo feature (default), `navigator.gpu` stays a pure
/// in-memory JS shim (Phase 0) under V8 too — no natives are registered.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_webgpu_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;

    #[cfg(feature = "webgpu")]
    install_webgpu_natives_v8(rt)?;
    rt.eval(WEBGPU_SHIM)?;
    Ok(())
}

/// Registers the real wgpu-backed `_lumen_webgpu_*` bridge natives. V8 twin of
/// [`install_webgpu_natives`] — see that function's per-native doc comments
/// for the wire protocol (JSON-encoded op lists, opaque numeric handles).
#[cfg(all(feature = "v8-backend", feature = "webgpu"))]
fn install_webgpu_natives_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4};
    use lumen_paint::webgpu_compute;

    rt.register_native(
        "_lumen_webgpu_adapter_info",
        into_v8_fn0(|| -> String {
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

    rt.register_native(
        "_lumen_webgpu_validate_shader",
        into_v8_fn1(|code: String| -> String { webgpu_compute::validate_wgsl(&code).unwrap_or_default() }),
    )?;

    rt.register_native(
        "_lumen_webgpu_buffer_create",
        into_v8_fn3(|size: f64, usage: u32, mapped: bool| -> f64 {
            webgpu_compute::buffer_create(size as u64, usage, mapped).unwrap_or(0) as f64
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_buffer_write",
        into_v8_fn3(|id: f64, offset: f64, data: Vec<u8>| -> bool {
            webgpu_compute::buffer_write(id as u64, offset as u64, &data)
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_buffer_read",
        into_v8_fn3(|id: f64, offset: f64, size: f64| -> Option<Vec<u8>> {
            webgpu_compute::buffer_read(id as u64, offset as u64, size as u64)
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_buffer_destroy",
        into_v8_fn1(|id: f64| {
            webgpu_compute::buffer_destroy(id as u64);
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_shader_create",
        into_v8_fn1(|code: String| -> f64 { webgpu_compute::shader_create(&code).unwrap_or(0) as f64 }),
    )?;

    rt.register_native(
        "_lumen_webgpu_compute_pipeline_create",
        into_v8_fn2(|shader: f64, entry: String| -> f64 {
            webgpu_compute::compute_pipeline_create(shader as u64, &entry).unwrap_or(0) as f64
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_pipeline_bind_group_layout",
        into_v8_fn2(|pipeline: f64, group: u32| -> f64 {
            webgpu_compute::pipeline_bind_group_layout(pipeline as u64, group).unwrap_or(0) as f64
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_bind_group_create",
        into_v8_fn2(|layout: f64, entries: String| -> f64 {
            webgpu_bind_group_create_impl(layout, &entries)
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_compute_pipeline_destroy",
        into_v8_fn1(|id: f64| {
            webgpu_compute::compute_pipeline_destroy(id as u64);
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_texture_create",
        into_v8_fn4(|width: f64, height: f64, format: String, usage: u32| -> f64 {
            webgpu_compute::texture_create(width as u32, height as u32, &format, usage).unwrap_or(0) as f64
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_texture_destroy",
        into_v8_fn1(|id: f64| {
            webgpu_compute::texture_destroy(id as u64);
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_render_pipeline_create",
        into_v8_fn1(|config: String| -> f64 { webgpu_render_pipeline_create_impl(&config) }),
    )?;

    rt.register_native(
        "_lumen_webgpu_render_pipeline_bind_group_layout",
        into_v8_fn2(|pipeline: f64, group: u32| -> f64 {
            webgpu_compute::render_pipeline_bind_group_layout(pipeline as u64, group).unwrap_or(0) as f64
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_render_pipeline_destroy",
        into_v8_fn1(|id: f64| {
            webgpu_compute::render_pipeline_destroy(id as u64);
        }),
    )?;

    rt.register_native(
        "_lumen_webgpu_submit",
        into_v8_fn1(|ops_json: String| -> bool { webgpu_submit_impl(&ops_json) }),
    )?;

    rt.register_native(
        "_lumen_webgpu_canvas_present",
        into_v8_fn2(|nid: u32, texture: f64| -> bool {
            let Some((w, h, rgba)) = webgpu_compute::texture_read_rgba(texture as u64) else {
                return false;
            };
            crate::canvas2d::present_rgba(nid, w, h, &rgba);
            true
        }),
    )?;

    Ok(())
}

/// Shared JSON-decode body for `_lumen_webgpu_bind_group_create`, factored
/// out of the closure so both backends' `entries` parsing logic can stay
/// byte-for-byte identical to [`install_webgpu_natives`]'s inline closure.
#[cfg(all(feature = "v8-backend", feature = "webgpu"))]
fn webgpu_bind_group_create_impl(layout: f64, entries: &str) -> f64 {
    use lumen_paint::webgpu_compute;

    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(entries) else {
        return 0.0;
    };
    let mut decoded = Vec::with_capacity(parsed.len());
    for e in &parsed {
        let u = |k: &str| e.get(k).and_then(serde_json::Value::as_u64);
        let (Some(binding), Some(buffer)) = (u("binding"), u("buffer")) else {
            return 0.0;
        };
        decoded.push(webgpu_compute::BufferBindEntry {
            binding: binding as u32,
            buffer,
            offset: u("offset").unwrap_or(0),
            size: u("size").unwrap_or(0),
        });
    }
    webgpu_compute::bind_group_create(layout as u64, &decoded).unwrap_or(0) as f64
}

/// Shared JSON-decode body for `_lumen_webgpu_render_pipeline_create`. Twin
/// of [`webgpu_bind_group_create_impl`] — see [`install_webgpu_natives`]'s
/// inline closure for the field-by-field rationale.
#[cfg(all(feature = "v8-backend", feature = "webgpu"))]
fn webgpu_render_pipeline_create_impl(config: &str) -> f64 {
    use lumen_paint::webgpu_compute;

    let Ok(cfg) = serde_json::from_str::<serde_json::Value>(config) else {
        return 0.0;
    };
    let u = |k: &str| cfg.get(k).and_then(serde_json::Value::as_u64);
    let s = |k: &str| cfg.get(k).and_then(serde_json::Value::as_str).unwrap_or("");
    let (Some(vs), Some(fs)) = (u("vs"), u("fs")) else {
        return 0.0;
    };
    let mut buffers = Vec::new();
    if let Some(bufs) = cfg.get("buffers").and_then(serde_json::Value::as_array) {
        for b in bufs {
            let mut attributes = Vec::new();
            if let Some(attrs) = b.get("attributes").and_then(serde_json::Value::as_array) {
                for a in attrs {
                    let Some(fmt) = a.get("format").and_then(serde_json::Value::as_str) else {
                        return 0.0;
                    };
                    attributes.push(webgpu_compute::VertexAttr {
                        format: fmt.to_string(),
                        offset: a.get("offset").and_then(serde_json::Value::as_u64).unwrap_or(0),
                        shader_location: a
                            .get("shaderLocation")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0) as u32,
                    });
                }
            }
            buffers.push(webgpu_compute::VertexBufferLayout {
                array_stride: b.get("arrayStride").and_then(serde_json::Value::as_u64).unwrap_or(0),
                instance_step: b.get("instance").and_then(serde_json::Value::as_bool).unwrap_or(false),
                attributes,
            });
        }
    }
    webgpu_compute::render_pipeline_create(vs, s("vsEntry"), fs, s("fsEntry"), s("format"), s("topology"), &buffers)
        .unwrap_or(0) as f64
}

/// Shared JSON-decode body for `_lumen_webgpu_submit`. Twin of
/// [`webgpu_bind_group_create_impl`] — see [`install_webgpu_natives`]'s
/// inline closure for the per-op-kind field rationale.
#[cfg(all(feature = "v8-backend", feature = "webgpu"))]
fn webgpu_submit_impl(ops_json: &str) -> bool {
    use lumen_paint::webgpu_compute;

    let Ok(ops) = serde_json::from_str::<Vec<serde_json::Value>>(ops_json) else {
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
            "computePass" => {
                let Some(cmds) = op.get("cmds").and_then(|v| v.as_array()) else {
                    return false;
                };
                let mut commands = Vec::with_capacity(cmds.len());
                for c in cmds {
                    let ck = c.get("c").and_then(|v| v.as_str()).unwrap_or("");
                    match ck {
                        "setPipeline" => {
                            let Some(p) = c.get("pipeline").and_then(serde_json::Value::as_u64) else {
                                return false;
                            };
                            commands.push(webgpu_compute::ComputeCmd::SetPipeline(p));
                        }
                        "setBindGroup" => {
                            let (Some(index), Some(bind_group)) = (
                                c.get("index").and_then(serde_json::Value::as_u64),
                                c.get("bindGroup").and_then(serde_json::Value::as_u64),
                            ) else {
                                return false;
                            };
                            commands.push(webgpu_compute::ComputeCmd::SetBindGroup {
                                index: index as u32,
                                bind_group,
                            });
                        }
                        "dispatch" => {
                            let g = |k: &str| {
                                c.get(k).and_then(serde_json::Value::as_u64).unwrap_or(1) as u32
                            };
                            commands.push(webgpu_compute::ComputeCmd::Dispatch {
                                x: g("x"),
                                y: g("y"),
                                z: g("z"),
                            });
                        }
                        _ => return false,
                    }
                }
                decoded.push(webgpu_compute::GpuOp::ComputePass { commands });
            }
            "renderPass" => {
                let f = |k: &str| op.get(k).and_then(serde_json::Value::as_u64);
                let Some(color_texture) = f("colorTexture") else {
                    return false;
                };
                let clear = op.get("clear").and_then(|c| c.as_array()).map(|arr| {
                    let g = |i: usize| arr.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                    [g(0), g(1), g(2), g(3)]
                });
                let Some(cmds) = op.get("cmds").and_then(|v| v.as_array()) else {
                    return false;
                };
                let mut commands = Vec::with_capacity(cmds.len());
                for c in cmds {
                    let ck = c.get("c").and_then(|v| v.as_str()).unwrap_or("");
                    let cu = |k: &str| c.get(k).and_then(serde_json::Value::as_u64);
                    let ci = |k: &str| c.get(k).and_then(serde_json::Value::as_i64);
                    match ck {
                        "setPipeline" => {
                            let Some(p) = cu("pipeline") else { return false };
                            commands.push(webgpu_compute::RenderCmd::SetPipeline(p));
                        }
                        "setBindGroup" => {
                            let (Some(index), Some(bind_group)) = (cu("index"), cu("bindGroup")) else {
                                return false;
                            };
                            commands.push(webgpu_compute::RenderCmd::SetBindGroup {
                                index: index as u32,
                                bind_group,
                            });
                        }
                        "setVertexBuffer" => {
                            let Some(buffer) = cu("buffer") else { return false };
                            commands.push(webgpu_compute::RenderCmd::SetVertexBuffer {
                                slot: cu("slot").unwrap_or(0) as u32,
                                buffer,
                                offset: cu("offset").unwrap_or(0),
                                size: cu("size").unwrap_or(0),
                            });
                        }
                        "setIndexBuffer" => {
                            let Some(buffer) = cu("buffer") else { return false };
                            commands.push(webgpu_compute::RenderCmd::SetIndexBuffer {
                                buffer,
                                format_u16: c.get("u16").and_then(serde_json::Value::as_bool).unwrap_or(false),
                                offset: cu("offset").unwrap_or(0),
                                size: cu("size").unwrap_or(0),
                            });
                        }
                        "draw" => {
                            commands.push(webgpu_compute::RenderCmd::Draw {
                                vertex_count: cu("vertexCount").unwrap_or(0) as u32,
                                instance_count: cu("instanceCount").unwrap_or(1) as u32,
                                first_vertex: cu("firstVertex").unwrap_or(0) as u32,
                                first_instance: cu("firstInstance").unwrap_or(0) as u32,
                            });
                        }
                        "drawIndexed" => {
                            commands.push(webgpu_compute::RenderCmd::DrawIndexed {
                                index_count: cu("indexCount").unwrap_or(0) as u32,
                                instance_count: cu("instanceCount").unwrap_or(1) as u32,
                                first_index: cu("firstIndex").unwrap_or(0) as u32,
                                base_vertex: ci("baseVertex").unwrap_or(0) as i32,
                                first_instance: cu("firstInstance").unwrap_or(0) as u32,
                            });
                        }
                        _ => return false,
                    }
                }
                decoded.push(webgpu_compute::GpuOp::RenderPass {
                    color_texture,
                    clear,
                    commands,
                });
            }
            "copyTexToBuf" => {
                let f = |k: &str| op.get(k).and_then(serde_json::Value::as_u64);
                let (Some(texture), Some(buffer)) = (f("texture"), f("buffer")) else {
                    return false;
                };
                decoded.push(webgpu_compute::GpuOp::CopyTextureToBuffer {
                    texture,
                    buffer,
                    buffer_offset: f("bufferOffset").unwrap_or(0),
                    bytes_per_row: f("bytesPerRow").unwrap_or(0) as u32,
                    rows_per_image: f("rowsPerImage").unwrap_or(1) as u32,
                    width: f("width").unwrap_or(1) as u32,
                    height: f("height").unwrap_or(1) as u32,
                });
            }
            _ => return false,
        }
    }
    webgpu_compute::submit(&decoded)
}

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
    fn gpu_canvas_context_unconfigure_drops_texture() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // configure → getCurrentTexture allocates; unconfigure clears it so the next
            // getCurrentTexture lazily re-creates a fresh one (present registry stays consistent).
            let ok: bool = ctx
                .eval(
                    r#"
                    var canvas = { width: 64, height: 64, __nid__: 5 };
                    var c = new GPUCanvasContext(canvas);
                    c.configure({ format: 'rgba8unorm', usage: GPUTextureUsage.RENDER_ATTACHMENT });
                    var t1 = c.getCurrentTexture();
                    c.unconfigure();
                    var cleared = (c._texture === null);
                    var t2 = c.getCurrentTexture();
                    cleared && t2 instanceof GPUTexture && t2.width === 64
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn gpu_canvas_present_native_unknown_texture_is_false() {
        // Present native rejects an unknown texture handle (no GPU readback) without panicking.
        // Registered only with the `webgpu` feature; without it the JS bridge is simply absent.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let has_native: bool = ctx
                .eval("typeof _lumen_webgpu_canvas_present === 'function'")
                .unwrap();
            if !has_native {
                return;
            }
            let result: bool = ctx
                .eval("_lumen_webgpu_canvas_present(123, 999999)")
                .unwrap();
            assert!(!result, "unknown texture handle must present nothing");
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

    #[test]
    fn compute_pipeline_api_shape() {
        // Without the `webgpu` feature / a GPU the compute API still exists and is callable
        // (no-op). Exercises createComputePipeline → getBindGroupLayout → createBindGroup →
        // beginComputePass → setPipeline/setBindGroup/dispatchWorkgroups → end → submit.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var mod = d.createShaderModule({ code:
                      '@group(0) @binding(0) var<storage, read_write> v: array<u32>;' +
                      '@compute @workgroup_size(1) fn main() { v[0] = 1u; }' });
                    var pipe = d.createComputePipeline({ layout: 'auto', compute: { module: mod, entryPoint: 'main' } });
                    var bgl  = pipe.getBindGroupLayout(0);
                    var buf  = d.createBuffer({ size: 16, usage: GPUBufferUsage.STORAGE });
                    var bg   = d.createBindGroup({ layout: bgl, entries: [{ binding: 0, resource: { buffer: buf } }] });
                    var enc  = d.createCommandEncoder({});
                    var pass = enc.beginComputePass();
                    pass.setPipeline(pipe);
                    pass.setBindGroup(0, bg);
                    pass.dispatchWorkgroups(4);
                    pass.end();
                    d.queue.submit([enc.finish()]);
                    pipe instanceof GPUComputePipeline
                      && bgl instanceof GPUBindGroupLayout
                      && bg instanceof GPUBindGroup
                      && typeof pass.dispatchWorkgroups === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok, "compute pipeline API must be callable end-to-end");
        });
    }

    #[test]
    fn compute_pass_records_op_on_encoder() {
        // The compute pass must record a single computePass op (with its command list)
        // onto the parent encoder so queue.submit can flush it.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginComputePass();
                    pass.setPipeline({ _id: 7 });
                    pass.setBindGroup(0, { _id: 9 });
                    pass.dispatchWorkgroups(2, 3, 4);
                    pass.end();
                    var op = enc._ops[0];
                    op.op === 'computePass'
                      && op.cmds.length === 3
                      && op.cmds[0].c === 'setPipeline' && op.cmds[0].pipeline === 7
                      && op.cmds[1].c === 'setBindGroup' && op.cmds[1].index === 0 && op.cmds[1].bindGroup === 9
                      && op.cmds[2].c === 'dispatch' && op.cmds[2].x === 2 && op.cmds[2].y === 3 && op.cmds[2].z === 4
                    "#,
                )
                .unwrap();
            assert!(ok, "compute pass must record its command list onto the encoder");
        });
    }

    #[test]
    fn dispatch_workgroups_defaults_y_z_to_one() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginComputePass();
                    pass.dispatchWorkgroups(8);
                    pass.end();
                    var c = enc._ops[0].cmds[0];
                    c.x === 8 && c.y === 1 && c.z === 1
                    "#,
                )
                .unwrap();
            assert!(ok, "omitted dispatch y/z must default to 1");
        });
    }

    // Real-GPU path: end-to-end compute. Only meaningful with the `webgpu` feature AND an
    // available adapter; skips on headless CI without a GPU.
    #[cfg(feature = "webgpu")]
    #[test]
    fn real_backend_runs_compute_shader() {
        if !lumen_paint::webgpu_compute::is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // Doubling shader: storage buffer values are multiplied by 2 on the GPU, then
            // copied to a MAP_READ buffer and read back through the JS GPUBuffer API.
            let doubled: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var mod = d.createShaderModule({ code:
                      '@group(0) @binding(0) var<storage, read_write> data: array<u32>;' +
                      '@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {' +
                      '  data[id.x] = data[id.x] * 2u; }' });
                    var pipe = d.createComputePipeline({ layout: 'auto', compute: { module: mod, entryPoint: 'main' } });
                    var storage = d.createBuffer({ size: 16,
                      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST });
                    var readback = d.createBuffer({ size: 16, usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
                    d.queue.writeBuffer(storage, 0, new Uint32Array([1, 2, 3, 4]));
                    var bg = d.createBindGroup({ layout: pipe.getBindGroupLayout(0),
                      entries: [{ binding: 0, resource: { buffer: storage } }] });
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginComputePass();
                    pass.setPipeline(pipe);
                    pass.setBindGroup(0, bg);
                    pass.dispatchWorkgroups(4);
                    pass.end();
                    enc.copyBufferToBuffer(storage, 0, readback, 0, 16);
                    d.queue.submit([enc.finish()]);
                    readback.mapAsync(GPUMapMode.READ);
                    var v = new Uint32Array(readback.getMappedRange());
                    v[0] === 2 && v[1] === 4 && v[2] === 6 && v[3] === 8
                    "#,
                )
                .unwrap();
            assert!(doubled, "real GPU compute shader must double the buffer");
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

    #[test]
    fn render_pipeline_api_shape() {
        // Without the `webgpu` feature / a GPU the render API still exists and is callable
        // (no-op). Exercises createTexture → createRenderPipeline → getBindGroupLayout →
        // beginRenderPass → setPipeline/setVertexBuffer/draw → end → copyTextureToBuffer → submit.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var mod = d.createShaderModule({ code:
                      '@vertex fn vs(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4<f32>(p, 0.0, 1.0); }' +
                      '@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }' });
                    var pipe = d.createRenderPipeline({
                      layout: 'auto',
                      vertex: { module: mod, entryPoint: 'vs',
                        buffers: [{ arrayStride: 8, attributes: [{ format: 'float32x2', offset: 0, shaderLocation: 0 }] }] },
                      fragment: { module: mod, entryPoint: 'fs', targets: [{ format: 'rgba8unorm' }] },
                      primitive: { topology: 'triangle-list' }
                    });
                    var bgl = pipe.getBindGroupLayout(0);
                    var tex = d.createTexture({ size: { width: 4, height: 4 }, format: 'rgba8unorm',
                      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
                    var vbuf = d.createBuffer({ size: 24, usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginRenderPass({ colorAttachments: [{ view: tex.createView(),
                      loadOp: 'clear', storeOp: 'store', clearValue: { r: 0, g: 0, b: 0, a: 1 } }] });
                    pass.setPipeline(pipe);
                    pass.setVertexBuffer(0, vbuf);
                    pass.draw(3);
                    pass.end();
                    d.queue.submit([enc.finish()]);
                    pipe instanceof GPURenderPipeline
                      && bgl instanceof GPUBindGroupLayout
                      && tex instanceof GPUTexture
                      && typeof pass.draw === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok, "render pipeline API must be callable end-to-end");
        });
    }

    #[test]
    fn render_pass_records_op_on_encoder() {
        // The render pass must record a single renderPass op (target texture id + clear +
        // command list) onto the parent encoder so queue.submit can flush it.
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var view = { _textureId: 42 };
                    var pass = enc.beginRenderPass({ colorAttachments: [{ view: view,
                      loadOp: 'clear', clearValue: { r: 1, g: 0, b: 0, a: 1 } }] });
                    pass.setPipeline({ _id: 7 });
                    pass.setVertexBuffer(0, { _id: 5 });
                    pass.draw(3, 1, 0, 0);
                    pass.end();
                    var op = enc._ops[0];
                    op.op === 'renderPass'
                      && op.colorTexture === 42
                      && op.clear[0] === 1 && op.clear[3] === 1
                      && op.cmds.length === 3
                      && op.cmds[0].c === 'setPipeline' && op.cmds[0].pipeline === 7
                      && op.cmds[1].c === 'setVertexBuffer' && op.cmds[1].buffer === 5
                      && op.cmds[2].c === 'draw' && op.cmds[2].vertexCount === 3
                    "#,
                )
                .unwrap();
            assert!(ok, "render pass must record its command list onto the encoder");
        });
    }

    #[test]
    fn render_pass_load_op_load_has_no_clear() {
        // loadOp: 'load' must record clear === null (no clear color).
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginRenderPass({ colorAttachments: [{ view: { _textureId: 1 },
                      loadOp: 'load' }] });
                    pass.end();
                    enc._ops[0].clear === null
                    "#,
                )
                .unwrap();
            assert!(ok, "loadOp 'load' must record a null clear");
        });
    }

    // Real-GPU path: end-to-end render. Only meaningful with the `webgpu` feature AND an
    // available adapter; skips on headless CI without a GPU.
    #[cfg(feature = "webgpu")]
    #[test]
    fn real_backend_renders_triangle() {
        if !lumen_paint::webgpu_compute::is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // Full-screen triangle filled green, rendered to a 4×4 texture, then read back
            // through a real MAP_READ buffer via copyTextureToBuffer. bytesPerRow padded to 256.
            let green: bool = ctx
                .eval(
                    r#"
                    var d = new GPUDevice({});
                    var mod = d.createShaderModule({ code:
                      '@vertex fn vs(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> { return vec4<f32>(p, 0.0, 1.0); }' +
                      '@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }' });
                    var pipe = d.createRenderPipeline({
                      layout: 'auto',
                      vertex: { module: mod, entryPoint: 'vs',
                        buffers: [{ arrayStride: 8, attributes: [{ format: 'float32x2', offset: 0, shaderLocation: 0 }] }] },
                      fragment: { module: mod, entryPoint: 'fs', targets: [{ format: 'rgba8unorm' }] },
                      primitive: { topology: 'triangle-list' }
                    });
                    var verts = new Float32Array([-1, -1, 3, -1, -1, 3]);
                    var vbuf = d.createBuffer({ size: verts.byteLength,
                      usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST });
                    d.queue.writeBuffer(vbuf, 0, verts);
                    var tex = d.createTexture({ size: { width: 4, height: 4 }, format: 'rgba8unorm',
                      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC });
                    var readback = d.createBuffer({ size: 256 * 4,
                      usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST });
                    var enc = d.createCommandEncoder({});
                    var pass = enc.beginRenderPass({ colorAttachments: [{ view: tex.createView(),
                      loadOp: 'clear', storeOp: 'store', clearValue: { r: 0, g: 0, b: 0, a: 1 } }] });
                    pass.setPipeline(pipe);
                    pass.setVertexBuffer(0, vbuf);
                    pass.draw(3);
                    pass.end();
                    enc.copyTextureToBuffer({ texture: tex },
                      { buffer: readback, bytesPerRow: 256, rowsPerImage: 4 },
                      { width: 4, height: 4 });
                    d.queue.submit([enc.finish()]);
                    // Center pixel (row 2, col 2): offset 2*256 + 2*4.
                    readback.mapAsync(GPUMapMode.READ);
                    var px = new Uint8Array(readback.getMappedRange(2 * 256 + 2 * 4, 4));
                    px[0] === 0 && px[1] === 255 && px[2] === 0 && px[3] === 255
                    "#,
                )
                .unwrap();
            assert!(green, "real GPU render must fill the triangle green");
        });
    }
}

/// V8-backend counterpart of the [`tests`] module above (Ph3 V8 migration
/// S9). WebGPU carries no GC roots (see [`install_webgpu_bindings_v8`]'s doc
/// comment), so this is a plain shim-installation smoke test — the harder S9
/// risk (GC roots) is covered by `webassembly::tests_v8` instead.
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::{JsRuntime, JsValue};

    #[test]
    fn v8_navigator_gpu_exists() {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("globalThis.navigator = globalThis.navigator || {};").unwrap();
        super::install_webgpu_bindings_v8(&rt).unwrap();
        let ok = rt
            .eval("typeof navigator.gpu !== 'undefined' && typeof navigator.gpu.requestAdapter === 'function'")
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }
}
