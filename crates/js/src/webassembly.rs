//! WebAssembly JavaScript Interface (W3C §7), backed by Lumen's MVP interpreter.
//!
//! Stage 1 of U-4: `WebAssembly.compile`/`validate`/`instantiate` now decode and
//! **execute** real bytecode through [`crate::wasm`]. `Instance.exports` contains
//! callable functions, exported memory/globals, instead of the previous empty
//! Phase 0 stubs. `Memory`/`Table`/`Global`/`Tag`/`Exception` standalone classes
//! are unchanged (used when constructed directly by JS).
//!
//! Numeric values cross the JS↔WASM boundary by type: `i64` rides as a JS
//! `BigInt` (full 64-bit precision, per the W3C WebAssembly JS Interface),
//! the others as `Number` — for exported function arguments/results, host
//! import arguments/results, and exported globals.
//!
//! Live memory aliasing (U-4b): exported `Memory.buffer` is a single, stable JS
//! `ArrayBuffer` synchronized with Rust-owned linear memory at WASM call
//! boundaries — JS writes (`HEAP32[i] = x`) are copied into Rust before each
//! exported call, and WASM writes are copied back into the *same* buffer after,
//! so `HEAP32 = new Int32Array(memory.buffer)` stays coherent and a captured
//! view keeps reflecting later writes. This is exact for the single-agent model
//! (ADR-014). Remaining boundary (documented): a host import cannot observe
//! writes made earlier in the same in-flight call; an *imported* `Memory` is not
//! aliased to the instance's internal memory (only the exported-memory path is).

use rquickjs::{ArrayBuffer, Ctx, Exception, Function, Persistent, TypedArray};

use crate::wasm;

/// Native backing for `__lumen_wasm_compile`.
///
/// Free function so the single `'js` ties `ctx` to the `TypedArray` handle.
/// Accepts the bytes as a `Uint8Array` (the JS engine's `Vec<u8>` `FromJs`
/// requires a real `Array`, which the shim does not pass).
fn wasm_compile_native<'js>(ctx: Ctx<'js>, bytes: TypedArray<'js, u8>) -> rquickjs::Result<u32> {
    let slice = bytes.as_bytes().unwrap_or(&[]);
    wasm::compile(slice).map_err(|e| Exception::throw_message(&ctx, &e))
}

/// Native backing for `__lumen_wasm_instantiate`.
///
/// A free function (not a closure) so the single `'js` lifetime ties `ctx` to
/// the incoming `Function` handles — required for [`Persistent::save`], which
/// closures cannot express via inferred HRTB lifetimes.
fn wasm_instantiate_native<'js>(
    ctx: Ctx<'js>,
    module_id: u32,
    funcs: Vec<Function<'js>>,
    globals: Vec<f64>,
) -> rquickjs::Result<u32> {
    let persistent: Vec<Persistent<Function<'static>>> =
        funcs.into_iter().map(|f| Persistent::save(&ctx, f)).collect();
    wasm::instantiate(&ctx, module_id, persistent, globals)
        .map_err(|e| Exception::throw_message(&ctx, &e))
}

/// Native backing for `__lumen_wasm_call`.
///
/// Each JS argument is coerced to its declared WASM parameter type and each
/// result is mapped back to JS: an `i64` rides the boundary as a `BigInt`
/// (W3C WebAssembly JS Interface), so 64-bit integers keep full precision
/// instead of being squeezed through an `f64`. Free function so the single
/// `'js` ties `ctx` to the incoming/returned `Value` handles.
fn wasm_call_native<'js>(
    ctx: Ctx<'js>,
    inst_id: u32,
    func_idx: u32,
    args: Vec<rquickjs::Value<'js>>,
) -> rquickjs::Result<Vec<rquickjs::Value<'js>>> {
    let (params, _results) = wasm::func_signature(inst_id, func_idx).unwrap_or_default();
    let typed_args: Vec<wasm::value::Value> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let ty = params.get(i).copied().unwrap_or(wasm::value::ValType::F64);
            wasm::js_value_to_wasm(a, ty)
        })
        .collect();
    let results = wasm::call_typed(&ctx, inst_id, func_idx, &typed_args)
        .map_err(|e| Exception::throw_message(&ctx, &e))?;
    Ok(results.into_iter().map(|v| wasm::wasm_value_to_js(&ctx, v)).collect())
}

