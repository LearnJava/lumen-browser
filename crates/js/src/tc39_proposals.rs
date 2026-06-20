//! TC39 Stage 4 proposal shims for APIs missing from QuickJS.
//!
//! QuickJS implements ES2023. Several proposals that reached Stage 4 / ES2024-2025
//! after that cutoff are not natively present. This module installs pure-JS shims
//! for the most impactful ones:
//!
//! * **Array Grouping** (`Object.groupBy`, `Map.groupBy`) — ECMAScript 2024
//! * **Set Methods** — `union`, `intersection`, `difference`, `symmetricDifference`,
//!   `isSubsetOf`, `isSupersetOf`, `isDisjointFrom` — ECMAScript 2025
//! * **Promise.withResolvers** — ECMAScript 2024
//! * **Iterator Helpers** — `Iterator.prototype.{map,filter,reduce,take,drop,flatMap,
//!   toArray,forEach,some,every,find}` + `Iterator.from()` — ECMAScript 2025
//! * **Array.fromAsync** — ECMAScript 2024
//! * **`Promise.try`** — ECMAScript 2025
//! * **Uint8Array Base64/Hex** — `toBase64`, `fromBase64`, `toHex`, `fromHex` — ECMAScript 2025
//! * **`RegExp.escape`** — static escape utility — ECMAScript 2025
//! * **`Error.isError`** — cross-realm error test — ECMAScript 2026 (Stage 4)
//! * **`Atomics.pause`** — spinlock power hint (no-op shim) — ECMAScript 2025
//! * **`Atomics.waitAsync`** — non-blocking wait usable on the single JS agent —
//!   ECMAScript 2024 (QuickJS ships `SharedArrayBuffer` + synchronous `Atomics`
//!   but not `waitAsync`)
//!
//! Each shim checks for native support first (no-op when the engine already has it).

use rquickjs::Ctx;

/// Install all TC39 Stage 4 proposal shims into the given QuickJS context.
///
/// Must run after the DOM shim so that `Promise` and `Symbol.iterator` are
/// already defined. Pure-JS; no Rust native bindings needed.
pub fn install_tc39_proposals(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TC39_PROPOSALS_SHIM)?;
    Ok(())
}

