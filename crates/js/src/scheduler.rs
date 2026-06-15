//! W3C Scheduler API Level 1.
//!
//! Exposes `scheduler` on `globalThis`:
//! - `scheduler.postTask(callback, {priority?, delay?, signal?})` → `Promise`
//! - `scheduler.yield()` → `Promise`
//!
//! Task priorities: `'user-blocking'` | `'user-visible'` | `'background'`.
//!
//! Also exposes `TaskController` and `TaskSignal`:
//! - `new TaskController({priority?})` → controller with `.signal: TaskSignal`
//! - `controller.setPriority(p)` → fires `prioritychange` on signal
//! - `controller.abort()` → aborts signal, pending postTask rejects with AbortError
//!
//! **Phase 0**: scheduling deferred via `queueMicrotask` (user-blocking) or
//! `setTimeout` (user-visible / background). No integration with browser rendering.

use rquickjs::Ctx;

/// Install the Scheduler API, TaskController, and TaskSignal into the JS context.
pub fn install_scheduler_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SCHEDULER_SHIM)?;
    Ok(())
}

const SCHEDULER_SHIM: &str = r#"(function() {
  'use strict';

  var VALID_PRIORITIES = ['user-blocking', 'user-visible', 'background'];

  // ── TaskSignal ────────────────────────────────────────────────────────────

  function TaskSignal(priority) {
    this._priority = priority;
    this.aborted   = false;
    this.reason    = undefined;
    this.onprioritychange = null;
    this._pcListeners    = [];
    this._abortListeners = [];
  }

  Object.defineProperty(TaskSignal.prototype, 'priority', {
    configurable: true,
    enumerable:   true,
    get: function() { return this._priority; }
  });

  TaskSignal.prototype.addEventListener = function(type, listener) {
    if (type === 'prioritychange') this._pcListeners.push(listener);
    else if (type === 'abort')     this._abortListeners.push(listener);
  };

  TaskSignal.prototype.removeEventListener = function(type, listener) {
    if (type === 'prioritychange') {
      this._pcListeners = this._pcListeners.filter(function(l) { return l !== listener; });
    } else if (type === 'abort') {
      this._abortListeners = this._abortListeners.filter(function(l) { return l !== listener; });
    }
  };

  // Called by TaskController.setPriority; fires synchronously (spec §5.2.1).
  TaskSignal.prototype._setPriority = function(newPriority) {
    var prev = this._priority;
    this._priority = newPriority;
    var evt = { type: 'prioritychange', previousPriority: prev };
    if (typeof this.onprioritychange === 'function') {
      try { this.onprioritychange(evt); } catch(e) {}
    }
    var ls = this._pcListeners.slice();
    for (var i = 0; i < ls.length; i++) { try { ls[i](evt); } catch(e) {} }
  };

  TaskSignal.prototype._abort = function(reason) {
    if (this.aborted) return;
    this.aborted = true;
    this.reason  = (reason !== undefined)
      ? reason
      : new DOMException('TaskSignal aborted', 'AbortError');
    var evt = { type: 'abort' };
    var ls = this._abortListeners.slice();
    for (var i = 0; i < ls.length; i++) { try { ls[i](evt); } catch(e) {} }
  };

  globalThis.TaskSignal = TaskSignal;

  // ── TaskController ────────────────────────────────────────────────────────

  function TaskController(opts) {
    var priority = (opts && opts.priority != null) ? opts.priority : 'user-visible';
    if (VALID_PRIORITIES.indexOf(priority) < 0) {
      throw new TypeError('Invalid task priority: ' + priority);
    }
    this.signal = new TaskSignal(priority);
  }

  TaskController.prototype.setPriority = function(priority) {
    if (VALID_PRIORITIES.indexOf(priority) < 0) {
      throw new TypeError('Invalid task priority: ' + priority);
    }
    this.signal._setPriority(priority);
  };

  TaskController.prototype.abort = function(reason) {
    this.signal._abort(reason);
  };

  globalThis.TaskController = TaskController;

  // ── scheduler ─────────────────────────────────────────────────────────────

  var _scheduler = {
    postTask: function(callback, opts) {
      var priority = (opts && opts.priority != null) ? opts.priority : 'user-visible';
      var delay    = (opts && opts.delay    != null) ? opts.delay    : 0;
      var signal   = (opts && opts.signal   != null) ? opts.signal   : null;

      if (VALID_PRIORITIES.indexOf(priority) < 0) {
        return Promise.reject(new TypeError('Unknown task priority: ' + priority));
      }

      // Synchronously reject if signal is already aborted (spec §3.2 step 3).
      if (signal && signal.aborted) {
        return Promise.reject(signal.reason);
      }

      return new Promise(function(resolve, reject) {
        var abortHandler = null;
        if (signal) {
          abortHandler = function() { reject(signal.reason); };
          signal.addEventListener('abort', abortHandler);
        }

        var run = function() {
          if (signal && abortHandler) signal.removeEventListener('abort', abortHandler);
          if (signal && signal.aborted) { reject(signal.reason); return; }
          try { resolve(callback()); } catch(e) { reject(e); }
        };

        // Phase 0 scheduling: delay takes precedence over priority-based strategy.
        if (delay > 0) {
          setTimeout(run, delay);
        } else if (priority === 'user-blocking') {
          // Highest priority: enqueue as microtask (spec §3.3.1).
          queueMicrotask(run);
        } else if (priority === 'background') {
          // Lowest priority: longer deferral (spec §3.3.3).
          setTimeout(run, 200);
        } else {
          // user-visible (default): next task checkpoint.
          setTimeout(run, 0);
        }
      });
    },

    // Yield to browser — continuation queued at user-visible priority (spec §8.5).
    yield: function() {
      return new Promise(function(resolve) { setTimeout(resolve, 0); });
    }
  };

  globalThis.scheduler = _scheduler;
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
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg); e.name = name || 'Error'; return e;
                }
                globalThis.DOMException = DOMException;
            }
            if (typeof queueMicrotask === 'undefined') {
                globalThis.queueMicrotask = function(fn) { Promise.resolve().then(fn); };
            }
            "#,
        )
        .unwrap();
        install_scheduler_api(ctx).unwrap();
    }

    #[test]
    fn scheduler_and_classes_exist() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof scheduler === 'object'
                      && typeof scheduler.postTask === 'function'
                      && typeof scheduler.yield === 'function'
                      && typeof TaskController === 'function'
                      && typeof TaskSignal === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn post_task_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("scheduler.postTask(function(){ return 1; }) instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn scheduler_yield_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx.eval("scheduler.yield() instanceof Promise").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn task_controller_signal_initial_state() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var c = new TaskController({ priority: 'user-blocking' });
                    c.signal instanceof TaskSignal
                      && c.signal.priority === 'user-blocking'
                      && c.signal.aborted === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn task_signal_set_priority_fires_prioritychange_synchronously() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var c = new TaskController({ priority: 'user-visible' });
                    var fired = false;
                    var prevSeen = null;
                    c.signal.addEventListener('prioritychange', function(e) {
                      fired = true;
                      prevSeen = e.previousPriority;
                    });
                    c.setPriority('background');
                    fired === true
                      && prevSeen === 'user-visible'
                      && c.signal.priority === 'background'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