/// Native backing for `__lumen_wasm_mem_buffer` — returns the instance's full
/// linear memory as a fresh JS `ArrayBuffer` (a single bulk copy). The shim
/// uses this to build the stable exported `Memory.buffer` and to refresh it
/// after each call. Free function so the single `'js` ties `ctx` to the
/// returned buffer handle.
fn wasm_mem_buffer_native<'js>(ctx: Ctx<'js>, inst_id: u32) -> rquickjs::Result<ArrayBuffer<'js>> {
    let bytes = wasm::mem_read_all(inst_id);
    ArrayBuffer::new_copy(ctx, &bytes)
}

/// Native backing for `__lumen_wasm_global_get` — returns the global's value as
/// a `BigInt` (i64) or `Number` (other types). Free function so `'js` ties
/// `ctx` to the returned handle.
fn wasm_global_get_native<'js>(
    ctx: Ctx<'js>,
    inst_id: u32,
    idx: u32,
) -> rquickjs::Value<'js> {
    match wasm::global_value(inst_id, idx) {
        Some(v) => wasm::wasm_value_to_js(&ctx, v),
        None => rquickjs::Value::new_float(ctx, 0.0),
    }
}

/// Native backing for `__lumen_wasm_global_set` — accepts a `BigInt` (i64) or
/// `Number`, coerced to the global's declared type (read from its current
/// value).
fn wasm_global_set_native(inst_id: u32, idx: u32, v: rquickjs::Value) -> bool {
    let Some(cur) = wasm::global_value(inst_id, idx) else {
        return false;
    };
    let wv = wasm::js_value_to_wasm(&v, cur.val_type());
    wasm::global_set_value(inst_id, idx, wv)
}

/// Register the `__lumen_wasm_*` native bindings used by the JS shim.
fn install_native_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    let g = ctx.globals();

    g.set(
        "__lumen_wasm_validate",
        Function::new(ctx.clone(), |bytes: TypedArray<u8>| -> bool {
            wasm::validate(bytes.as_bytes().unwrap_or(&[]))
        })?,
    )?;

    g.set("__lumen_wasm_compile", Function::new(ctx.clone(), wasm_compile_native)?)?;

    g.set(
        "__lumen_wasm_module_exports",
        Function::new(ctx.clone(), |id: u32| -> String { wasm::module_exports_json(id) })?,
    )?;

    g.set(
        "__lumen_wasm_module_imports",
        Function::new(ctx.clone(), |id: u32| -> String { wasm::module_imports_json(id) })?,
    )?;

    g.set(
        "__lumen_wasm_instantiate",
        Function::new(ctx.clone(), wasm_instantiate_native)?,
    )?;

    g.set("__lumen_wasm_call", Function::new(ctx.clone(), wasm_call_native)?)?;

    g.set(
        "__lumen_wasm_mem_size",
        Function::new(ctx.clone(), |inst_id: u32| -> u32 { wasm::mem_size(inst_id) })?,
    )?;
    g.set(
        "__lumen_wasm_mem_grow",
        Function::new(ctx.clone(), |inst_id: u32, delta: u32| -> i32 {
            wasm::mem_grow(inst_id, delta)
        })?,
    )?;
    g.set(
        "__lumen_wasm_mem_read",
        Function::new(ctx.clone(), |inst_id: u32, offset: u32, len: u32| -> Vec<u8> {
            wasm::mem_read(inst_id, offset, len)
        })?,
    )?;
    g.set(
        "__lumen_wasm_mem_write",
        Function::new(
            ctx.clone(),
            |inst_id: u32, offset: u32, bytes: TypedArray<u8>| -> bool {
                wasm::mem_write(inst_id, offset, bytes.as_bytes().unwrap_or(&[]))
            },
        )?,
    )?;
    g.set(
        "__lumen_wasm_mem_buffer",
        Function::new(ctx.clone(), wasm_mem_buffer_native)?,
    )?;
    g.set("__lumen_wasm_global_get", Function::new(ctx.clone(), wasm_global_get_native)?)?;
    g.set(
        "__lumen_wasm_global_set",
        Function::new(ctx.clone(), wasm_global_set_native)?,
    )?;

    Ok(())
}