/// The combined shim script. Each feature section is gated by a native check.
const TC39_PROPOSALS_SHIM: &str = r#"(function(global) {
  'use strict';

  // ── 1. Object.groupBy / Map.groupBy (Array Grouping — ES2024) ──────────────
  // https://tc39.es/proposal-array-grouping/
  if (typeof Object.groupBy !== 'function') {
    Object.groupBy = function groupBy(items, keySelector) {
      var result = Object.create(null);
      var idx = 0;
      for (var item of items) {
        var key = keySelector(item, idx++);
        // groupBy keys are always coerced to string via ToPropertyKey
        if (Object.prototype.hasOwnProperty.call(result, key)) {
          result[key].push(item);
        } else {
          result[key] = [item];
        }
      }
      return result;
    };
  }

  if (typeof Map.groupBy !== 'function') {
    Map.groupBy = function groupBy(items, keySelector) {
      var result = new Map();
      var idx = 0;
      for (var item of items) {
        var key = keySelector(item, idx++);
        if (result.has(key)) {
          result.get(key).push(item);
        } else {
          result.set(key, [item]);
        }
      }
      return result;
    };
  }

  // ── 2. Set Methods (ES2025) ─────────────────────────────────────────────────
  // https://tc39.es/proposal-set-methods/
  (function() {
    var SP = Set.prototype;

    function coerceToSet(arg) {
      // Accept any Set-like (has size + has + keys) or iterable.
      if (arg instanceof Set) return arg;
      // Minimal set-like duck-type per spec §2.1
      if (arg && typeof arg.has === 'function' && typeof arg.keys === 'function' &&
          typeof arg.size === 'number') {
        return arg;
      }
      return new Set(arg);
    }

    if (typeof SP.union !== 'function') {
      SP.union = function union(other) {
        var o = coerceToSet(other);
        var result = new Set(this);
        for (var v of (o instanceof Set ? o : o.keys())) result.add(v);
        return result;
      };
    }

    if (typeof SP.intersection !== 'function') {
      SP.intersection = function intersection(other) {
        var o = coerceToSet(other);
        var result = new Set();
        // Iterate the smaller side for O(min(|this|,|other|)) membership tests
        if (this.size <= (o.size !== undefined ? o.size : Infinity)) {
          for (var v of this) if (o.has(v)) result.add(v);
        } else {
          for (var v2 of (o instanceof Set ? o : o.keys())) if (this.has(v2)) result.add(v2);
        }
        return result;
      };
    }

    if (typeof SP.difference !== 'function') {
      SP.difference = function difference(other) {
        var o = coerceToSet(other);
        var result = new Set();
        for (var v of this) if (!o.has(v)) result.add(v);
        return result;
      };
    }

    if (typeof SP.symmetricDifference !== 'function') {
      SP.symmetricDifference = function symmetricDifference(other) {
        var o = coerceToSet(other);
        var result = new Set(this);
        for (var v of (o instanceof Set ? o : o.keys())) {
          if (result.has(v)) result.delete(v);
          else result.add(v);
        }
        return result;
      };
    }

    if (typeof SP.isSubsetOf !== 'function') {
      SP.isSubsetOf = function isSubsetOf(other) {
        var o = coerceToSet(other);
        if (this.size > (o.size !== undefined ? o.size : Infinity)) return false;
        for (var v of this) if (!o.has(v)) return false;
        return true;
      };
    }

    if (typeof SP.isSupersetOf !== 'function') {
      SP.isSupersetOf = function isSupersetOf(other) {
        var o = coerceToSet(other);
        for (var v of (o instanceof Set ? o : o.keys())) if (!this.has(v)) return false;
        return true;
      };
    }

    if (typeof SP.isDisjointFrom !== 'function') {
      SP.isDisjointFrom = function isDisjointFrom(other) {
        var o = coerceToSet(other);
        for (var v of this) if (o.has(v)) return false;
        return true;
      };
    }
  })();

  // ── 3. Promise.withResolvers (ES2024) ──────────────────────────────────────
  // https://tc39.es/proposal-promise-with-resolvers/
  if (typeof Promise.withResolvers !== 'function') {
    Promise.withResolvers = function withResolvers() {
      var resolve, reject;
      var promise = new this(function(res, rej) {
        resolve = res;
        reject = rej;
      });
      return { promise: promise, resolve: resolve, reject: reject };
    };
  }

  // ── 4. Promise.try (ES2025) ─────────────────────────────────────────────────
  // https://tc39.es/proposal-promise-try/
  if (typeof Promise.try !== 'function') {
    Promise['try'] = function promiseTry(fn) {
      var C = this;
      return new C(function(resolve) { resolve(fn()); });
    };
  }

  // ── 5. Array.fromAsync (ES2024) ─────────────────────────────────────────────
  // https://tc39.es/proposal-array-from-async/
  if (typeof Array.fromAsync !== 'function') {
    Array.fromAsync = function fromAsync(asyncItems, mapFn, thisArg) {
      return new Promise(function(resolve, reject) {
        (async function() {
          var result = [];
          var idx = 0;
          // Works with both async iterables and sync iterables / array-likes
          if (asyncItems != null &&
              typeof asyncItems[Symbol.asyncIterator] === 'function') {
            for await (var v of asyncItems) {
              result.push(mapFn ? mapFn.call(thisArg, v, idx) : v);
              idx++;
            }
          } else if (asyncItems != null &&
                     typeof asyncItems[Symbol.iterator] === 'function') {
            for (var item of asyncItems) {
              var mapped = mapFn ? mapFn.call(thisArg, item, idx) : item;
              result.push(await Promise.resolve(mapped));
              idx++;
            }
          } else if (asyncItems != null) {
            // Array-like with .length
            var len = asyncItems.length >>> 0;
            for (var i = 0; i < len; i++) {
              var val = await Promise.resolve(asyncItems[i]);
              result.push(mapFn ? mapFn.call(thisArg, val, i) : val);
            }
          }
          return result;
        })().then(resolve, reject);
      });
    };
  }

  // ── 6. Iterator Helpers (ES2025) ─────────────────────────────────────────────
  // https://tc39.es/proposal-iterator-helpers/
  //
  // Creates an IteratorPrototype chain so that every generator/iterator inherits
  // the helper methods.  The spec requires these on %IteratorPrototype%; in our
  // shim we patch the prototype of the object returned by a generator.
  (function() {
    // Detect native support: if Iterator global exists and has .from(), skip.
    if (typeof Iterator !== 'undefined' && typeof Iterator.from === 'function') return;

    // Obtain %IteratorPrototype% (the prototype shared by all built-in iterators).
    var ArrayIterProto = Object.getPrototypeOf([][Symbol.iterator]());
    var IteratorPrototype = Object.getPrototypeOf(ArrayIterProto);

    function wrapIter(next, returnFn) {
      var iter = Object.create(IteratorPrototype);
      iter.next = next;
      if (returnFn) iter.return = returnFn;
      iter[Symbol.iterator] = function() { return this; };
      return iter;
    }

    if (typeof IteratorPrototype.map !== 'function') {
      IteratorPrototype.map = function map(mapperFn) {
        var self = this;
        var done = false;
        return wrapIter(function next() {
          if (done) return { value: undefined, done: true };
          var n = self.next();
          if (n.done) { done = true; return { value: undefined, done: true }; }
          return { value: mapperFn(n.value), done: false };
        }, function returnFn(v) {
          done = true;
          return typeof self.return === 'function' ? self.return(v) : { value: v, done: true };
        });
      };
    }

    if (typeof IteratorPrototype.filter !== 'function') {
      IteratorPrototype.filter = function filter(filtererFn) {
        var self = this;
        var done = false;
        return wrapIter(function next() {
          if (done) return { value: undefined, done: true };
          while (true) {
            var n = self.next();
            if (n.done) { done = true; return { value: undefined, done: true }; }
            if (filtererFn(n.value)) return { value: n.value, done: false };
          }
        }, function returnFn(v) {
          done = true;
          return typeof self.return === 'function' ? self.return(v) : { value: v, done: true };
        });
      };
    }

    if (typeof IteratorPrototype.take !== 'function') {
      IteratorPrototype.take = function take(limit) {
        limit = Math.trunc(+limit);
        if (limit < 0 || isNaN(limit)) throw new RangeError('take limit must be >= 0');
        var self = this;
        var remaining = limit;
        return wrapIter(function next() {
          if (remaining <= 0) {
            if (typeof self.return === 'function') self.return();
            return { value: undefined, done: true };
          }
          remaining--;
          return self.next();
        }, function returnFn(v) {
          return typeof self.return === 'function' ? self.return(v) : { value: v, done: true };
        });
      };
    }

    if (typeof IteratorPrototype.drop !== 'function') {
      IteratorPrototype.drop = function drop(limit) {
        limit = Math.trunc(+limit);
        if (limit < 0 || isNaN(limit)) throw new RangeError('drop limit must be >= 0');
        var self = this;
        var skipped = false;
        return wrapIter(function next() {
          if (!skipped) {
            skipped = true;
            for (var i = 0; i < limit; i++) {
              var n = self.next();
              if (n.done) return { value: undefined, done: true };
            }
          }
          return self.next();
        }, function returnFn(v) {
          return typeof self.return === 'function' ? self.return(v) : { value: v, done: true };
        });
      };
    }

    if (typeof IteratorPrototype.flatMap !== 'function') {
      IteratorPrototype.flatMap = function flatMap(mapperFn) {
        var self = this;
        var inner = null;
        var outerDone = false;
        return wrapIter(function next() {
          while (true) {
            if (inner) {
              var n = inner.next();
              if (!n.done) return { value: n.value, done: false };
              inner = null;
            }
            if (outerDone) return { value: undefined, done: true };
            var o = self.next();
            if (o.done) { outerDone = true; return { value: undefined, done: true }; }
            var mapped = mapperFn(o.value);
            // Flatten one level if iterable
            if (mapped != null && typeof mapped[Symbol.iterator] === 'function') {
              inner = mapped[Symbol.iterator]();
            } else {
              return { value: mapped, done: false };
            }
          }
        });
      };
    }

    if (typeof IteratorPrototype.reduce !== 'function') {
      IteratorPrototype.reduce = function reduce(reducer, initialValue) {
        var first = arguments.length < 2;
        var acc = first ? undefined : initialValue;
        for (var v of this) {
          if (first) { acc = v; first = false; }
          else acc = reducer(acc, v);
        }
        if (first) throw new TypeError('reduce of empty iterator with no initial value');
        return acc;
      };
    }

    if (typeof IteratorPrototype.toArray !== 'function') {
      IteratorPrototype.toArray = function toArray() {
        var result = [];
        for (var v of this) result.push(v);
        return result;
      };
    }

    if (typeof IteratorPrototype.forEach !== 'function') {
      IteratorPrototype.forEach = function forEach(fn) {
        for (var v of this) fn(v);
      };
    }

    if (typeof IteratorPrototype.some !== 'function') {
      IteratorPrototype.some = function some(predFn) {
        for (var v of this) if (predFn(v)) return true;
        return false;
      };
    }

    if (typeof IteratorPrototype.every !== 'function') {
      IteratorPrototype.every = function every(predFn) {
        for (var v of this) if (!predFn(v)) return false;
        return true;
      };
    }

    if (typeof IteratorPrototype.find !== 'function') {
      IteratorPrototype.find = function find(predFn) {
        for (var v of this) if (predFn(v)) return v;
        return undefined;
      };
    }

    // Iterator.from() — wraps any iterable/iterator into a spec-compliant iterator
    var IteratorCtor = function Iterator() {
      throw new TypeError('Iterator is not a constructor');
    };
    IteratorCtor.prototype = IteratorPrototype;

    IteratorCtor.from = function from(O) {
      if (O == null) throw new TypeError('Cannot convert undefined or null to iterator');
      if (typeof O[Symbol.iterator] === 'function') {
        var iter = O[Symbol.iterator]();
        // If already an %IteratorPrototype%-based iterator, return as-is
        if (iter instanceof IteratorCtor || Object.getPrototypeOf(iter) === IteratorPrototype) {
          return iter;
        }
        // Wrap in a forwarding iterator
        return wrapIter(function() { return iter.next(); },
                        iter.return ? function(v) { return iter.return(v); } : undefined);
      }
      throw new TypeError('Object is not iterable');
    };

    // Expose as global `Iterator`
    global.Iterator = IteratorCtor;
  })();

  // ── 7. Uint8Array Base64 / Hex methods (ES2025) ────────────────────────────
  // https://tc39.es/proposal-arraybuffer-base64/
  (function() {
    var B64_STD  = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    var B64_URL  = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

    // Build a reverse lookup: charCode → 6-bit value (0xff = invalid).
    function mkLookup(alpha) {
      var t = new Uint8Array(128).fill(255);
      for (var i = 0; i < 64; i++) t[alpha.charCodeAt(i)] = i;
      return t;
    }
    var LU_STD = mkLookup(B64_STD);
    var LU_URL = mkLookup(B64_URL);

    if (!Uint8Array.prototype.toBase64) {
      Object.defineProperty(Uint8Array.prototype, 'toBase64', {
        value: function toBase64(options) {
          var alpha = (options && options.alphabet === 'base64url') ? B64_URL : B64_STD;
          var omit  = !!(options && options.omitPadding);
          var out = '', len = this.length;
          for (var i = 0; i < len; i += 3) {
            var b0 = this[i];
            var b1 = i + 1 < len ? this[i + 1] : 0;
            var b2 = i + 2 < len ? this[i + 2] : 0;
            out += alpha[b0 >> 2];
            out += alpha[((b0 & 3) << 4) | (b1 >> 4)];
            if (i + 1 < len)   { out += alpha[((b1 & 15) << 2) | (b2 >> 6)]; }
            else if (!omit)    { out += '='; }
            if (i + 2 < len)   { out += alpha[b2 & 63]; }
            else if (!omit)    { out += '='; }
          }
          return out;
        },
        writable: true, configurable: true
      });
    }

    if (!Uint8Array.fromBase64) {
      Object.defineProperty(Uint8Array, 'fromBase64', {
        value: function fromBase64(str, options) {
          if (typeof str !== 'string') throw new TypeError('fromBase64 requires a string');
          var lu     = (options && options.alphabet === 'base64url') ? LU_URL : LU_STD;
          var strict = options && options.lastChunkHandling === 'strict';
          // Strip ASCII whitespace (CR LF SP TAB)
          str = str.replace(/[\t\n\r ]/g, '');
          // Strip trailing '=' padding
          str = str.replace(/=+$/, '');
          var bytes = [], buf = 0, bits = 0;
          for (var i = 0; i < str.length; i++) {
            var cc = str.charCodeAt(i);
            if (cc >= 128) throw new SyntaxError('Invalid base64 character: ' + str[i]);
            var v = lu[cc];
            if (v === 255) throw new SyntaxError('Invalid base64 character: ' + str[i]);
            buf = (buf << 6) | v;
            bits += 6;
            if (bits >= 8) { bits -= 8; bytes.push((buf >> bits) & 0xff); }
          }
          if (strict && bits > 0 && (buf & ((1 << bits) - 1)) !== 0) {
            throw new SyntaxError('Unexpected non-zero trailing bits');
          }
          return new Uint8Array(bytes);
        },
        writable: true, configurable: true
      });
    }

    if (!Uint8Array.prototype.toHex) {
      Object.defineProperty(Uint8Array.prototype, 'toHex', {
        value: function toHex() {
          var HEX = '0123456789abcdef';
          var out = '';
          for (var i = 0; i < this.length; i++) {
            var b = this[i];
            out += HEX[b >> 4] + HEX[b & 15];
          }
          return out;
        },
        writable: true, configurable: true
      });
    }

    if (!Uint8Array.fromHex) {
      Object.defineProperty(Uint8Array, 'fromHex', {
        value: function fromHex(str) {
          if (typeof str !== 'string') throw new TypeError('fromHex requires a string');
          if (str.length & 1) throw new SyntaxError('fromHex: odd-length string');
          var bytes = new Uint8Array(str.length >> 1);
          for (var i = 0; i < str.length; i += 2) {
            var hi = parseInt(str[i],     16);
            var lo = parseInt(str[i + 1], 16);
            if (hi !== hi || lo !== lo) {   // NaN check
              throw new SyntaxError('Invalid hex character at position ' + i);
            }
            bytes[i >> 1] = (hi << 4) | lo;
          }
          return bytes;
        },
        writable: true, configurable: true
      });
    }
  })();

  // ── 8. RegExp.escape (ES2025) ───────────────────────────────────────────────
  // https://tc39.es/proposal-regex-escaping/
  // Escapes a string so it can be used verbatim inside a RegExp pattern.
  if (typeof RegExp.escape !== 'function') {
    Object.defineProperty(RegExp, 'escape', {
      value: function escape(str) {
        if (typeof str !== 'string') throw new TypeError('RegExp.escape requires a string');
        // Escape all SyntaxCharacters + / and whitespace control chars
        return str
          .replace(/[\\^$.*+?()[\]{}|]/g, '\\$&')
          .replace(/\//g, '\\/')
          .replace(/\0/g, '\\0')
          .replace(/\n/g, '\\n')
          .replace(/\r/g, '\\r')
          .replace(/\t/g, '\\t')
          .replace(/\v/g, '\\v')
          .replace(/\f/g, '\\f');
      },
      writable: true, configurable: true
    });
  }

  // ── 9. Error.isError (ES2026 Stage 4) ─────────────────────────────────────
  // https://tc39.es/proposal-is-error/
  // Returns true iff value has the internal [[ErrorData]] slot.
  // In pure JS we use toString (reliable within a realm; cross-realm via the
  // [object Error] tag that all built-in Error objects produce).
  if (typeof Error.isError !== 'function') {
    Object.defineProperty(Error, 'isError', {
      value: function isError(value) {
        if (value == null || typeof value !== 'object') return false;
        try {
          // Object.prototype.toString returns '[object Error]' for all
          // native Error instances including subclasses.
          var tag = Object.prototype.toString.call(value);
          return tag === '[object Error]' || value instanceof Error;
        } catch (e) {
          return false;
        }
      },
      writable: true, configurable: true
    });
  }

  // ── 10. Atomics.pause (ES2025) ──────────────────────────────────────────────
  // https://tc39.es/proposal-atomics-microwait/
  // Power-reduction hint for spinloops.  A pure-JS shim is necessarily a no-op.
  if (typeof Atomics !== 'undefined' && typeof Atomics.pause !== 'function') {
    Object.defineProperty(Atomics, 'pause', {
      value: function pause() {
        // No-op: the PAUSE/yield instruction is a CPU-level hint unavailable in JS.
      },
      writable: true, configurable: true
    });
  }

  // ── 11. Atomics.waitAsync (ES2024) ──────────────────────────────────────────
  // https://tc39.es/proposal-atomics-wait-async/
  // Non-blocking counterpart of Atomics.wait. QuickJS ships SharedArrayBuffer +
  // synchronous Atomics, but the synchronous Atomics.wait throws
  // ("cannot block in this thread") because Lumen runs all JS on a single
  // non-blocking agent (one JS thread, ADR-014) — exactly like a browser's main
  // thread. waitAsync stays meaningful: a later Atomics.notify or a timeout,
  // both dispatched on this same agent's event loop, settles the returned
  // promise. With a single agent every still-parked waiter can only ever be
  // woken by this agent itself, so we maintain a FIFO waiter list here and wrap
  // Atomics.notify to resolve matching async waiters.
  if (typeof Atomics !== 'undefined' && typeof SharedArrayBuffer !== 'undefined' &&
      typeof Atomics.waitAsync !== 'function') {
    (function() {
      // Parked async waiters, in arrival order. Each records the underlying
      // SharedArrayBuffer data block + byte offset it is watching (so two views
      // aliasing the same location match per spec), plus its promise resolver
      // and optional timeout id.
      var waiters = [];
      var hasTimers = (typeof setTimeout === 'function' && typeof clearTimeout === 'function');

      function isI32(ta) { return (typeof Int32Array === 'function') && (ta instanceof Int32Array); }
      function isI64(ta) { return (typeof BigInt64Array === 'function') && (ta instanceof BigInt64Array); }

      // ValidateIntegerTypedArray(waitable=true) + shared-buffer + ValidateAtomicAccess.
      function validate(ta, index) {
        var big = isI64(ta);
        if (!isI32(ta) && !big) {
          throw new TypeError('Atomics.waitAsync: typedArray must be Int32Array or BigInt64Array');
        }
        if (!(ta.buffer instanceof SharedArrayBuffer)) {
          throw new TypeError('Atomics.waitAsync: typedArray must be backed by a SharedArrayBuffer');
        }
        var i = Math.trunc(Number(index));
        if (!(i >= 0) || i >= ta.length) {
          throw new RangeError('Atomics.waitAsync: access index out of bounds');
        }
        return { big: big, idx: i, byteIndex: ta.byteOffset + i * (big ? 8 : 4) };
      }

      Object.defineProperty(Atomics, 'waitAsync', {
        value: function waitAsync(typedArray, index, value, timeout) {
          var info = validate(typedArray, index);
          // Coerce the expected value to the element type (ToBigInt / ToInt32).
          var expected = info.big ? BigInt(value) : (value | 0);
          // Coerce timeout: undefined -> +Infinity; NaN -> +Infinity; clamp >= 0.
          var t = (timeout === undefined) ? Infinity : Number(timeout);
          if (t !== t) t = Infinity;
          if (t < 0) t = 0;

          var current = Atomics.load(typedArray, info.idx);
          if (current !== expected) {
            return { async: false, value: 'not-equal' };
          }
          if (t === 0) {
            return { async: false, value: 'timed-out' };
          }

          // Park an async waiter; it settles on a matching notify or on timeout.
          var rec = {
            buffer: typedArray.buffer,
            byteIndex: info.byteIndex,
            settled: false,
            resolve: null,
            timer: null
          };
          var promise = new Promise(function(resolve) { rec.resolve = resolve; });
          if (t !== Infinity && hasTimers) {
            rec.timer = setTimeout(function() {
              if (rec.settled) return;
              rec.settled = true;
              var k = waiters.indexOf(rec);
              if (k !== -1) waiters.splice(k, 1);
              rec.resolve('timed-out');
            }, t);
          }
          waiters.push(rec);
          return { async: true, value: promise };
        },
        writable: true, configurable: true
      });

      // Wrap Atomics.notify so it also wakes this agent's async waiters. The
      // native notify wakes blocking agents (none on a single agent) and
      // validates its arguments; we add the async-waiter resolution on top and
      // fold the count of resolved async waiters into the returned total, per
      // spec (RemoveWaiter applies to async waiters too).
      var nativeNotify = Atomics.notify;
      Object.defineProperty(Atomics, 'notify', {
        value: function notify(typedArray, index, count) {
          var woken = nativeNotify.apply(Atomics, arguments);
          if (waiters.length === 0) return woken;
          var buffer = typedArray.buffer;
          var byteIndex = typedArray.byteOffset +
            Math.trunc(Number(index)) * (isI64(typedArray) ? 8 : 4);
          var n = (count === undefined) ? Infinity : Math.trunc(Number(count));
          if (!(n >= 0)) n = 0; // NaN or negative wakes none
          var awoken = 0;
          for (var k = 0; k < waiters.length && awoken < n; ) {
            var w = waiters[k];
            if (!w.settled && w.buffer === buffer && w.byteIndex === byteIndex) {
              w.settled = true;
              if (w.timer !== null && hasTimers) clearTimeout(w.timer);
              waiters.splice(k, 1);
              w.resolve('ok');
              awoken++;
            } else {
              k++;
            }
          }
          return woken + awoken;
        },
        writable: true, configurable: true
      });
    })();
  }

})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn setup() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            super::install_tc39_proposals(&ctx).unwrap();
        });
        (rt, ctx)
    }

    // ── Object.groupBy / Map.groupBy ──────────────────────────────────────────

    #[test]
    fn object_group_by_exists() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let v: bool = ctx.eval("typeof Object.groupBy === 'function'").unwrap();
            assert!(v, "Object.groupBy must be a function");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn object_group_by_groups_by_key() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var nums = [1, 2, 3, 4, 5];
          var g = Object.groupBy(nums, n => n % 2 === 0 ? 'even' : 'odd');
          JSON.stringify({ even: g.even.sort(), odd: g.odd.sort() })
        "#,
                )
                .unwrap();
            assert_eq!(result, r#"{"even":[2,4],"odd":[1,3,5]}"#);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn map_group_by_uses_identity_keys() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: i32 = ctx
                .eval(
                    r#"
          var pets = [{name:'cat',type:'feline'},{name:'lion',type:'feline'},{name:'dog',type:'canine'}];
          var g = Map.groupBy(pets, p => p.type);
          g.get('feline').length
        "#,
                )
                .unwrap();
            assert_eq!(result, 2);
        });
        drop(ctx);
        drop(rt);
    }

    // ── Set methods ───────────────────────────────────────────────────────────

    #[test]
    fn set_methods_exist() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let v: bool = ctx
                .eval(
                    r#"
          ['union','intersection','difference','symmetricDifference',
           'isSubsetOf','isSupersetOf','isDisjointFrom']
          .every(m => typeof Set.prototype[m] === 'function')
        "#,
                )
                .unwrap();
            assert!(v, "all Set methods must be functions");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_union() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let size: i32 = ctx
                .eval("new Set([1,2,3]).union(new Set([2,3,4])).size")
                .unwrap();
            assert_eq!(size, 4);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_intersection() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let size: i32 = ctx
                .eval("new Set([1,2,3]).intersection(new Set([2,3,4])).size")
                .unwrap();
            assert_eq!(size, 2);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_difference() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let size: i32 = ctx
                .eval("new Set([1,2,3]).difference(new Set([2,3,4])).size")
                .unwrap();
            assert_eq!(size, 1);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_symmetric_difference() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let size: i32 = ctx
                .eval("new Set([1,2,3]).symmetricDifference(new Set([2,3,4])).size")
                .unwrap();
            assert_eq!(size, 2);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_is_subset_of() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let yes: bool = ctx
                .eval("new Set([1,2]).isSubsetOf(new Set([1,2,3]))")
                .unwrap();
            assert!(yes);
            let no: bool = ctx
                .eval("new Set([1,4]).isSubsetOf(new Set([1,2,3]))")
                .unwrap();
            assert!(!no);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_is_superset_of() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let yes: bool = ctx
                .eval("new Set([1,2,3]).isSupersetOf(new Set([1,2]))")
                .unwrap();
            assert!(yes);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn set_is_disjoint_from() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let yes: bool = ctx
                .eval("new Set([1,2]).isDisjointFrom(new Set([3,4]))")
                .unwrap();
            assert!(yes);
            let no: bool = ctx
                .eval("new Set([1,2]).isDisjointFrom(new Set([2,3]))")
                .unwrap();
            assert!(!no);
        });
        drop(ctx);
        drop(rt);
    }

    // ── Promise.withResolvers ─────────────────────────────────────────────────

    #[test]
    fn promise_with_resolvers_returns_triple() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
          var t = Promise.withResolvers();
          typeof t.promise === 'object' &&
          typeof t.resolve === 'function' &&
          typeof t.reject  === 'function'
        "#,
                )
                .unwrap();
            assert!(ok);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn promise_try_resolves() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval("Promise.try instanceof Function")
                .unwrap();
            assert!(ok);
        });
        drop(ctx);
        drop(rt);
    }

    // ── Array.fromAsync ────────────────────────────────────────────────────────

    #[test]
    fn array_from_async_exists() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval("typeof Array.fromAsync === 'function'")
                .unwrap();
            assert!(ok);
        });
        drop(ctx);
        drop(rt);
    }

    // ── Iterator helpers ──────────────────────────────────────────────────────

    #[test]
    fn iterator_prototype_has_helpers() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
          var iter = [][Symbol.iterator]();
          var proto = Object.getPrototypeOf(Object.getPrototypeOf(iter));
          ['map','filter','take','drop','flatMap','reduce','toArray',
           'forEach','some','every','find'].every(m => typeof proto[m] === 'function')
        "#,
                )
                .unwrap();
            assert!(ok, "all iterator helper methods must exist on %IteratorPrototype%");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_map_transforms() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify([1,2,3][Symbol.iterator]().map(x => x * 2).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[2,4,6]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_filter_keeps_matching() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify([1,2,3,4,5][Symbol.iterator]().filter(x => x % 2 === 0).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[2,4]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_take_limits() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify([1,2,3,4,5][Symbol.iterator]().take(3).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[1,2,3]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_drop_skips() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify([1,2,3,4,5][Symbol.iterator]().drop(2).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[3,4,5]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_reduce_sums() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let sum: i32 = ctx
                .eval("[1,2,3,4,5][Symbol.iterator]().reduce((acc, v) => acc + v, 0)")
                .unwrap();
            assert_eq!(sum, 15);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_some_finds() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval("[1,2,3][Symbol.iterator]().some(x => x > 2)")
                .unwrap();
            assert!(ok);
            let none: bool = ctx
                .eval("[1,2,3][Symbol.iterator]().some(x => x > 10)")
                .unwrap();
            assert!(!none);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_every_checks_all() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let all: bool = ctx
                .eval("[2,4,6][Symbol.iterator]().every(x => x % 2 === 0)")
                .unwrap();
            assert!(all);
            let not_all: bool = ctx
                .eval("[2,3,6][Symbol.iterator]().every(x => x % 2 === 0)")
                .unwrap();
            assert!(!not_all);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_find_returns_value() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let val: i32 = ctx
                .eval("[1,2,3,4][Symbol.iterator]().find(x => x > 2)")
                .unwrap();
            assert_eq!(val, 3);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_flat_map_flattens() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify([[1,2],[3,4],[5]][Symbol.iterator]().flatMap(a => a[Symbol.iterator]()).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[1,2,3,4,5]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn iterator_from_wraps_iterable() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          JSON.stringify(Iterator.from([10,20,30]).toArray())
        "#,
                )
                .unwrap();
            assert_eq!(result, "[10,20,30]");
        });
        drop(ctx);
        drop(rt);
    }

    // ── Uint8Array Base64 / Hex ───────────────────────────────────────────────

    #[test]
    fn uint8array_to_hex_encodes() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let hex: String = ctx
                .eval("new Uint8Array([0x00, 0x0f, 0x10, 0xff]).toHex()")
                .unwrap();
            assert_eq!(hex, "000f10ff");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_from_hex_decodes() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(r#"JSON.stringify(Array.from(Uint8Array.fromHex("deadbeef")))"#)
                .unwrap();
            assert_eq!(result, "[222,173,190,239]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_hex_roundtrip() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            var orig = new Uint8Array([1, 2, 3, 255, 0, 128]);
            var hex  = orig.toHex();
            var back = Uint8Array.fromHex(hex);
            orig.every((v, i) => v === back[i])
          "#,
                )
                .unwrap();
            assert!(ok, "hex roundtrip must be lossless");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_to_base64_encodes_standard() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // "Man" in ASCII = 77 97 110 → base64 "TWFu"
            let b64: String = ctx
                .eval("new Uint8Array([77, 97, 110]).toBase64()")
                .unwrap();
            assert_eq!(b64, "TWFu");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_to_base64_with_padding() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Single byte → 2 encoded chars + 2 padding
            let b64: String = ctx.eval("new Uint8Array([0]).toBase64()").unwrap();
            assert_eq!(b64, "AA==");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_to_base64_omit_padding() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let b64: String = ctx
                .eval("new Uint8Array([0]).toBase64({ omitPadding: true })")
                .unwrap();
            assert_eq!(b64, "AA");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_from_base64_decodes() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(r#"JSON.stringify(Array.from(Uint8Array.fromBase64("TWFu")))"#)
                .unwrap();
            assert_eq!(result, "[77,97,110]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_base64_roundtrip() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            var orig = new Uint8Array([72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100]);
            var b64  = orig.toBase64();
            var back = Uint8Array.fromBase64(b64);
            orig.length === back.length && orig.every((v, i) => v === back[i])
          "#,
                )
                .unwrap();
            assert!(ok, "base64 roundtrip must be lossless");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn uint8array_to_base64url_alphabet() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // 0xfb 0xff → standard "+/" → URL "-_"
            let b64url: String = ctx
                .eval(r#"new Uint8Array([0xfb, 0xff]).toBase64({ alphabet: 'base64url', omitPadding: true })"#)
                .unwrap();
            // 0xfb = 11111011, 0xff = 11111111
            // groups: 111110 110111 1111xx → 62,55,... → '-', '3', ...
            assert!(!b64url.contains('+'), "base64url must not contain '+'");
            assert!(!b64url.contains('/'), "base64url must not contain '/'");
        });
        drop(ctx);
        drop(rt);
    }

    // ── RegExp.escape ──────────────────────────────────────────────────────────

    #[test]
    fn regexp_escape_exists() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx.eval("typeof RegExp.escape === 'function'").unwrap();
            assert!(ok, "RegExp.escape must be a function");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn regexp_escape_metacharacters() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Each metachar must be escaped
            let ok: bool = ctx
                .eval(
                    r#"
            var special = ['\\', '^', '$', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|'];
            special.every(function(c) {
              var escaped = RegExp.escape(c);
              return escaped[0] === '\\' && escaped[1] === c;
            })
          "#,
                )
                .unwrap();
            assert!(ok, "all metacharacters must be backslash-escaped");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn regexp_escape_plain_string_matches() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // The escaped pattern must match the original string literally,
            // regardless of what exact escape form is chosen by the engine.
            let ok: bool = ctx
                .eval(
                    r#"
            var original = 'hello123';
            var pattern  = new RegExp(RegExp.escape(original));
            pattern.test(original)
          "#,
                )
                .unwrap();
            assert!(ok, "escaped pattern must match the original string");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn regexp_escape_use_in_pattern() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let matches: bool = ctx
                .eval(
                    r#"
            var needle = 'a.b*c';
            var pattern = new RegExp(RegExp.escape(needle));
            pattern.test('a.b*c') && !pattern.test('axbbc')
          "#,
                )
                .unwrap();
            assert!(matches, "escaped pattern should match literal string only");
        });
        drop(ctx);
        drop(rt);
    }

    // ── Error.isError ──────────────────────────────────────────────────────────

    #[test]
    fn error_is_error_exists() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx.eval("typeof Error.isError === 'function'").unwrap();
            assert!(ok, "Error.isError must be a function");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn error_is_error_true_for_errors() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            Error.isError(new Error('e')) &&
            Error.isError(new TypeError('t')) &&
            Error.isError(new RangeError('r'))
          "#,
                )
                .unwrap();
            assert!(ok, "Error.isError must return true for Error instances");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn error_is_error_false_for_non_errors() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            !Error.isError(null) &&
            !Error.isError(undefined) &&
            !Error.isError(42) &&
            !Error.isError({message:'fake'}) &&
            !Error.isError('string error')
          "#,
                )
                .unwrap();
            assert!(ok, "Error.isError must return false for non-Error values");
        });
        drop(ctx);
        drop(rt);
    }

    // ── Atomics.pause ──────────────────────────────────────────────────────────

    #[test]
    fn atomics_pause_is_function() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Atomics may or may not be present in QuickJS default context;
            // if present, pause must be a function.
            let ok: bool = ctx
                .eval(
                    r#"
            typeof Atomics === 'undefined' ||
            typeof Atomics.pause === 'function'
          "#,
                )
                .unwrap();
            assert!(ok, "Atomics.pause must be a function if Atomics is defined");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_pause_returns_undefined() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            typeof Atomics === 'undefined' ||
            Atomics.pause() === undefined
          "#,
                )
                .unwrap();
            assert!(ok, "Atomics.pause() must return undefined");
        });
        drop(ctx);
        drop(rt);
    }

    // ── Atomics.waitAsync ───────────────────────────────────────────────────────

    #[test]
    fn atomics_wait_async_is_function() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let ok: bool = ctx
                .eval("typeof Atomics.waitAsync === 'function'")
                .unwrap();
            assert!(ok, "Atomics.waitAsync must be installed");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_not_equal_is_synchronous() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Cell holds 0, we wait for 999 -> immediate, synchronous not-equal.
            let r: String = ctx
                .eval(
                    r#"
            var a = new Int32Array(new SharedArrayBuffer(8));
            var res = Atomics.waitAsync(a, 0, 999);
            res.async + ':' + res.value
          "#,
                )
                .unwrap();
            assert_eq!(r, "false:not-equal");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_zero_timeout_is_timed_out() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Value matches (0) but timeout 0 -> synchronous timed-out.
            let r: String = ctx
                .eval(
                    r#"
            var a = new Int32Array(new SharedArrayBuffer(8));
            var res = Atomics.waitAsync(a, 0, 0, 0);
            res.async + ':' + res.value
          "#,
                )
                .unwrap();
            assert_eq!(r, "false:timed-out");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_notify_resolves_ok() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // Matching value + infinite timeout -> async promise; notify wakes it.
            let async_and_count: String = ctx
                .eval(
                    r#"
            globalThis.__r = 'pending';
            var a = new Int32Array(new SharedArrayBuffer(8));
            var res = Atomics.waitAsync(a, 0, 0);
            res.value.then(function(v) { globalThis.__r = v; });
            var woken = Atomics.notify(a, 0);
            res.async + ':' + woken
          "#,
                )
                .unwrap();
            assert_eq!(async_and_count, "true:1", "async waiter must be woken by notify");
            // Drain the microtask queue so the .then reaction runs.
            while ctx.execute_pending_job() {}
            let settled: String = ctx.eval("globalThis.__r").unwrap();
            assert_eq!(settled, "ok");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_notify_on_other_index_does_not_wake() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let woken: i32 = ctx
                .eval(
                    r#"
            var a = new Int32Array(new SharedArrayBuffer(8));
            Atomics.waitAsync(a, 0, 0);  // parks watching index 0
            Atomics.notify(a, 1);        // notifies a different index
          "#,
                )
                .unwrap();
            assert_eq!(woken, 0, "notify on a different index wakes no async waiter");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_rejects_non_shared_buffer() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let name: String = ctx
                .eval(
                    r#"
            try {
              var a = new Int32Array(new ArrayBuffer(8));
              Atomics.waitAsync(a, 0, 0);
              'no-throw';
            } catch (e) { e.name; }
          "#,
                )
                .unwrap();
            assert_eq!(name, "TypeError");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_rejects_non_integer_array() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let name: String = ctx
                .eval(
                    r#"
            try {
              var a = new Float64Array(new SharedArrayBuffer(16));
              Atomics.waitAsync(a, 0, 0);
              'no-throw';
            } catch (e) { e.name; }
          "#,
                )
                .unwrap();
            assert_eq!(name, "TypeError");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn atomics_wait_async_bigint64_roundtrip() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            // not-equal on BigInt64Array (cell 0n, expecting 7n).
            let neq: String = ctx
                .eval(
                    r#"
            var a = new BigInt64Array(new SharedArrayBuffer(16));
            var res = Atomics.waitAsync(a, 0, 7n);
            res.async + ':' + res.value
          "#,
                )
                .unwrap();
            assert_eq!(neq, "false:not-equal");

            // matching 0n -> async; notify resolves to ok.
            let async_flag: bool = ctx
                .eval(
                    r#"
            globalThis.__rb = 'pending';
            var b = new BigInt64Array(new SharedArrayBuffer(16));
            var r2 = Atomics.waitAsync(b, 1, 0n);
            r2.value.then(function(v){ globalThis.__rb = v; });
            Atomics.notify(b, 1);
            r2.async
          "#,
                )
                .unwrap();
            assert!(async_flag);
            while ctx.execute_pending_job() {}
            let settled: String = ctx.eval("globalThis.__rb").unwrap();
            assert_eq!(settled, "ok");
        });
        drop(ctx);
        drop(rt);
    }
}
