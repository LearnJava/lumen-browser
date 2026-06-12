//! AsyncContext (TC39 Stage 2.7) — Phase 0 shim.
//!
//! <https://tc39.es/proposal-async-context/>
//!
//! Installs `AsyncContext.Variable` and `AsyncContext.Snapshot` as a pure-JS
//! shim. The context mapping is a copy-on-write `Map` keyed by `Variable`
//! identity; `Variable.prototype.run` swaps the mapping for the duration of
//! the callback and restores it afterwards (spec §2.1.2 "run").
//!
//! **Microtask propagation.** `Promise.prototype.then` is patched to capture
//! the current mapping at *registration* time and restore it around the
//! reaction callback (spec: snapshot at `EnqueueJob`). `catch`/`finally`
//! delegate to the public `then` per ECMA-262, and the DOM shim implements
//! `queueMicrotask(fn)` as `Promise.resolve().then(fn)` — both therefore
//! propagate automatically through the same patch.
//!
//! **Phase 0 limitations** (no-op outside async chains):
//! * `await` continuations use the engine-internal `PerformPromiseThen`, not
//!   the patched public `then` — context does not flow across `await`.
//! * Tasks (`setTimeout`, event handlers) are not wrapped — callbacks
//!   scheduled there observe the empty/default mapping.
//! * `AsyncContext.Snapshot.wrap` is provided for manual propagation where
//!   the automatic one does not reach.

use rquickjs::Ctx;

/// Install the `AsyncContext` global (Variable + Snapshot) into the context.
///
/// Must run after the DOM shim so `Promise` is already in its final shape:
/// the shim patches `Promise.prototype.then` for microtask propagation.
/// No-ops if `AsyncContext` is already defined. Pure JS, no native bindings.
pub fn install_async_context(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(ASYNC_CONTEXT_SHIM)?;
    Ok(())
}