/// Install WebAssembly API bindings into the JS context.
pub fn install_webassembly_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    install_native_bindings(ctx)?;
    ctx.eval::<(), _>(WEBASSEMBLY_SHIM)?;
    Ok(())
}

const WEBASSEMBLY_SHIM: &str = r#"
(function() {
  'use strict';

  // ── Error classes ──────────────────────────────────────────────────────────
  class CompileError extends Error {
    constructor(msg) { super(msg); this.name = 'CompileError'; }
  }
  class LinkError extends Error {
    constructor(msg) { super(msg); this.name = 'LinkError'; }
  }
  class RuntimeError extends Error {
    constructor(msg) { super(msg); this.name = 'RuntimeError'; }
  }

  function errMsg(e) { return String((e && e.message) ? e.message : e); }

  // Coerce a BufferSource to a Uint8Array view of its bytes.
  function bytesOf(bufferSource) {
    if (bufferSource instanceof ArrayBuffer) return new Uint8Array(bufferSource);
    if (ArrayBuffer.isView(bufferSource)) {
      return new Uint8Array(bufferSource.buffer, bufferSource.byteOffset, bufferSource.byteLength);
    }
    throw new TypeError('expected a BufferSource');
  }

  // ── WebAssembly.Module ─────────────────────────────────────────────────────
  // Decodes the module via the native engine and keeps its registry id.
  class Module {
    constructor(bufferSource) {
      if (bufferSource === undefined || bufferSource === null) {
        throw new CompileError('Module requires a BufferSource');
      }
      var u8;
      try { u8 = bytesOf(bufferSource); }
      catch (e) { throw new CompileError(errMsg(e)); }
      try { this._id = __lumen_wasm_compile(u8); }
      catch (e) { throw new CompileError(errMsg(e)); }
      this._byteLength = u8.length;
    }
  }

  Module.exports = function(module) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    var desc = JSON.parse(__lumen_wasm_module_exports(module._id));
    return desc.map(function(e) { return { name: e.name, kind: e.kind }; });
  };
  Module.imports = function(module) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    var desc = JSON.parse(__lumen_wasm_module_imports(module._id));
    return desc.map(function(e) { return { module: e.module, name: e.name, kind: e.kind }; });
  };
  Module.customSections = function(module, sectionName) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    return []; // custom sections are skipped by the decoder (MVP)
  };

  // ── WebAssembly.Memory ────────────────────────────────────────────────────
  class Memory {
    constructor(descriptor) {
      if (!descriptor || typeof descriptor !== 'object') {
        throw new TypeError('Memory descriptor must be an object');
      }
      var initial = descriptor.initial | 0;
      if (initial < 0) throw new RangeError('Memory initial must be >= 0');
      var maximum = (descriptor.maximum !== undefined) ? (descriptor.maximum | 0) : 65536;
      this._pages = initial;
      this._max = maximum;
      this._buffer = new ArrayBuffer(initial * 65536);
    }
    get buffer() { return this._buffer; }
    grow(delta) {
      var d = delta | 0;
      if (d < 0) throw new RangeError('grow delta must be >= 0');
      var prev = this._pages;
      var next = prev + d;
      if (next > this._max) return -1;
      this._pages = next;
      this._buffer = new ArrayBuffer(next * 65536);
      return prev;
    }
  }

  // ── WebAssembly.Table ─────────────────────────────────────────────────────
  class Table {
    constructor(descriptor) {
      if (!descriptor || typeof descriptor !== 'object') {
        throw new TypeError('Table descriptor must be an object');
      }
      var element = descriptor.element;
      if (element !== 'anyfunc' && element !== 'funcref' && element !== 'externref') {
        throw new TypeError('Table element must be "funcref" or "externref"');
      }
      var initial = descriptor.initial | 0;
      this._element = element;
      this._entries = new Array(initial).fill(null);
      this._max = (descriptor.maximum !== undefined) ? (descriptor.maximum | 0) : Infinity;
    }
    get length() { return this._entries.length; }
    get(index) {
      if (index < 0 || index >= this._entries.length) throw new RangeError('Table index out of bounds');
      return this._entries[index];
    }
    set(index, value) {
      if (index < 0 || index >= this._entries.length) throw new RangeError('Table index out of bounds');
      this._entries[index] = (value === undefined) ? null : value;
    }
    grow(delta, initValue) {
      var d = delta | 0;
      if (d < 0) throw new RangeError('grow delta must be >= 0');
      var prev = this._entries.length;
      var next = prev + d;
      if (next > this._max) return -1;
      var fill = (initValue === undefined) ? null : initValue;
      for (var i = 0; i < d; i++) this._entries.push(fill);
      return prev;
    }
  }

  // ── WebAssembly.Global ────────────────────────────────────────────────────
  class Global {
    constructor(descriptor, value) {
      if (!descriptor || typeof descriptor !== 'object') {
        throw new TypeError('Global descriptor must be an object');
      }
      var mutable = !!descriptor.mutable;
      var type = descriptor.value || descriptor.type || 'i32';
      var allowed = ['i32', 'i64', 'f32', 'f64', 'anyfunc', 'funcref', 'externref'];
      if (!allowed.includes(type)) throw new TypeError('Unknown global value type');
      this._mutable = mutable;
      this._type = type;
      this._value = (value !== undefined) ? value : 0;
    }
    get value() { return this._value; }
    set value(v) {
      if (!this._mutable) throw new TypeError('Cannot assign to immutable global');
      this._value = v;
    }
    valueOf() { return this._value; }
  }

  // ── WebAssembly.Tag / Exception (Exceptions proposal stub) ────────────────
  class Tag {
    constructor(type) { this._type = type || { parameters: [] }; }
  }
  class WasmException {
    constructor(tag, payload) { this._tag = tag; this._payload = payload || []; }
    getArg(tag, index) { if (tag !== this._tag) throw new TypeError('Wrong tag'); return this._payload[index]; }
    is(tag) { return this._tag === tag; }
  }

  // ── Exported wrappers (backed by the native instance) ─────────────────────

  // `memRef.mem` (if set) is the instance's exported memory; its buffer is
  // synchronized with Rust-owned linear memory around every exported call so the
  // `new Int32Array(memory.buffer)` (emscripten HEAP) pattern stays coherent.
  function makeExportFn(instId, funcIdx, memRef) {
    return function() {
      // Pass arguments through untouched: the native side coerces each to its
      // declared WASM type. `+arg` would throw on a BigInt and lose precision
      // on a large i64, so we must not eagerly numify here.
      var args = new Array(arguments.length);
      for (var i = 0; i < arguments.length; i++) args[i] = arguments[i];
      var mem = memRef && memRef.mem;
      // Push JS-side buffer writes into Rust before the call sees memory.
      if (mem) mem._syncIn();
      var res;
      try { res = __lumen_wasm_call(instId, funcIdx, args); }
      catch (e) { if (mem) mem._syncOut(); throw new RuntimeError(errMsg(e)); }
      // Reflect WASM's writes back into the same buffer the HEAP views alias.
      if (mem) mem._syncOut();
      if (!res || res.length === 0) return undefined;
      if (res.length === 1) return res[0];
      return res;
    };
  }

  function makeExportMemory(instId) {
    var mem = Object.create(Memory.prototype);
    mem._instId = instId;
    // Stable canonical buffer: built once and reused so a captured view such as
    // `HEAP32 = new Int32Array(memory.buffer)` remains valid across calls. It is
    // refreshed in place by `_syncOut`, and only replaced (detached) on growth.
    mem._buf = __lumen_wasm_mem_buffer(instId);
    mem._pages = __lumen_wasm_mem_size(instId);

    // JS buffer -> Rust linear memory (run before each exported call).
    mem._syncIn = function() {
      __lumen_wasm_mem_write(instId, 0, new Uint8Array(mem._buf));
    };
    // Rust linear memory -> JS buffer in place (run after each exported call).
    // If WASM grew memory mid-call, allocate a fresh, larger buffer (matching
    // the spec's detach-on-grow — callers re-acquire their HEAP views).
    mem._syncOut = function() {
      var pages = __lumen_wasm_mem_size(instId);
      if (pages !== mem._pages) {
        mem._pages = pages;
        mem._buf = __lumen_wasm_mem_buffer(instId);
        return;
      }
      new Uint8Array(mem._buf).set(new Uint8Array(__lumen_wasm_mem_buffer(instId)));
    };

    Object.defineProperty(mem, 'buffer', {
      get: function() { return mem._buf; },
      configurable: true
    });
    mem.grow = function(d) {
      var prev = __lumen_wasm_mem_grow(instId, d | 0);
      if (prev >= 0) {
        mem._pages = __lumen_wasm_mem_size(instId);
        mem._buf = __lumen_wasm_mem_buffer(instId); // fresh, larger (detach)
      }
      return prev;
    };
    // MVP escape hatch for direct memory I/O against Rust-owned linear memory.
    // These bypass the call-boundary sync, so prefer `buffer`/HEAP views.
    mem.read = function(offset, len) { return new Uint8Array(__lumen_wasm_mem_read(instId, offset | 0, len | 0)); };
    mem.write = function(offset, bytes) { return __lumen_wasm_mem_write(instId, offset | 0, bytes); };
    return mem;
  }

  function makeExportGlobal(instId, gidx) {
    var g = Object.create(Global.prototype);
    g._instId = instId;
    Object.defineProperty(g, 'value', {
      get: function() { return __lumen_wasm_global_get(instId, gidx); },
      // Pass `v` through untouched so an i64 global accepts a BigInt without
      // `+v` throwing / truncating.
      set: function(v) { __lumen_wasm_global_set(instId, gidx, v); },
      configurable: true
    });
    g.valueOf = function() { return __lumen_wasm_global_get(instId, gidx); };
    return g;
  }

  function buildExports(instId, exportsDesc) {
    var exports = Object.create(null);
    // Build the exported memory first so function wrappers can reference it for
    // call-boundary synchronization (MVP exposes at most one memory).
    var memRef = { mem: null };
    for (var i = 0; i < exportsDesc.length; i++) {
      var em = exportsDesc[i];
      if (em.kind === 'memory') {
        var m = makeExportMemory(instId);
        exports[em.name] = m;
        memRef.mem = m;
      }
    }
    for (var j = 0; j < exportsDesc.length; j++) {
      var e = exportsDesc[j];
      if (e.kind === 'function') exports[e.name] = makeExportFn(instId, e.index, memRef);
      else if (e.kind === 'global') exports[e.name] = makeExportGlobal(instId, e.index);
      else if (e.kind !== 'memory') exports[e.name] = null; // table export — MVP stub
    }
    return exports;
  }

  // ── WebAssembly.Instance ──────────────────────────────────────────────────
  class Instance {
    constructor(module, importObject) {
      if (!(module instanceof Module)) {
        throw new LinkError('Instance requires a WebAssembly.Module');
      }
      var imports = JSON.parse(__lumen_wasm_module_imports(module._id));
      var funcs = [], globals = [];
      for (var i = 0; i < imports.length; i++) {
        var im = imports[i];
        var modObj = importObject && importObject[im.module];
        var val = modObj ? modObj[im.name] : undefined;
        if (im.kind === 'function') {
          if (typeof val !== 'function') {
            throw new LinkError('Import "' + im.module + '.' + im.name + '" is not a function');
          }
          funcs.push(val);
        } else if (im.kind === 'global') {
          var gv = (val && typeof val === 'object' && ('value' in val)) ? val.value : val;
          globals.push(typeof gv === 'number' ? gv : 0);
        }
        // imported memory/table: MVP synthesizes internal ones from declared limits
      }
      var instId;
      try { instId = __lumen_wasm_instantiate(module._id, funcs, globals); }
      catch (e) { throw new LinkError(errMsg(e)); }
      this._instId = instId;
      var desc = JSON.parse(__lumen_wasm_module_exports(module._id));
      this.exports = buildExports(instId, desc);
    }
  }

  // ── Top-level API ──────────────────────────────────────────────────────────

  function validate(bufferSource) {
    if (!bufferSource) return false;
    try { return __lumen_wasm_validate(bytesOf(bufferSource)); }
    catch (e) { return false; }
  }

  function compile(bufferSource) {
    try { return Promise.resolve(new Module(bufferSource)); }
    catch (e) { return Promise.reject(e); }
  }

  function instantiate(source, importObject) {
    if (source instanceof Module) {
      try { return Promise.resolve(new Instance(source, importObject)); }
      catch (e) { return Promise.reject(e); }
    }
    return compile(source).then(function(mod) {
      var inst = new Instance(mod, importObject);
      return { module: mod, instance: inst };
    });
  }

  function compileStreaming(source) {
    return Promise.resolve(source).then(function(resp) {
      if (resp && typeof resp.arrayBuffer === 'function') {
        return resp.arrayBuffer().then(function(buf) { return compile(buf); });
      }
      return compile(resp);
    });
  }

  function instantiateStreaming(source, importObject) {
    return Promise.resolve(source).then(function(resp) {
      if (resp && typeof resp.arrayBuffer === 'function') {
        return resp.arrayBuffer().then(function(buf) { return instantiate(buf, importObject); });
      }
      return instantiate(resp, importObject);
    });
  }

  // ── Publish global WebAssembly object ─────────────────────────────────────
  var WebAssembly = {
    Module:               Module,
    Instance:             Instance,
    Memory:               Memory,
    Table:                Table,
    Global:               Global,
    Tag:                  Tag,
    Exception:            WasmException,
    CompileError:         CompileError,
    LinkError:            LinkError,
    RuntimeError:         RuntimeError,
    compile:              compile,
    instantiate:          instantiate,
    compileStreaming:     compileStreaming,
    instantiateStreaming: instantiateStreaming,
    validate:             validate,
  };

  Object.defineProperty(globalThis, 'WebAssembly', {
    value: WebAssembly,
    writable: false,
    enumerable: false,
    configurable: true,
  });
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

    fn with_wasm(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webassembly_bindings(&ctx).unwrap();
            f(&ctx);
            // Release any `Persistent` import handles before the Runtime drops,
            // or QuickJS asserts on a non-empty GC object list (BUG-222).
            wasm::clear_registry();
        });
    }

    #[test]
    fn webassembly_global_exists() {
        with_wasm(|ctx| {
            let ok: bool = ctx.eval("typeof WebAssembly === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_validate_magic_bytes() {
        with_wasm(|ctx| {
            let valid: bool = ctx
                .eval(
                    "var b = new Uint8Array([0x00,0x61,0x73,0x6D,0x01,0x00,0x00,0x00]).buffer;\
                     WebAssembly.validate(b)",
                )
                .unwrap();
            assert!(valid);
            let invalid: bool = ctx
                .eval("WebAssembly.validate(new Uint8Array([0xFF,0x61,0x73,0x6D,1,0,0,0]).buffer)")
                .unwrap();
            assert!(!invalid);
        });
    }

    /// `(module (func (export "add") (param i32 i32) (result i32)
    ///   local.get 0 local.get 1 i32.add))` — hand-assembled bytes.
    const ADD_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
        0x01, 0x07, 0x01, 0x60, 0x02, 0x7F, 0x7F, 0x01, 0x7F, // type (i32,i32)->i32
        0x03, 0x02, 0x01, 0x00, // one func of type 0
        0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, // export "add" func 0
        0x0A, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6A, 0x0B, // code
    ];

    #[test]
    fn webassembly_instantiate_and_call_add() {
        with_wasm(|ctx| {
            ctx.globals().set("__add_bytes", ADD_WASM.to_vec()).unwrap();
            let sum: i32 = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__add_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     inst.exports.add(40, 2)",
                )
                .unwrap();
            assert_eq!(sum, 42);
        });
    }

    #[test]
    fn webassembly_module_exports_lists_add() {
        with_wasm(|ctx| {
            ctx.globals().set("__add_bytes", ADD_WASM.to_vec()).unwrap();
            let name: String = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__add_bytes));\
                     WebAssembly.Module.exports(m)[0].name",
                )
                .unwrap();
            assert_eq!(name, "add");
        });
    }

    #[test]
    fn webassembly_global_mutable_direct() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval(
                    "var g = new WebAssembly.Global({value:'i32', mutable:true}, 5);\
                     g.value = 9; g.value === 9",
                )
                .unwrap();
            assert!(ok);
        });
    }

    /// `(module (func (export "add") (param i64 i64) (result i64)
    ///   local.get 0 local.get 1 i64.add))` — hand-assembled.
    const ADD64_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
        0x01, 0x07, 0x01, 0x60, 0x02, 0x7E, 0x7E, 0x01, 0x7E, // type (i64,i64)->i64
        0x03, 0x02, 0x01, 0x00, // one func of type 0
        0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, // export "add" func 0
        0x0A, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x7C, 0x0B, // code: local0 local1 i64.add
    ];

    #[test]
    fn webassembly_i64_export_uses_bigint_full_precision() {
        with_wasm(|ctx| {
            ctx.globals().set("__add64_bytes", ADD64_WASM.to_vec()).unwrap();
            // 2^53 + 1 is the first integer an f64 cannot represent. A correct
            // BigInt boundary keeps it exact; the old f64 path would round it.
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__add64_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var r = inst.exports.add(9007199254740993n, 2n);\
                     (typeof r === 'bigint') && (r === 9007199254740995n)",
                )
                .unwrap();
            assert!(ok, "i64 export must round-trip as exact BigInt");
        });
    }

    /// `(module (global (export "g") (mut i64) (i64.const 0)))`.
    const GLOBAL64_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
        0x06, 0x06, 0x01, 0x7E, 0x01, 0x42, 0x00, 0x0B, // global: mut i64 = 0
        0x07, 0x05, 0x01, 0x01, 0x67, 0x03, 0x00, // export "g" global 0
    ];

    #[test]
    fn webassembly_i64_global_roundtrips_as_bigint() {
        with_wasm(|ctx| {
            ctx.globals().set("__g64_bytes", GLOBAL64_WASM.to_vec()).unwrap();
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__g64_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     inst.exports.g.value = 9007199254740993n;\
                     var v = inst.exports.g.value;\
                     (typeof v === 'bigint') && (v === 9007199254740993n)",
                )
                .unwrap();
            assert!(ok, "i64 global get/set must preserve exact BigInt");
        });
    }

    /// `(module (import "env" "h" (func (param i64) (result i64)))
    ///   (func (export "f") (param i64) (result i64) local.get 0 call 0))`.
    const IMPORT64_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
        0x01, 0x06, 0x01, 0x60, 0x01, 0x7E, 0x01, 0x7E, // type (i64)->i64
        0x02, 0x09, 0x01, 0x03, 0x65, 0x6E, 0x76, 0x01, 0x68, 0x00, 0x00, // import env.h func type0
        0x03, 0x02, 0x01, 0x00, // defined func 1, type 0
        0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x01, // export "f" func 1
        0x0A, 0x08, 0x01, 0x06, 0x00, 0x20, 0x00, 0x10, 0x00, 0x0B, // code: local0 call0
    ];

    #[test]
    fn webassembly_i64_import_arg_and_result_use_bigint() {
        with_wasm(|ctx| {
            ctx.globals().set("__imp64_bytes", IMPORT64_WASM.to_vec()).unwrap();
            // The host import sees the i64 argument as a BigInt and returns a
            // BigInt; both legs must keep full 64-bit precision.
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__imp64_bytes));\
                     var seen;\
                     var inst = new WebAssembly.Instance(m, {env:{h:function(x){ seen = x; return x + 1n; }}});\
                     var r = inst.exports.f(9007199254740993n);\
                     (typeof seen === 'bigint') && (seen === 9007199254740993n) &&\
                     (typeof r === 'bigint') && (r === 9007199254740994n)",
                )
                .unwrap();
            assert!(ok, "i64 import arg + result must round-trip as exact BigInt");
        });
    }

    /// `(module (memory (export "memory") 1)
    ///   (func (export "store") (param i32 i32) local.get 0 local.get 1 i32.store)
    ///   (func (export "load") (param i32) (result i32) local.get 0 i32.load))`
    /// — hand-assembled. `store(off,val)` writes a 32-bit word; `load(off)`
    /// reads one. Lets a test observe coherence between WASM memory and the JS
    /// `memory.buffer` HEAP view in both directions.
    const MEM_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // header
        // type: (i32,i32)->() , (i32)->(i32)
        0x01, 0x0B, 0x02, 0x60, 0x02, 0x7F, 0x7F, 0x00, 0x60, 0x01, 0x7F, 0x01, 0x7F,
        0x03, 0x03, 0x02, 0x00, 0x01, // funcs: type 0, type 1
        0x05, 0x03, 0x01, 0x00, 0x01, // memory: min 1 page
        // exports: "memory" mem0, "store" func0, "load" func1
        0x07, 0x19, 0x03, 0x06, 0x6D, 0x65, 0x6D, 0x6F, 0x72, 0x79, 0x02, 0x00, 0x05, 0x73,
        0x74, 0x6F, 0x72, 0x65, 0x00, 0x00, 0x04, 0x6C, 0x6F, 0x61, 0x64, 0x00, 0x01,
        // code: store = local0 local1 i32.store ; load = local0 i32.load
        0x0A, 0x13, 0x02, 0x09, 0x00, 0x20, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, 0x0B, 0x07,
        0x00, 0x20, 0x00, 0x28, 0x02, 0x00, 0x0B,
    ];

    #[test]
    fn wasm_writes_visible_through_stable_buffer_view() {
        with_wasm(|ctx| {
            ctx.globals().set("__mem_bytes", MEM_WASM.to_vec()).unwrap();
            // Capture an Int32Array view BEFORE the call; a coherent live buffer
            // must show the WASM write through that same (stable) view.
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__mem_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var view = new Int32Array(inst.exports.memory.buffer);\
                     inst.exports.store(0, 1234);\
                     view[0] === 1234",
                )
                .unwrap();
            assert!(ok, "WASM memory write must reach a pre-captured HEAP view");
        });
    }

    #[test]
    fn js_buffer_writes_visible_to_wasm() {
        with_wasm(|ctx| {
            ctx.globals().set("__mem_bytes", MEM_WASM.to_vec()).unwrap();
            // A JS write through the HEAP view must be synced into Rust memory
            // before the next exported call reads it.
            let val: i32 = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__mem_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var view = new Int32Array(inst.exports.memory.buffer);\
                     view[4] = 5678;\
                     inst.exports.load(16)",
                )
                .unwrap();
            assert_eq!(val, 5678, "JS HEAP write must be visible to a later WASM load");
        });
    }

    #[test]
    fn buffer_identity_is_stable_across_calls() {
        with_wasm(|ctx| {
            ctx.globals().set("__mem_bytes", MEM_WASM.to_vec()).unwrap();
            let same: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__mem_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var b1 = inst.exports.memory.buffer;\
                     inst.exports.store(8, 99);\
                     var b2 = inst.exports.memory.buffer;\
                     b1 === b2",
                )
                .unwrap();
            assert!(same, "buffer identity must persist across a non-growing call");
        });
    }

    #[test]
    fn js_grow_resizes_buffer() {
        with_wasm(|ctx| {
            ctx.globals().set("__mem_bytes", MEM_WASM.to_vec()).unwrap();
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__mem_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var prev = inst.exports.memory.grow(1);\
                     (prev === 1) && (inst.exports.memory.buffer.byteLength === 2 * 65536)",
                )
                .unwrap();
            assert!(ok, "JS Memory.grow must enlarge the exported buffer");
        });
    }

    #[test]
    fn round_trip_through_heap_and_back() {
        with_wasm(|ctx| {
            ctx.globals().set("__mem_bytes", MEM_WASM.to_vec()).unwrap();
            // WASM stores, JS reads it via the view, mutates a neighbouring word,
            // and WASM reads that back — full bidirectional coherence in one go.
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Module(new Uint8Array(__mem_bytes));\
                     var inst = new WebAssembly.Instance(m);\
                     var view = new Int32Array(inst.exports.memory.buffer);\
                     inst.exports.store(0, 11);\
                     var a = view[0];\
                     view[1] = 22;\
                     var b = inst.exports.load(4);\
                     (a === 11) && (b === 22)",
                )
                .unwrap();
            assert!(ok, "memory must stay coherent across mixed WASM/JS access");
        });
    }
}
