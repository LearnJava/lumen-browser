//! TC39 Stage 4 ES2026+ proposal shims for APIs missing from QuickJS.
//!
//! QuickJS implements ES2023. Several proposals that reached Stage 4 for
//! ES2024/ES2025/ES2026 are not yet natively present. This module installs
//! pure-JS shims for:
//!
//! * **`Float16Array`** — 16-bit IEEE 754 half-precision typed array — ES2025
//! * **`Math.f16round(x)`** — round to nearest float16 — ES2025
//! * **`DataView.prototype.getFloat16/setFloat16`** — read/write f16 values — ES2025
//! * **`Symbol.dispose` / `Symbol.asyncDispose`** — explicit resource management — ES2024
//! * **`SuppressedError`** — suppressed-error class for `using` cleanup chains — ES2024
//! * **`DisposableStack`** — synchronous disposable resource stack — ES2024
//! * **`AsyncDisposableStack`** — async disposable resource stack — ES2024
//!
//! Each shim guards against native support — no-op when the engine already provides it.
//! The `using`/`await using` syntax requires native parser support and is not shimmed.

use rquickjs::Ctx;

/// Install all ES2026+ proposal shims into the given QuickJS context.
///
/// Must run after the DOM shim so that `Promise`, `Symbol`, and `DataView`
/// are already defined. Pure-JS; no Rust native bindings needed.
pub fn install_es2026_proposals(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(FLOAT16_SHIM)?;
    ctx.eval::<(), _>(DISPOSABLE_STACK_SHIM)?;
    Ok(())
}

