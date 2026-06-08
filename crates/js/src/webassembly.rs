/// WebAssembly Phase 0 stub (W3C WebAssembly JavaScript Interface §7).
///
/// Phase 0: API surface is complete — compile/instantiate return resolved Promises
/// with empty Module/Instance stubs. No actual WASM bytecode execution.
/// Phase 1 (future): integrate `wasmtime` or `wasmer` for real execution.
use rquickjs::Ctx;

/// Install WebAssembly API bindings into the JS context.
pub fn install_webassembly_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
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

  // ── WebAssembly.Module ─────────────────────────────────────────────────────
  // Phase 0: compiled module stub; stores no actual bytecode.
  class Module {
    constructor(bufferSource) {
      if (bufferSource === undefined || bufferSource === null) {
        throw new CompileError('Module requires a BufferSource');
      }
      this._byteLength = (bufferSource.byteLength !== undefined)
        ? bufferSource.byteLength
        : (bufferSource.length || 0);
    }
  }

  // Static methods — return empty arrays (Phase 0: no section parsing).
  Module.exports = function(module) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    return [];
  };
  Module.imports = function(module) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    return [];
  };
  Module.customSections = function(module, sectionName) {
    if (!(module instanceof Module)) throw new TypeError('Argument must be a WebAssembly.Module');
    return [];
  };

  // ── WebAssembly.Instance ──────────────────────────────────────────────────
  // Phase 0: exports are always empty — no actual WASM function execution.
  class Instance {
    constructor(module, importObject) {
      if (!(module instanceof Module)) {
        throw new LinkError('Instance requires a WebAssembly.Module');
      }
      // Phase 0: empty exports — real execution requires wasmtime/wasmer.
      this.exports = Object.create(null);
    }
  }

  // ── WebAssembly.Memory ────────────────────────────────────────────────────
  // Resizable ArrayBuffer with page-based addressing (1 page = 64 KiB).
  class Memory {
    constructor(descriptor) {
      if (!descriptor || typeof descriptor !== 'object') {
        throw new TypeError('Memory descriptor must be an object');
      }
      var initial = descriptor.initial | 0;
      if (initial < 0) throw new RangeError('Memory initial must be ≥ 0');
      var maximum = (descriptor.maximum !== undefined) ? (descriptor.maximum | 0) : 65536;
      this._pages = initial;
      this._max = maximum;
      this._buffer = new ArrayBuffer(initial * 65536);
    }

    get buffer() { return this._buffer; }

    // Returns previous page count, or -1 if growth fails.
    grow(delta) {
      var d = delta | 0;
      if (d < 0) throw new RangeError('grow delta must be ≥ 0');
      var prev = this._pages;
      var next = prev + d;
      if (next > this._max) return -1;
      this._pages = next;
      this._buffer = new ArrayBuffer(next * 65536);
      return prev;
    }
  }

  // ── WebAssembly.Table ─────────────────────────────────────────────────────
  // Resizable typed reference table.
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
      if (index < 0 || index >= this._entries.length) {
        throw new RangeError('Table index out of bounds');
      }
      return this._entries[index];
    }

    set(index, value) {
      if (index < 0 || index >= this._entries.length) {
        throw new RangeError('Table index out of bounds');
      }
      this._entries[index] = (value === undefined) ? null : value;
    }

    // Returns previous length, or -1 if growth fails.
    grow(delta, initValue) {
      var d = delta | 0;
      if (d < 0) throw new RangeError('grow delta must be ≥ 0');
      var prev = this._entries.length;
      var next = prev + d;
      if (next > this._max) return -1;
      var fill = (initValue === undefined) ? null : initValue;
      for (var i = 0; i < d; i++) this._entries.push(fill);
      return prev;
    }
  }

  // ── WebAssembly.Global ────────────────────────────────────────────────────
  // A boxed WASM global value (mutable or immutable).
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

    // Returns the internal value (spec §11.3).
    valueOf() { return this._value; }
  }

  // ── WebAssembly.Tag (Exceptions proposal) ─────────────────────────────────
  // Stub: tags are identified by reference equality, no type checking in Phase 0.
  class Tag {
    constructor(type) {
      this._type = type || { parameters: [] };
    }
  }

  class Exception {
    constructor(tag, payload) {
      this._tag = tag;
      this._payload = payload || [];
    }
    getArg(tag, index) {
      if (tag !== this._tag) throw new TypeError('Wrong tag');
      return this._payload[index];
    }
    is(tag) { return this._tag === tag; }
  }

  // ── Top-level API ──────────────────────────────────────────────────────────

  // validate() — Phase 0: accepts any non-empty ArrayBuffer.
  // Real validation checks the WASM magic header (0x00 0x61 0x73 0x6D).
  function validate(bufferSource) {
    if (!bufferSource) return false;
    var bytes = bufferSource instanceof ArrayBuffer
      ? new Uint8Array(bufferSource)
      : (bufferSource.buffer ? new Uint8Array(bufferSource.buffer) : null);
    if (!bytes || bytes.length < 8) return false;
    // Check WASM magic: \0asm
    return bytes[0] === 0x00 && bytes[1] === 0x61 && bytes[2] === 0x73 && bytes[3] === 0x6D;
  }

  // compile() → Promise<Module>
  function compile(bufferSource) {
    try {
      var mod = new Module(bufferSource);
      return Promise.resolve(mod);
    } catch (e) {
      return Promise.reject(e);
    }
  }

  // instantiate(bufferSource | Module, importObject?) → Promise<{module, instance}> | Promise<Instance>
  function instantiate(source, importObject) {
    if (source instanceof Module) {
      // instantiate(module) → Promise<Instance>
      try {
        var inst = new Instance(source, importObject);
        return Promise.resolve(inst);
      } catch (e) {
        return Promise.reject(e);
      }
    }
    // instantiate(bufferSource) → Promise<{module, instance}>
    return compile(source).then(function(mod) {
      var inst = new Instance(mod, importObject);
      return { module: mod, instance: inst };
    });
  }

  // compileStreaming() — accepts Response or Promise<Response>, reads body as ArrayBuffer.
  function compileStreaming(source) {
    return Promise.resolve(source).then(function(resp) {
      if (resp && typeof resp.arrayBuffer === 'function') {
        return resp.arrayBuffer().then(function(buf) { return compile(buf); });
      }
      return compile(resp);
    });
  }

  // instantiateStreaming() — accepts Response or Promise<Response>.
  function instantiateStreaming(source, importObject) {
    return compileStreaming(source).then(function(mod) {
      return instantiate(mod, importObject);
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
    Exception:            Exception,
    CompileError:         CompileError,
    LinkError:            LinkError,
    RuntimeError:         RuntimeError,
    compile:              compile,
    instantiate:          instantiate,
    compileStreaming:      compileStreaming,
    instantiateStreaming:  instantiateStreaming,
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
    fn webassembly_compile_returns_promise() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval("WebAssembly.compile(new ArrayBuffer(8)) instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_instantiate_returns_promise() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval("WebAssembly.instantiate(new ArrayBuffer(8)) instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_validate_magic_bytes() {
        with_wasm(|ctx| {
            // valid WASM magic: \0asm + version 1
            let valid: bool = ctx
                .eval(
                    "var b = new Uint8Array([0x00,0x61,0x73,0x6D,0x01,0x00,0x00,0x00]).buffer;\
                     WebAssembly.validate(b)",
                )
                .unwrap();
            assert!(valid);
            // wrong magic
            let invalid: bool = ctx
                .eval(
                    "WebAssembly.validate(new Uint8Array([0xFF,0x61,0x73,0x6D,1,0,0,0]).buffer)",
                )
                .unwrap();
            assert!(!invalid);
        });
    }

    #[test]
    fn webassembly_memory_construction() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval(
                    "var m = new WebAssembly.Memory({initial:1});\
                     m.buffer instanceof ArrayBuffer && m.buffer.byteLength === 65536",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_memory_grow() {
        with_wasm(|ctx| {
            let prev: i32 = ctx
                .eval(
                    "var m = new WebAssembly.Memory({initial:1, maximum:3});\
                     m.grow(1)",
                )
                .unwrap();
            assert_eq!(prev, 1);
        });
    }

    #[test]
    fn webassembly_table_get_set() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval(
                    "var t = new WebAssembly.Table({element:'funcref', initial:2});\
                     t.set(0, null);\
                     t.get(0) === null && t.length === 2",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_global_mutable() {
        with_wasm(|ctx| {
            let v: f64 = ctx
                .eval(
                    "var g = new WebAssembly.Global({value:'f32', mutable:true}, 42);\
                     g.value = 7;\
                     g.value",
                )
                .unwrap();
            assert!((v - 7.0).abs() < f64::EPSILON);
        });
    }

    #[test]
    fn webassembly_error_classes_exist() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof WebAssembly.CompileError === 'function' &&\
                     typeof WebAssembly.LinkError === 'function' &&\
                     typeof WebAssembly.RuntimeError === 'function'",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webassembly_module_static_methods() {
        with_wasm(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof WebAssembly.Module.exports === 'function' &&\
                     typeof WebAssembly.Module.imports === 'function' &&\
                     typeof WebAssembly.Module.customSections === 'function'",
                )
                .unwrap();
            assert!(ok);
        });
    }
}