/// The AsyncContext shim script. See module docs for scope and limitations.
const ASYNC_CONTEXT_SHIM: &str = r#"(function(global) {
  'use strict';

  if (typeof global.AsyncContext !== 'undefined') return;

  // Current context mapping: Variable instance -> value. Treated as immutable —
  // `run` builds a new Map (copy-on-write), so a captured reference is a
  // consistent snapshot without further copying.
  var currentMapping = new Map();

  // Internal state keyed by instance identity; kept out of the objects so
  // user code cannot forge or tamper with it.
  var variableDefaults = new WeakMap();
  var snapshotMappings = new WeakMap();

  // ── AsyncContext.Variable (§2.1) ──────────────────────────────────────────
  function Variable(options) {
    if (!(this instanceof Variable)) {
      throw new TypeError("AsyncContext.Variable constructor requires 'new'");
    }
    var name = '';
    var defaultValue;
    if (options !== undefined && options !== null) {
      var opts = Object(options);
      if (opts.name !== undefined) name = String(opts.name);
      defaultValue = opts.defaultValue;
    }
    Object.defineProperty(this, 'name', {
      value: name, writable: false, enumerable: false, configurable: true
    });
    variableDefaults.set(this, defaultValue);
  }

  // Current value: innermost enclosing `run` for this variable, else default.
  Variable.prototype.get = function get() {
    if (!variableDefaults.has(this)) {
      throw new TypeError('AsyncContext.Variable.prototype.get called on incompatible receiver');
    }
    if (currentMapping.has(this)) return currentMapping.get(this);
    return variableDefaults.get(this);
  };

  // run(value, fn, ...args): call fn with this variable set to value.
  // The mapping is restored on exit — including on throw.
  Variable.prototype.run = function run(value, fn) {
    if (!variableDefaults.has(this)) {
      throw new TypeError('AsyncContext.Variable.prototype.run called on incompatible receiver');
    }
    if (typeof fn !== 'function') {
      throw new TypeError('AsyncContext.Variable.prototype.run: callback must be a function');
    }
    var previous = currentMapping;
    var next = new Map(previous);
    next.set(this, value);
    currentMapping = next;
    try {
      return fn.apply(undefined, Array.prototype.slice.call(arguments, 2));
    } finally {
      currentMapping = previous;
    }
  };

  // ── AsyncContext.Snapshot (§2.2) ──────────────────────────────────────────
  // Captures the entire mapping at construction time.
  function Snapshot() {
    if (!(this instanceof Snapshot)) {
      throw new TypeError("AsyncContext.Snapshot constructor requires 'new'");
    }
    snapshotMappings.set(this, currentMapping);
  }

  // run(fn, ...args): call fn with the captured mapping as current.
  Snapshot.prototype.run = function run(fn) {
    if (!snapshotMappings.has(this)) {
      throw new TypeError('AsyncContext.Snapshot.prototype.run called on incompatible receiver');
    }
    if (typeof fn !== 'function') {
      throw new TypeError('AsyncContext.Snapshot.prototype.run: callback must be a function');
    }
    var previous = currentMapping;
    currentMapping = snapshotMappings.get(this);
    try {
      return fn.apply(undefined, Array.prototype.slice.call(arguments, 1));
    } finally {
      currentMapping = previous;
    }
  };

  // Snapshot.wrap(fn): bind fn to the context active right now; `this` and
  // arguments pass through to fn.
  Snapshot.wrap = function wrap(fn) {
    if (typeof fn !== 'function') {
      throw new TypeError('AsyncContext.Snapshot.wrap: argument must be a function');
    }
    var snapshot = new Snapshot();
    return function wrapped() {
      var self = this;
      var args = arguments;
      return snapshot.run(function() { return fn.apply(self, args); });
    };
  };

  // ── Microtask propagation: patch Promise.prototype.then ──────────────────
  // Capture the mapping when the reaction is registered, restore it around
  // the callback. catch/finally call the public then (ECMA-262 §27.2.5.1,
  // §27.2.5.3), and queueMicrotask is Promise.resolve().then(fn) in the DOM
  // shim — all covered by this single patch.
  var originalThen = Promise.prototype.then;

  function bindToCurrentContext(callback) {
    if (typeof callback !== 'function') return callback;
    var captured = currentMapping;
    return function(value) {
      var previous = currentMapping;
      currentMapping = captured;
      try {
        return callback.call(this, value);
      } finally {
        currentMapping = previous;
      }
    };
  }

  Object.defineProperty(Promise.prototype, 'then', {
    value: function then(onFulfilled, onRejected) {
      return originalThen.call(this, bindToCurrentContext(onFulfilled), bindToCurrentContext(onRejected));
    },
    writable: true, enumerable: false, configurable: true
  });

  global.AsyncContext = { Variable: Variable, Snapshot: Snapshot };
})(globalThis);
"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn setup() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            super::install_async_context(&ctx).unwrap();
        });
        (rt, ctx)
    }

    #[test]
    fn async_context_globals_exist() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let v: bool = ctx
                .eval(
                    "typeof AsyncContext === 'object' \
                     && typeof AsyncContext.Variable === 'function' \
                     && typeof AsyncContext.Snapshot === 'function' \
                     && typeof AsyncContext.Snapshot.wrap === 'function'",
                )
                .unwrap();
            assert!(v, "AsyncContext.Variable / Snapshot must be installed");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn variable_run_sets_value_and_restores_default() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var v = new AsyncContext.Variable({ name: 'reqId', defaultValue: 'none' });
          var inside = v.run('r-42', function() { return v.get(); });
          JSON.stringify({ name: v.name, inside: inside, outside: v.get() })
        "#,
                )
                .unwrap();
            assert_eq!(result, r#"{"name":"reqId","inside":"r-42","outside":"none"}"#);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn nested_run_shadows_and_restores() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var a = new AsyncContext.Variable({ defaultValue: 0 });
          var b = new AsyncContext.Variable({ defaultValue: 0 });
          var log = [];
          a.run(1, function() {
            b.run(2, function() {
              log.push(a.get(), b.get());
              a.run(3, function() { log.push(a.get(), b.get()); });
              log.push(a.get());
            });
            log.push(b.get());
          });
          log.push(a.get(), b.get());
          JSON.stringify(log)
        "#,
                )
                .unwrap();
            assert_eq!(result, "[1,2,3,2,1,0,0,0]");
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn run_restores_mapping_on_throw() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var v = new AsyncContext.Variable({ defaultValue: 'def' });
          var caught = '';
          try {
            v.run('inner', function() { throw new Error('boom'); });
          } catch (e) { caught = e.message; }
          JSON.stringify({ caught: caught, after: v.get() })
        "#,
                )
                .unwrap();
            assert_eq!(result, r#"{"caught":"boom","after":"def"}"#);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn snapshot_restores_captured_context() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var v = new AsyncContext.Variable({ defaultValue: 'outer' });
          var snap = v.run('captured', function() { return new AsyncContext.Snapshot(); });
          var viaSnap = snap.run(function() { return v.get(); });
          JSON.stringify({ viaSnap: viaSnap, direct: v.get() })
        "#,
                )
                .unwrap();
            assert_eq!(result, r#"{"viaSnap":"captured","direct":"outer"}"#);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn snapshot_wrap_binds_context_and_passes_args() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            let result: String = ctx
                .eval(
                    r#"
          var v = new AsyncContext.Variable({ defaultValue: 'none' });
          var wrapped = v.run('bound', function() {
            return AsyncContext.Snapshot.wrap(function(x, y) {
              return v.get() + ':' + (x + y);
            });
          });
          JSON.stringify({ out: wrapped(2, 3), after: v.get() })
        "#,
                )
                .unwrap();
            assert_eq!(result, r#"{"out":"bound:5","after":"none"}"#);
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn context_propagates_through_promise_then() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
          globalThis.__observed = [];
          var v = new AsyncContext.Variable({ defaultValue: 'default' });
          v.run('in-chain', function() {
            Promise.resolve(1)
              .then(function(x) { __observed.push(v.get() + ':' + x); return x + 1; })
              .then(function(x) { __observed.push(v.get() + ':' + x); });
          });
          Promise.resolve().then(function() { __observed.push('outside:' + v.get()); });
        "#,
            )
            .unwrap();
            // Drain the microtask queue so the reactions actually run.
            while ctx.execute_pending_job() {}
            let result: String = ctx.eval("JSON.stringify(__observed)").unwrap();
            assert_eq!(
                result,
                r#"["in-chain:1","outside:default","in-chain:2"]"#
            );
        });
        drop(ctx);
        drop(rt);
    }

    #[test]
    fn promise_catch_and_finally_propagate_context() {
        let (rt, ctx) = setup();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
          globalThis.__observed = [];
          var v = new AsyncContext.Variable({ defaultValue: 'default' });
          v.run('ctx', function() {
            Promise.reject(new Error('x'))
              .catch(function() { __observed.push('catch:' + v.get()); })
              .finally(function() { __observed.push('finally:' + v.get()); });
          });
        "#,
            )
            .unwrap();
            while ctx.execute_pending_job() {}
            let result: String = ctx.eval("JSON.stringify(__observed)").unwrap();
            assert_eq!(result, r#"["catch:ctx","finally:ctx"]"#);
        });
        drop(ctx);
        drop(rt);
    }
}