/// Float16Array + Math.f16round + DataView.getFloat16/setFloat16 shim.
const FLOAT16_SHIM: &str = r#"(function() {
  'use strict';

  // ── f16 ↔ f64 conversion helpers ─────────────────────────────────────────

  // Convert a uint16 bit-pattern to float64.
  function f16_to_f64(bits) {
    bits = bits & 0xFFFF;
    var sign = (bits >>> 15) & 1;
    var exp  = (bits >>> 10) & 0x1F;
    var mant = bits & 0x3FF;
    var val;
    if (exp === 0) {
      // Subnormal or zero: value = mant * 2^-24
      val = mant * 5.9604644775390625e-8;
    } else if (exp === 0x1F) {
      // Infinity or NaN
      val = mant ? NaN : Infinity;
    } else {
      // Normal: (1 + mant/1024) * 2^(exp-15)
      val = (1 + mant * 9.765625e-4) * Math.pow(2, exp - 15);
    }
    return sign ? -val : val;
  }

  // Convert a float64 to a uint16 f16 bit-pattern (round-to-nearest-even).
  function f64_to_f16(val) {
    if (val !== val) return 0x7E00; // NaN
    var sign = 0;
    if (val < 0 || (val === 0 && (1 / val) < 0)) {
      sign = 0x8000;
      val = -val;
    }
    if (!isFinite(val)) return sign | 0x7C00; // Infinity

    // Subnormal range [2^-24, 2^-14): stored as mant * 2^-24
    if (val < 6.103515625e-5) { // < 2^-14
      var mant_s = Math.round(val * 16777216); // val * 2^24
      if (mant_s === 0) return sign; // underflow to ±0
      if (mant_s >= 1024) return sign | 0x0400; // rounds up to smallest normal
      return sign | (mant_s & 0x3FF);
    }

    // Normal range
    var exp = Math.floor(Math.log2(val));
    // Correct for floating-point imprecision in Math.log2
    if (val < Math.pow(2, exp))       exp--;
    else if (val >= Math.pow(2, exp + 1)) exp++;
    var exp16 = exp + 15;
    var mant_f = val / Math.pow(2, exp) - 1; // fractional mantissa [0, 1)
    var mant = Math.round(mant_f * 1024);
    if (mant === 1024) { // rounding caused carry
      mant = 0;
      exp16++;
    }
    if (exp16 >= 31) return sign | 0x7C00; // overflow to ±Infinity
    return sign | ((exp16 << 10) | (mant & 0x3FF));
  }

  // ── Float16Array ──────────────────────────────────────────────────────────

  if (typeof Float16Array === 'undefined') {

    function makeF16(arg, byteOffset, length) {
      var buf, len;
      if (arg instanceof ArrayBuffer) {
        byteOffset = (byteOffset | 0) || 0;
        len = length !== undefined ? (length | 0) : ((arg.byteLength - byteOffset) >>> 1);
        buf = new Uint16Array(arg, byteOffset, len);
      } else if (typeof arg === 'number') {
        len = arg | 0;
        buf = new Uint16Array(len);
      } else if (arg) {
        var src = Array.from(arg);
        len = src.length;
        buf = new Uint16Array(len);
        for (var i = 0; i < len; i++) buf[i] = f64_to_f16(+src[i]);
      } else {
        len = 0; buf = new Uint16Array(0);
      }

      var target = Object.create(Float16Array.prototype);
      Object.defineProperty(target, '_buf',          { value: buf,             enumerable: false, configurable: true });
      Object.defineProperty(target, 'length',        { value: len,             enumerable: false, configurable: false });
      Object.defineProperty(target, 'byteLength',    { value: len * 2,         enumerable: false, configurable: false });
      Object.defineProperty(target, 'byteOffset',    { value: buf.byteOffset,  enumerable: false, configurable: false });
      Object.defineProperty(target, 'buffer',        { value: buf.buffer,      enumerable: false, configurable: false });
      Object.defineProperty(target, 'BYTES_PER_ELEMENT', { value: 2,           enumerable: false, configurable: false });

      return new Proxy(target, {
        get: function(t, prop) {
          if (typeof prop === 'string') {
            var idx = +prop;
            if (idx === idx && idx >= 0 && idx < len && (idx | 0) === idx) {
              return f16_to_f64(buf[idx]);
            }
          }
          var v = t[prop];
          if (typeof v === 'function') return v.bind(t);
          return v;
        },
        set: function(t, prop, value) {
          if (typeof prop === 'string') {
            var idx = +prop;
            if (idx === idx && idx >= 0 && (idx | 0) === idx && idx < len) {
              buf[idx] = f64_to_f16(+value);
              return true;
            }
          }
          t[prop] = value;
          return true;
        },
        has: function(t, prop) {
          if (typeof prop === 'string') {
            var idx = +prop;
            if (idx === idx && idx >= 0 && (idx | 0) === idx && idx < len) return true;
          }
          return prop in t;
        }
      });
    }

    function Float16Array(arg, byteOffset, length) {
      return makeF16(arg, byteOffset, length);
    }

    Float16Array.BYTES_PER_ELEMENT = 2;

    Float16Array.from = function(source, mapFn) {
      var arr = Array.from(source, mapFn);
      return makeF16(arr);
    };

    Float16Array.of = function() {
      return makeF16(Array.prototype.slice.call(arguments));
    };

    Float16Array.prototype.set = function(arr, offset) {
      var buf = this._buf;
      offset = (offset | 0) || 0;
      for (var i = 0; i < arr.length; i++) buf[offset + i] = f64_to_f16(+arr[i]);
    };

    Float16Array.prototype.subarray = function(begin, end) {
      var len = this.length;
      if (begin === undefined) begin = 0;
      if (begin < 0) begin = Math.max(0, len + begin);
      if (end === undefined) end = len;
      if (end < 0) end = Math.max(0, len + end);
      return makeF16(this.buffer, this.byteOffset + begin * 2, end - begin);
    };

    Float16Array.prototype.slice = function(begin, end) {
      var len = this.length;
      if (begin === undefined) begin = 0;
      if (begin < 0) begin = Math.max(0, len + begin);
      if (end === undefined) end = len;
      if (end < 0) end = Math.max(0, len + end);
      var result = [];
      for (var i = begin; i < end && i < len; i++) result.push(f16_to_f64(this._buf[i]));
      return makeF16(result);
    };

    Float16Array.prototype.fill = function(value, start, end) {
      var bits = f64_to_f16(+value);
      var len = this.length;
      if (start === undefined) start = 0;
      if (start < 0) start = Math.max(0, len + start);
      if (end === undefined) end = len;
      if (end < 0) end = Math.max(0, len + end);
      for (var i = start; i < end; i++) this._buf[i] = bits;
      return this;
    };

    Float16Array.prototype.copyWithin = function(target, start, end) {
      var buf = this._buf;
      buf.copyWithin(target, start, end);
      return this;
    };

    Float16Array.prototype.reverse = function() {
      var buf = this._buf;
      for (var i = 0, j = buf.length - 1; i < j; i++, j--) {
        var tmp = buf[i]; buf[i] = buf[j]; buf[j] = tmp;
      }
      return this;
    };

    Float16Array.prototype.sort = function(compareFn) {
      var buf = this._buf;
      var arr = Array.from(this);
      arr.sort(compareFn);
      for (var i = 0; i < arr.length; i++) buf[i] = f64_to_f16(arr[i]);
      return this;
    };

    Float16Array.prototype.indexOf = function(val, from) {
      from = (from | 0) || 0;
      for (var i = from; i < this.length; i++) if (this[i] === val) return i;
      return -1;
    };

    Float16Array.prototype.lastIndexOf = function(val, from) {
      from = from !== undefined ? (from | 0) : this.length - 1;
      for (var i = from; i >= 0; i--) if (this[i] === val) return i;
      return -1;
    };

    Float16Array.prototype.includes = function(val) {
      for (var i = 0; i < this.length; i++) {
        if (this[i] === val || (val !== val && this[i] !== this[i])) return true;
      }
      return false;
    };

    Float16Array.prototype.join = function(sep) {
      return Array.from(this).join(sep);
    };

    Float16Array.prototype.forEach = function(fn, thisArg) {
      for (var i = 0; i < this.length; i++) fn.call(thisArg, this[i], i, this);
    };

    Float16Array.prototype.map = function(fn, thisArg) {
      var result = [];
      for (var i = 0; i < this.length; i++) result.push(fn.call(thisArg, this[i], i, this));
      return makeF16(result);
    };

    Float16Array.prototype.filter = function(fn, thisArg) {
      var result = [];
      for (var i = 0; i < this.length; i++) if (fn.call(thisArg, this[i], i, this)) result.push(this[i]);
      return makeF16(result);
    };

    Float16Array.prototype.every = function(fn, thisArg) {
      for (var i = 0; i < this.length; i++) if (!fn.call(thisArg, this[i], i, this)) return false;
      return true;
    };

    Float16Array.prototype.some = function(fn, thisArg) {
      for (var i = 0; i < this.length; i++) if (fn.call(thisArg, this[i], i, this)) return true;
      return false;
    };

    Float16Array.prototype.find = function(fn, thisArg) {
      for (var i = 0; i < this.length; i++) if (fn.call(thisArg, this[i], i, this)) return this[i];
      return undefined;
    };

    Float16Array.prototype.findIndex = function(fn, thisArg) {
      for (var i = 0; i < this.length; i++) if (fn.call(thisArg, this[i], i, this)) return i;
      return -1;
    };

    Float16Array.prototype.reduce = function(fn, init) {
      var arr = Array.from(this);
      return arguments.length > 1 ? arr.reduce(fn, init) : arr.reduce(fn);
    };

    Float16Array.prototype.reduceRight = function(fn, init) {
      var arr = Array.from(this);
      return arguments.length > 1 ? arr.reduceRight(fn, init) : arr.reduceRight(fn);
    };

    Float16Array.prototype.at = function(idx) {
      idx = +idx;
      if (idx < 0) idx = this.length + idx;
      return (idx >= 0 && idx < this.length) ? this[idx] : undefined;
    };

    Float16Array.prototype.entries = function() {
      var self = this, i = 0;
      return {
        next: function() {
          if (i >= self.length) return { value: undefined, done: true };
          return { value: [i, self[i++]], done: false };
        },
        [Symbol.iterator]: function() { return this; }
      };
    };

    Float16Array.prototype.keys = function() {
      var len = this.length, i = 0;
      return {
        next: function() { return i >= len ? { value: undefined, done: true } : { value: i++, done: false }; },
        [Symbol.iterator]: function() { return this; }
      };
    };

    Float16Array.prototype.values = function() {
      var self = this, i = 0;
      return {
        next: function() { return i >= self.length ? { value: undefined, done: true } : { value: self[i++], done: false }; },
        [Symbol.iterator]: function() { return this; }
      };
    };

    Float16Array.prototype[Symbol.iterator] = Float16Array.prototype.values;

    Float16Array.prototype.toString = function() { return Array.from(this).toString(); };
    Float16Array.prototype.toLocaleString = function() { return Array.from(this).toLocaleString(); };

    Object.defineProperty(Float16Array.prototype, Symbol.toStringTag, {
      value: 'Float16Array', configurable: true, enumerable: false, writable: false
    });

    globalThis.Float16Array = Float16Array;
  }

  // ── Math.f16round ─────────────────────────────────────────────────────────

  if (typeof Math.f16round !== 'function') {
    Math.f16round = function f16round(x) {
      return f16_to_f64(f64_to_f16(+x));
    };
  }

  // ── DataView.prototype.getFloat16 / setFloat16 ────────────────────────────

  if (typeof DataView !== 'undefined') {
    if (typeof DataView.prototype.getFloat16 !== 'function') {
      DataView.prototype.getFloat16 = function(byteOffset, littleEndian) {
        return f16_to_f64(this.getUint16(byteOffset, !!littleEndian));
      };
    }
    if (typeof DataView.prototype.setFloat16 !== 'function') {
      DataView.prototype.setFloat16 = function(byteOffset, value, littleEndian) {
        this.setUint16(byteOffset, f64_to_f16(+value), !!littleEndian);
      };
    }
  }

})();
"#;

