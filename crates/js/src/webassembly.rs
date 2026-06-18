//! WebAssembly JavaScript Interface (W3C §7), backed by Lumen's MVP interpreter.
//!
//! Stage 1 of U-4: `WebAssembly.compile`/`validate`/`instantiate` now decode and
//! **execute** real bytecode through [`crate::wasm`]. `Instance.exports` contains
//! callable functions, exported memory/globals, instead of the previous empty
//! Phase 0 stubs. `Memory`/`Table`/`Global`/`Tag`/`Exception` standalone classes
//! are unchanged (used when constructed directly by JS).
//!
//! MVP boundaries (documented): numeric values cross the JS↔WASM boundary as
//! `f64` (so `i64` beyond 2^53 loses precision); exported `Memory.buffer` is a
//! *snapshot copy* of Rust-owned linear memory (the live-aliasing
//! `new Int32Array(memory.buffer)` emscripten pattern is not coherent); host
//! imports cannot read/write the instance's memory mid-call.

use rquickjs::{Ctx, Exception, Function, Persistent, TypedArray};

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

    g.set(
        "__lumen_wasm_call",
        Function::new(
            ctx.clone(),
            |ctx: Ctx, inst_id: u32, func_idx: u32, args: Vec<f64>| -> rquickjs::Result<Vec<f64>> {
                wasm::call_f64(&ctx, inst_id, func_idx, args)
                    .map_err(|e| Exception::throw_message(&ctx, &e))
            },
        )?,
    )?;

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
        "__lumen_wasm_global_get",
        Function::new(ctx.clone(), |inst_id: u32, idx: u32| -> f64 { wasm::global_get(inst_id, idx) })?,
    )?;
    g.set(
        "__lumen_wasm_global_set",
        Function::new(ctx.clone(), |inst_id: u32, idx: u32, v: f64| -> bool {
            wasm::global_set(inst_id, idx, v)
        })?,
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

  function makeExportFn(instId, funcIdx) {
    return function() {
      var args = new Array(arguments.length);
      for (var i = 0; i < arguments.length; i++) args[i] = +arguments[i];
      var res;
      try { res = __lumen_wasm_call(instId, funcIdx, args); }
      catch (e) { throw new RuntimeError(errMsg(e)); }
      if (!res || res.length === 0) return undefined;
      if (res.length === 1) return res[0];
      return res;
    };
  }

  function makeExportMemory(instId) {
    var mem = Object.create(Memory.prototype);
    mem._instId = instId;
    Object.defineProperty(mem, 'buffer', {
      get: function() {
        var pages = __lumen_wasm_mem_size(instId);
        var raw = __lumen_wasm_mem_read(instId, 0, pages * 65536);
        return new Uint8Array(raw).buffer;
      },
      configurable: true
    });
    mem.grow = function(d) { return __lumen_wasm_mem_grow(instId, d | 0); };
    // MVP escape hatch for memory I/O against Rust-owned linear memory.
    mem.read = function(offset, len) { return new Uint8Array(__lumen_wasm_mem_read(instId, offset | 0, len | 0)); };
    mem.write = function(offset, bytes) { return __lumen_wasm_mem_write(instId, offset | 0, bytes); };
    return mem;
  }

  function makeExportGlobal(instId, gidx) {
    var g = Object.create(Global.prototype);
    g._instId = instId;
    Object.defineProperty(g, 'value', {
      get: function() { return __lumen_wasm_global_get(instId, gidx); },
      set: function(v) { __lumen_wasm_global_set(instId, gidx, +v); },
      configurable: true
    });
    g.valueOf = function() { return __lumen_wasm_global_get(instId, gidx); };
    return g;
  }

  function buildExports(instId, exportsDesc) {
    var exports = Object.create(null);
    for (var i = 0; i < exportsDesc.length; i++) {
      var e = exportsDesc[i];
      if (e.kind === 'function') exports[e.name] = makeExportFn(instId, e.index);
      else if (e.kind === 'memory') exports[e.name] = makeExportMemory(instId);
      else if (e.kind === 'global') exports[e.name] = makeExportGlobal(instId, e.index);
      else exports[e.name] = null; // table export — MVP stub
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
}