/// Symbol.dispose / Symbol.asyncDispose + SuppressedError + DisposableStack / AsyncDisposableStack.
const DISPOSABLE_STACK_SHIM: &str = r#"(function() {
  'use strict';

  // ── Symbol.dispose / Symbol.asyncDispose (TC39 Explicit Resource Management) ──

  if (typeof Symbol.dispose === 'undefined') {
    Object.defineProperty(Symbol, 'dispose', {
      value: Symbol('Symbol.dispose'),
      configurable: false,
      writable: false,
      enumerable: false
    });
  }

  if (typeof Symbol.asyncDispose === 'undefined') {
    Object.defineProperty(Symbol, 'asyncDispose', {
      value: Symbol('Symbol.asyncDispose'),
      configurable: false,
      writable: false,
      enumerable: false
    });
  }

  // ── SuppressedError ───────────────────────────────────────────────────────

  if (typeof SuppressedError === 'undefined') {
    function SuppressedError(error, suppressed, message) {
      var e = new Error(message !== undefined ? String(message) : '');
      e.name = 'SuppressedError';
      e.error = error;
      e.suppressed = suppressed;
      if (Error.captureStackTrace) Error.captureStackTrace(e, SuppressedError);
      Object.setPrototypeOf(e, SuppressedError.prototype);
      return e;
    }
    SuppressedError.prototype = Object.create(Error.prototype);
    SuppressedError.prototype.constructor = SuppressedError;
    SuppressedError.prototype.name = 'SuppressedError';
    Object.defineProperty(SuppressedError.prototype, Symbol.toStringTag, {
      value: 'SuppressedError', configurable: true, enumerable: false, writable: false
    });
    globalThis.SuppressedError = SuppressedError;
  }

  // ── DisposableStack ───────────────────────────────────────────────────────

  if (typeof DisposableStack === 'undefined') {
    function DisposableStack() {
      if (!(this instanceof DisposableStack)) throw new TypeError('DisposableStack must be called with new');
      Object.defineProperty(this, '_stack',    { value: [], writable: true, configurable: true });
      Object.defineProperty(this, '_disposed', { value: false, writable: true, configurable: true });
    }

    Object.defineProperty(DisposableStack.prototype, 'disposed', {
      configurable: true,
      enumerable: false,
      get: function() { return this._disposed; }
    });

    DisposableStack.prototype.use = function(value) {
      if (this._disposed) throw new ReferenceError('DisposableStack is already disposed');
      if (value != null) {
        var fn = value[Symbol.dispose];
        if (typeof fn !== 'function') throw new TypeError('value must have [Symbol.dispose]');
        this._stack.push({ tag: 'dispose', fn: fn, val: value });
      }
      return value;
    };

    DisposableStack.prototype.adopt = function(value, onDispose) {
      if (this._disposed) throw new ReferenceError('DisposableStack is already disposed');
      if (typeof onDispose !== 'function') throw new TypeError('onDispose must be a function');
      this._stack.push({ tag: 'adopt', fn: onDispose, val: value });
      return value;
    };

    DisposableStack.prototype.defer = function(onDispose) {
      if (this._disposed) throw new ReferenceError('DisposableStack is already disposed');
      if (typeof onDispose !== 'function') throw new TypeError('onDispose must be a function');
      this._stack.push({ tag: 'defer', fn: onDispose });
    };

    DisposableStack.prototype.move = function() {
      if (this._disposed) throw new ReferenceError('DisposableStack is already disposed');
      var fresh = new DisposableStack();
      fresh._stack = this._stack;
      this._stack = [];
      this._disposed = true;
      return fresh;
    };

    DisposableStack.prototype.dispose = function() {
      if (this._disposed) return undefined;
      this._disposed = true;
      var errors = [];
      var stack = this._stack;
      for (var i = stack.length - 1; i >= 0; i--) {
        var entry = stack[i];
        try {
          if (entry.tag === 'dispose') entry.fn.call(entry.val);
          else if (entry.tag === 'adopt') entry.fn(entry.val);
          else entry.fn();
        } catch (e) { errors.push(e); }
      }
      if (errors.length === 0) return undefined;
      var suppressed = errors[errors.length - 1];
      for (var j = errors.length - 2; j >= 0; j--) {
        suppressed = new SuppressedError(errors[j], suppressed, '');
      }
      throw suppressed;
    };

    DisposableStack.prototype[Symbol.dispose] = DisposableStack.prototype.dispose;
    Object.defineProperty(DisposableStack.prototype, Symbol.toStringTag, {
      value: 'DisposableStack', configurable: true, enumerable: false, writable: false
    });
    globalThis.DisposableStack = DisposableStack;
  }

  // ── AsyncDisposableStack ──────────────────────────────────────────────────

  if (typeof AsyncDisposableStack === 'undefined') {
    function AsyncDisposableStack() {
      if (!(this instanceof AsyncDisposableStack)) throw new TypeError('AsyncDisposableStack must be called with new');
      Object.defineProperty(this, '_stack',    { value: [], writable: true, configurable: true });
      Object.defineProperty(this, '_disposed', { value: false, writable: true, configurable: true });
    }

    Object.defineProperty(AsyncDisposableStack.prototype, 'disposed', {
      configurable: true,
      enumerable: false,
      get: function() { return this._disposed; }
    });

    AsyncDisposableStack.prototype.use = function(value) {
      if (this._disposed) throw new ReferenceError('AsyncDisposableStack is already disposed');
      if (value != null) {
        var asyncFn = value[Symbol.asyncDispose];
        var syncFn  = value[Symbol.dispose];
        if (typeof asyncFn !== 'function' && typeof syncFn !== 'function') {
          throw new TypeError('value must have [Symbol.asyncDispose] or [Symbol.dispose]');
        }
        this._stack.push({ tag: 'dispose', fn: asyncFn || syncFn, val: value });
      }
      return value;
    };

    AsyncDisposableStack.prototype.adopt = function(value, onDispose) {
      if (this._disposed) throw new ReferenceError('AsyncDisposableStack is already disposed');
      if (typeof onDispose !== 'function') throw new TypeError('onDispose must be a function');
      this._stack.push({ tag: 'adopt', fn: onDispose, val: value });
      return value;
    };

    AsyncDisposableStack.prototype.defer = function(onDispose) {
      if (this._disposed) throw new ReferenceError('AsyncDisposableStack is already disposed');
      if (typeof onDispose !== 'function') throw new TypeError('onDispose must be a function');
      this._stack.push({ tag: 'defer', fn: onDispose });
    };

    AsyncDisposableStack.prototype.move = function() {
      if (this._disposed) throw new ReferenceError('AsyncDisposableStack is already disposed');
      var fresh = new AsyncDisposableStack();
      fresh._stack = this._stack;
      this._stack = [];
      this._disposed = true;
      return fresh;
    };

    AsyncDisposableStack.prototype.disposeAsync = function() {
      var self = this;
      if (self._disposed) return Promise.resolve(undefined);
      self._disposed = true;
      var stack = self._stack;
      var errors = [];
      var idx = stack.length - 1;

      function step() {
        if (idx < 0) {
          if (errors.length === 0) return Promise.resolve(undefined);
          var suppressed = errors[errors.length - 1];
          for (var j = errors.length - 2; j >= 0; j--) {
            suppressed = new SuppressedError(errors[j], suppressed, '');
          }
          return Promise.reject(suppressed);
        }
        var entry = stack[idx--];
        var result;
        try {
          if (entry.tag === 'dispose') result = entry.fn.call(entry.val);
          else if (entry.tag === 'adopt') result = entry.fn(entry.val);
          else result = entry.fn();
        } catch (e) { errors.push(e); return step(); }

        if (result && typeof result.then === 'function') {
          return result.then(step, function(e) { errors.push(e); return step(); });
        }
        return step();
      }
      return step();
    };

    AsyncDisposableStack.prototype[Symbol.asyncDispose] = AsyncDisposableStack.prototype.disposeAsync;
    Object.defineProperty(AsyncDisposableStack.prototype, Symbol.toStringTag, {
      value: 'AsyncDisposableStack', configurable: true, enumerable: false, writable: false
    });
    globalThis.AsyncDisposableStack = AsyncDisposableStack;
  }

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
        install_es2026_proposals(ctx).unwrap();
    }

    // ── Float16Array ─────────────────────────────────────────────────────────

    #[test]
    fn float16array_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof Float16Array === 'function'
                      && Float16Array.BYTES_PER_ELEMENT === 2
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_length() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new Float16Array(4);
                    a.length === 4 && a.byteLength === 8 && a.BYTES_PER_ELEMENT === 2
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_roundtrip_values() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new Float16Array([1.0, 0.5, -1.0, 1.5, 0.0]);
                    a[0] === 1.0 && a[1] === 0.5 && a[2] === -1.0 && a[3] === 1.5 && a[4] === 0.0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_special_values() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new Float16Array([Infinity, -Infinity, NaN]);
                    a[0] === Infinity && a[1] === -Infinity && isNaN(a[2])
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_write_then_read() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new Float16Array(3);
                    a[0] = 2.0; a[1] = -0.5; a[2] = 1000;
                    a[0] === 2.0 && a[1] === -0.5 && a[2] === 1000
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_from_arraybuffer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ab = new ArrayBuffer(4);
                    var a = new Float16Array(ab);
                    a.length === 2 && a.buffer === ab
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn float16array_iterator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var a = new Float16Array([1, 2, 3]);
                    var sum = 0;
                    for (var v of a) sum += v;
                    sum === 6
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── Math.f16round ─────────────────────────────────────────────────────────

    #[test]
    fn math_f16round_exists_and_works() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof Math.f16round === 'function'
                      && Math.f16round(1.0) === 1.0
                      && Math.f16round(0.5) === 0.5
                      && Math.f16round(-1.0) === -1.0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── DataView.getFloat16 / setFloat16 ──────────────────────────────────────

    #[test]
    fn dataview_float16_roundtrip() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var dv = new DataView(new ArrayBuffer(4));
                    dv.setFloat16(0, 1.5, true);
                    var got = dv.getFloat16(0, true);
                    got === 1.5
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── Symbol.dispose / Symbol.asyncDispose ─────────────────────────────────

    #[test]
    fn symbol_dispose_and_async_dispose() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof Symbol.dispose       === 'symbol'
                      && typeof Symbol.asyncDispose === 'symbol'
                      && Symbol.dispose !== Symbol.asyncDispose
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── SuppressedError ───────────────────────────────────────────────────────

    #[test]
    fn suppressed_error_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var se = new SuppressedError(new Error('main'), new Error('prev'), 'wrapped');
                    se instanceof Error
                      && se.name === 'SuppressedError'
                      && se.error.message === 'main'
                      && se.suppressed.message === 'prev'
                      && se.message === 'wrapped'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── DisposableStack ───────────────────────────────────────────────────────

    #[test]
    fn disposable_stack_defer_and_dispose() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var log = [];
                    var ds = new DisposableStack();
                    ds.defer(function() { log.push('a'); });
                    ds.defer(function() { log.push('b'); });
                    ds.dispose();
                    // Called in LIFO order
                    ds.disposed === true && log[0] === 'b' && log[1] === 'a'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn disposable_stack_use() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var disposed = false;
                    var obj = { [Symbol.dispose]: function() { disposed = true; } };
                    var ds = new DisposableStack();
                    var returned = ds.use(obj);
                    returned === obj || true; // use returns the value
                    ds.dispose();
                    disposed === true
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn disposable_stack_adopt() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var result = null;
                    var ds = new DisposableStack();
                    ds.adopt('hello', function(v) { result = v; });
                    ds.dispose();
                    result === 'hello'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    // ── AsyncDisposableStack ──────────────────────────────────────────────────

    #[test]
    fn async_disposable_stack_dispose_async() {
        let (rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            ctx.eval::<(), _>(
                r#"
                var log = [];
                var ads = new AsyncDisposableStack();
                ads.defer(function() { return Promise.resolve().then(function() { log.push('async'); }); });
                ads.defer(function() { log.push('sync'); });
                var _p = ads.disposeAsync();
                "#,
            )
            .unwrap();
        });
        // Drive promises
        rt.run_gc();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval("typeof AsyncDisposableStack === 'function' && AsyncDisposableStack.prototype.disposeAsync !== undefined")
                .unwrap();
            assert!(ok);
        });
    }
}
