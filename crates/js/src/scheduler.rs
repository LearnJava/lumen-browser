//! Prioritized Task Scheduling (WICG `scheduling-apis`).
//!
//! Exposes `scheduler` (a `Scheduler`), `TaskController : AbortController`,
//! `TaskSignal : AbortSignal` (with `TaskSignal.any()`) and
//! `TaskPriorityChangeEvent` on `globalThis`.
//!
//! **Task queue.** Every `postTask` task and every `yield()` continuation sits
//! in one scheduler queue. Each engine task (a `_lumen_timers` entry, so the
//! microtask checkpoint runs between two of them) takes the pending entry with
//! the best *current* rank — `user-blocking` continuation, `user-blocking`
//! task, `user-visible` continuation, … `background` task — and the oldest of
//! equal rank. The rank is read from the task's priority signal at selection
//! time, so `TaskController.setPriority()` reorders tasks already queued (spec
//! «select the next scheduler task queue»). Before BUG-665 the priority only
//! chose `queueMicrotask` vs. `setTimeout`, once, at enqueue time.
//!
//! **Scheduling state.** A running task's `{abortSource, prioritySource}` is
//! kept in V8's continuation-preserved embedder data (`_lumen_sched_cped_*`
//! natives), which V8 captures into every promise reaction and
//! `queueMicrotask` callback — the mechanism Chromium uses. `scheduler.yield()`
//! therefore inherits the priority and abort signal of the task it is called
//! from, across `await`s, and nothing leaks into an unrelated timer task.
//! `requestIdleCallback` callbacks run with a `background` state
//! (`_lumen_sched_idle_invoke`, called from `web_api_shim_tail.js`).

/// Installs the Scheduler API: the two continuation-data natives, then the
/// shim, which captures them into its closure and deletes the globals.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_scheduler_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.register_native_scoped(
        "_lumen_sched_cped_get",
        Box::new(|scope: &mut v8::PinScope, _args: &v8::FunctionCallbackArguments, rv: &mut v8::ReturnValue| {
            rv.set(scope.get_continuation_preserved_embedder_data());
        }),
    )?;
    rt.register_native_scoped(
        "_lumen_sched_cped_set",
        Box::new(|scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, _rv: &mut v8::ReturnValue| {
            scope.set_continuation_preserved_embedder_data(args.get(0));
        }),
    )?;
    rt.eval(SCHEDULER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SCHEDULER_SHIM: &str = r#"(function() {
  'use strict';

  var PRIORITIES = ['user-blocking', 'user-visible', 'background'];
  var TOKEN = {};

  var cpedGet = globalThis._lumen_sched_cped_get;
  var cpedSet = globalThis._lumen_sched_cped_set;
  delete globalThis._lumen_sched_cped_get;
  delete globalThis._lumen_sched_cped_set;
  if (typeof cpedGet !== 'function' || typeof cpedSet !== 'function') {
    var plainState;
    cpedGet = function() { return plainState; };
    cpedSet = function(v) { plainState = v; };
  }

  function report(e) {
    if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
    else if (typeof _lumen_et_report === 'function') _lumen_et_report(e);
  }

  function checkPriority(p) {
    if (PRIORITIES.indexOf(p) < 0) {
      throw new TypeError("The provided value '" + p + "' is not a valid enum value of type TaskPriority.");
    }
    return p;
  }

  // WebIDL converts a platform object of *any* realm, so a signal created in
  // an iframe must pass even though `instanceof` sees another `AbortSignal`.
  // The brand is the shim's own internal state, which every realm installs.
  function isAbortSignal(v) {
    if (v instanceof AbortSignal) return true;
    return v !== null && typeof v === 'object' && typeof v.aborted === 'boolean'
      && Array.isArray(v._listeners) && typeof v.addEventListener === 'function';
  }

  function isTaskSignal(v) {
    if (v instanceof TaskSignal) return true;
    return isAbortSignal(v) && typeof v._priority === 'string' && Array.isArray(v._priorityDependents);
  }

  function defineInterface(name, value) {
    Object.defineProperty(globalThis, name, { value: value, writable: true, enumerable: false, configurable: true });
  }

  // ── TaskPriorityChangeEvent ───────────────────────────────────────────────

  function TaskPriorityChangeEvent(type, init) {
    if (!(this instanceof TaskPriorityChangeEvent)) {
      throw new TypeError("Failed to construct 'TaskPriorityChangeEvent': Please use the 'new' operator");
    }
    if (arguments.length < 2 || init == null || init.previousPriority === undefined) {
      throw new TypeError("Failed to construct 'TaskPriorityChangeEvent': required member previousPriority is undefined.");
    }
    var prev = checkPriority(String(init.previousPriority));
    Event.call(this, type, init);
    Object.defineProperty(this, '_previousPriority', { value: prev });
  }
  TaskPriorityChangeEvent.prototype = Object.create(Event.prototype, {
    constructor: { value: TaskPriorityChangeEvent, writable: true, configurable: true }
  });
  Object.defineProperty(TaskPriorityChangeEvent.prototype, 'previousPriority', {
    configurable: true, enumerable: true,
    get: function() { return this._previousPriority; }
  });
  defineInterface('TaskPriorityChangeEvent', TaskPriorityChangeEvent);

  // ── TaskSignal : AbortSignal ──────────────────────────────────────────────

  function TaskSignal(token, priority) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor');
    AbortSignal.call(this);
    this.onprioritychange = null;
    Object.defineProperties(this, {
      _priority:           { value: priority, writable: true },
      _priorityChanging:   { value: false, writable: true },
      // The signal this one follows for priority (`TaskSignal.any`), and the
      // signals following this one — spec «dependent signals».
      _prioritySource:     { value: null, writable: true },
      _priorityDependents: { value: [], writable: true },
      _pcListeners:        { value: [], writable: true }
    });
  }
  TaskSignal.prototype = Object.create(AbortSignal.prototype, {
    constructor: { value: TaskSignal, writable: true, configurable: true }
  });
  Object.setPrototypeOf(TaskSignal, AbortSignal);

  Object.defineProperty(TaskSignal.prototype, 'priority', {
    configurable: true, enumerable: true,
    get: function() { return this._priority; }
  });

  TaskSignal.prototype.addEventListener = function(type, listener, options) {
    if (String(type) !== 'prioritychange') {
      return AbortSignal.prototype.addEventListener.call(this, type, listener, options);
    }
    if (listener == null || this._pcListeners.indexOf(listener) >= 0) return;
    this._pcListeners.push(listener);
  };

  TaskSignal.prototype.removeEventListener = function(type, listener, options) {
    if (String(type) !== 'prioritychange') {
      return AbortSignal.prototype.removeEventListener.call(this, type, listener, options);
    }
    var i = this._pcListeners.indexOf(listener);
    if (i >= 0) this._pcListeners.splice(i, 1);
  };

  function invokeListener(l, target, evt) {
    try {
      if (typeof l === 'function') l.call(target, evt);
      else if (l && typeof l.handleEvent === 'function') l.handleEvent(evt);
    } catch (e) { report(e); }
  }

  function firePriorityChange(sig, prev) {
    var evt = new TaskPriorityChangeEvent('prioritychange', { previousPriority: prev });
    evt.target = sig;
    evt.currentTarget = sig;
    evt.eventPhase = 2;
    if (typeof sig.onprioritychange === 'function') invokeListener(sig.onprioritychange, sig, evt);
    var ls = sig._pcListeners.slice();
    for (var i = 0; i < ls.length && !evt._stopImmediate; i++) invokeListener(ls[i], sig, evt);
    evt.currentTarget = null;
    evt.eventPhase = 0;
  }

  // Spec «signal priority change». Dependents are snapshotted before the
  // event, so one created by a `prioritychange` listener is not notified of
  // the change it was created after.
  function signalPriorityChange(sig, priority) {
    if (sig._priorityChanging) {
      throw new DOMException('Cannot change the priority of a TaskSignal while it is changing.', 'NotAllowedError');
    }
    if (sig._priority === priority) return;
    sig._priorityChanging = true;
    var prev = sig._priority;
    sig._priority = priority;
    var deps = sig._priorityDependents.slice();
    try {
      firePriorityChange(sig, prev);
      for (var i = 0; i < deps.length; i++) signalPriorityChange(deps[i], priority);
    } finally {
      sig._priorityChanging = false;
    }
  }

  function toSignalList(signals) {
    if (signals == null || typeof signals[Symbol.iterator] !== 'function') {
      throw new TypeError("Failed to execute 'any' on 'TaskSignal': The provided value cannot be converted to a sequence.");
    }
    var list = Array.from(signals);
    for (var i = 0; i < list.length; i++) {
      if (!isAbortSignal(list[i])) {
        throw new TypeError("Failed to execute 'any' on 'TaskSignal': Failed to convert value to 'AbortSignal'.");
      }
    }
    return list;
  }

  // Spec «create a dependent task signal»: abort follows `signals` (the DOM
  // algorithm in abort_shim.js); priority is either fixed (a string) or
  // follows a TaskSignal — flattened to that signal's own source, so every
  // dependent hangs directly off a non-dependent signal.
  TaskSignal.any = function any(signals, init) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'any' on 'TaskSignal': 1 argument required, but only 0 present.");
    }
    var list = toSignalList(signals);
    var p = (init != null && init.priority !== undefined) ? init.priority : 'user-visible';
    var source = null;
    if (isTaskSignal(p)) source = p._prioritySource || p;
    else p = checkPriority(String(p));
    var result = new TaskSignal(TOKEN, source ? source._priority : p);
    if (source) {
      result._prioritySource = source;
      source._priorityDependents.push(result);
    }
    return _lumen_abort_signal_make_dependent(result, list);
  };

  defineInterface('TaskSignal', TaskSignal);

  // One never-changing signal per priority: the priority source of a task
  // posted with an explicit `priority` (spec: a fixed-priority TaskSignal).
  var FIXED = {};
  for (var fi = 0; fi < PRIORITIES.length; fi++) FIXED[PRIORITIES[fi]] = new TaskSignal(TOKEN, PRIORITIES[fi]);

  // ── TaskController : AbortController ─────────────────────────────────────

  function TaskController(init) {
    if (!(this instanceof TaskController)) {
      throw new TypeError("Failed to construct 'TaskController': Please use the 'new' operator");
    }
    var priority = (init != null && init.priority !== undefined) ? checkPriority(String(init.priority)) : 'user-visible';
    this.signal = new TaskSignal(TOKEN, priority);
  }
  TaskController.prototype = Object.create(AbortController.prototype, {
    constructor: { value: TaskController, writable: true, configurable: true }
  });
  Object.setPrototypeOf(TaskController, AbortController);

  TaskController.prototype.setPriority = function setPriority(priority) {
    if (arguments.length < 1) {
      throw new TypeError("Failed to execute 'setPriority' on 'TaskController': 1 argument required, but only 0 present.");
    }
    signalPriorityChange(this.signal, checkPriority(String(priority)));
  };

  defineInterface('TaskController', TaskController);

  // ── Scheduler task queue ─────────────────────────────────────────────────

  var queue = [];          // pending { prio: TaskSignal, continuation, run } in enqueue order
  var pumpArmed = false;

  function rank(t) {
    return PRIORITIES.indexOf(t.prio._priority) * 2 + (t.continuation ? 0 : 1);
  }

  // An engine task, like `_perf_queue_task`: straight into `_lumen_timers`
  // with nesting 0 so the §8.6 timer clamp never applies to it.
  function queueEngineTask(fn) {
    if (typeof _lumen_timers !== 'undefined' && _lumen_timers && typeof _lumen_timer_seq === 'number') {
      var deadline = (typeof _lumen_now_ms === 'function') ? _lumen_now_ms() : 0;
      _lumen_timers.push({ id: _lumen_timer_seq++, fn: fn, deadline: deadline, interval: null, nesting: 0 });
      if (typeof _lumen_request_wakeup === 'function') _lumen_request_wakeup(deadline);
      return;
    }
    if (typeof setTimeout === 'function') { setTimeout(fn, 0); return; }
    Promise.resolve().then(fn);
  }

  function armPump() {
    if (pumpArmed || queue.length === 0) return;
    pumpArmed = true;
    queueEngineTask(pump);
  }

  // Runs exactly one scheduler task, so its promise reactions (a `yield()`
  // continuation's `await`) settle before the next selection.
  function pump() {
    pumpArmed = false;
    var best = -1, bestRank = Infinity;
    for (var i = 0; i < queue.length; i++) {
      var r = rank(queue[i]);
      if (r < bestRank) { best = i; bestRank = r; }
    }
    if (best < 0) return;
    var task = queue.splice(best, 1)[0];
    armPump();
    task.run();
  }

  function enqueue(task) {
    queue.push(task);
    armPump();
  }

  function dequeue(task) {
    var i = queue.indexOf(task);
    if (i >= 0) queue.splice(i, 1);
  }

  // WebIDL `unsigned long long` for `delay`.
  function toDelay(v) {
    var n = Number(v);
    if (!isFinite(n) || n <= 0) return 0;
    return Math.floor(n);
  }

  // ── Scheduler ────────────────────────────────────────────────────────────

  function Scheduler() { throw new TypeError('Illegal constructor'); }

  function postTask(callback, options) {
    if (typeof callback !== 'function') {
      return Promise.reject(new TypeError("Failed to execute 'postTask' on 'Scheduler': parameter 1 is not of type 'Function'."));
    }
    var signal = null, priority, delay = 0;
    try {
      if (options != null) {
        if (options.signal != null) {
          if (!isAbortSignal(options.signal)) {
            throw new TypeError("Failed to execute 'postTask' on 'Scheduler': member signal is not of type AbortSignal.");
          }
          signal = options.signal;
        }
        if (options.priority !== undefined) priority = checkPriority(String(options.priority));
        if (options.delay !== undefined) delay = toDelay(options.delay);
      }
    } catch (e) {
      return Promise.reject(e);
    }
    if (signal && signal.aborted) return Promise.reject(signal.reason);

    // An explicit priority wins over the signal's; the signal still aborts.
    var prio = priority !== undefined ? FIXED[priority]
             : (isTaskSignal(signal) ? signal : FIXED['user-visible']);
    var state = { abortSource: signal, prioritySource: prio };

    return new Promise(function(resolve, reject) {
      var task = { prio: prio, continuation: false, run: null };
      var delayTimer = null;
      var onAbort = null;
      if (signal) {
        onAbort = function() {
          dequeue(task);
          if (delayTimer !== null) { clearTimeout(delayTimer); delayTimer = null; }
          reject(signal.reason);
        };
        signal.addEventListener('abort', onAbort);
      }
      // The abort listener stays attached while the callback runs: an abort
      // from inside it rejects the promise before the return value would
      // resolve it.
      task.run = function() {
        var outer = cpedGet();
        cpedSet(state);
        try { resolve(callback()); } catch (e) { reject(e); }
        finally {
          cpedSet(outer);
          if (onAbort) signal.removeEventListener('abort', onAbort);
        }
      };
      if (delay > 0) {
        delayTimer = setTimeout(function() { delayTimer = null; enqueue(task); }, delay);
      } else {
        enqueue(task);
      }
    });
  }

  // The continuation inherits the current scheduling state: the priority and
  // abort signal of the task this runs in, or `user-visible` outside one.
  function yieldNow() {
    var state = cpedGet();
    var abortSource = (state && state.abortSource) || null;
    var prio = (state && state.prioritySource) || FIXED['user-visible'];
    if (abortSource && abortSource.aborted) return Promise.reject(abortSource.reason);
    return new Promise(function(resolve, reject) {
      var task = { prio: prio, continuation: true, run: null };
      var onAbort = null;
      if (abortSource) {
        onAbort = function() { dequeue(task); reject(abortSource.reason); };
        abortSource.addEventListener('abort', onAbort);
      }
      task.run = function() {
        if (onAbort) abortSource.removeEventListener('abort', onAbort);
        resolve();
      };
      enqueue(task);
    });
  }

  var methods = {
    postTask: function postTask_(callback) { return postTask(callback, arguments[1]); },
    yield: function() { return yieldNow(); }
  };
  Object.defineProperty(methods.postTask, 'name', { value: 'postTask' });
  Object.defineProperty(methods.yield, 'name', { value: 'yield' });
  Object.defineProperty(Scheduler.prototype, 'postTask', { value: methods.postTask, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(Scheduler.prototype, 'yield', { value: methods.yield, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(Scheduler, 'prototype', { writable: false });
  defineInterface('Scheduler', Scheduler);

  // `[Replaceable] readonly attribute Scheduler scheduler` — a plain writable
  // global, so `scheduler = x` in strict code does not throw.
  globalThis.scheduler = Object.create(Scheduler.prototype);

  // HTML «invoke idle callbacks»: they run with a background scheduling state,
  // so a `yield()` inside one continues at background priority.
  var IDLE_STATE = { abortSource: null, prioritySource: FIXED['background'] };
  Object.defineProperty(globalThis, '_lumen_sched_idle_invoke', {
    value: function(fn, deadline) {
      var outer = cpedGet();
      cpedSet(IDLE_STATE);
      try { fn(deadline); } finally { cpedSet(outer); }
    },
    writable: false, enumerable: false, configurable: false
  });
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    /// A page runtime: `install_dom` installs the scheduler module on top of
    /// `AbortSignal`/`Event`/`_lumen_timers`, which the shim builds on.
    fn page() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt
    }

    /// Runs `setup`, then drives the event loop: each `_lumen_tick_timers()`
    /// is its own script, so the microtask checkpoint runs between ticks the
    /// way it does between two shell ticks. A scheduler task takes one tick.
    fn run(setup: &str, ticks: usize, check: &str) -> JsValue {
        let rt = page();
        rt.eval(setup).unwrap();
        for _ in 0..ticks {
            rt.eval("_lumen_tick_timers()").unwrap();
        }
        rt.eval(check).unwrap()
    }

    fn assert_str(v: JsValue, expected: &str) {
        assert_eq!(v, JsValue::String(expected.into()));
    }

    #[test]
    fn interfaces_inherit_from_abort_signal_and_controller() {
        let v = run(
            r#"var c = new TaskController({ priority: 'background' });
               var ctorThrows = false;
               try { new TaskSignal(); } catch (e) { ctorThrows = e instanceof TypeError; }"#,
            0,
            r#"c instanceof AbortController && c.signal instanceof AbortSignal
               && c.signal instanceof TaskSignal && c.signal.priority === 'background'
               && scheduler instanceof Scheduler && typeof TaskSignal.any === 'function'
               && ctorThrows"#,
        );
        assert_eq!(v, JsValue::Bool(true));
    }

    #[test]
    fn tasks_run_in_priority_order() {
        let v = run(
            r#"var order = [];
               [['B1','background'],['B2','background'],['UV1','user-visible'],
                ['UV2','user-visible'],['UB1','user-blocking'],['UB2','user-blocking']]
                 .forEach(function(p) { scheduler.postTask(function() { order.push(p[0]); }, { priority: p[1] }); });"#,
            8,
            "order.join()",
        );
        assert_str(v, "UB1,UB2,UV1,UV2,B1,B2");
    }

    /// BUG-665 §2: `setPriority` must move tasks that are already queued.
    #[test]
    fn set_priority_reorders_queued_tasks() {
        let v = run(
            r#"var order = [];
               var c = new TaskController();
               for (var i = 0; i < 5; i++) (function(i) {
                 scheduler.postTask(function() { order.push(i); }, { signal: c.signal });
               })(i);
               scheduler.postTask(function() { order.push(5); }, { priority: 'user-blocking' });
               scheduler.postTask(function() { order.push(6); }, { priority: 'user-visible' });
               c.setPriority('background');"#,
            9,
            "order.join()",
        );
        assert_str(v, "5,6,0,1,2,3,4");
    }

    /// BUG-665 §4: a real `TaskPriorityChangeEvent` whose `target` is the
    /// signal; a nested `setPriority` throws `NotAllowedError`.
    #[test]
    fn prioritychange_event_has_target_and_recursion_throws() {
        let v = run(
            r#"var c = new TaskController();
               var seen = '';
               c.signal.onprioritychange = function(e) {
                 var nested = '';
                 try { c.setPriority('user-blocking'); } catch (x) { nested = x.name; }
                 seen = [e instanceof TaskPriorityChangeEvent, e.type, e.target === c.signal,
                         e.target.priority, e.previousPriority, nested].join();
               };
               c.setPriority('background');"#,
            0,
            "seen",
        );
        assert_str(v, "true,prioritychange,true,background,user-visible,NotAllowedError");
    }

    /// BUG-665 §1: `TaskSignal.any` follows a priority source (flattened, so
    /// dependents are notified in creation order) and aborts only from its
    /// abort sources.
    #[test]
    fn task_signal_any_follows_priority_and_abort() {
        let v = run(
            r#"var tc = new TaskController();
               var ac = new AbortController();
               var events = [];
               var first = [];
               for (var i = 0; i < 3; i++) (function(i) {
                 var s = TaskSignal.any([], { priority: tc.signal });
                 s.addEventListener('prioritychange', function() { events.push(i); });
                 first.push(s);
               })(i);
               for (var j = 0; j < 3; j++) (function(j) {
                 var s = TaskSignal.any([], { priority: first[j] });
                 s.addEventListener('prioritychange', function() { events.push(3 + j); });
               })(j);
               var combined = TaskSignal.any([ac.signal], { priority: tc.signal });
               tc.setPriority('background');
               tc.abort();
               var afterTc = combined.aborted;
               ac.abort('why');"#,
            0,
            "[events.join(''), combined.priority, afterTc, combined.aborted, combined.reason].join()",
        );
        assert_str(v, "012345,background,false,true,why");
    }

    /// DOM §3.2: dependents are marked aborted before any event fires and get
    /// their events after the source, in creation order.
    #[test]
    fn abort_signal_any_fires_in_creation_order() {
        let v = run(
            r#"var c = new AbortController();
               var s = [c.signal];
               s.push(AbortSignal.any([c.signal]));
               s.push(AbortSignal.any([c.signal]));
               s.push(AbortSignal.any([s[0]]));
               s.push(AbortSignal.any([s[1]]));
               var out = '';
               var allMarked = true;
               s.forEach(function(sig, i) {
                 sig.addEventListener('abort', function() {
                   out += i;
                   if (!s[4].aborted) allMarked = false;
                 });
               });
               c.abort();"#,
            0,
            "out + ',' + allMarked",
        );
        assert_str(v, "01234,true");
    }

    /// BUG-665 §5: an abort from inside the running callback rejects the task.
    #[test]
    fn abort_inside_sync_callback_rejects() {
        let v = run(
            r#"var c = new TaskController();
               var result = 'pending';
               scheduler.postTask(function() { c.abort(); return 1; }, { signal: c.signal })
                 .then(function() { result = 'resolved'; }, function(e) { result = e.name; });"#,
            2,
            "result",
        );
        assert_str(v, "AbortError");
    }

    /// A signal from another realm (an iframe's) is accepted: `instanceof`
    /// fails across realms, the WebIDL conversion does not. The stand-in has
    /// the shim's methods but not this realm's `AbortSignal.prototype`.
    #[test]
    fn signal_from_another_realm_is_accepted() {
        let v = run(
            r#"function foreign(proto) {
                 var s = new AbortController().signal;
                 var f = Object.create(Object.assign({}, AbortSignal.prototype, proto || {}));
                 Object.getOwnPropertyNames(s).forEach(function(k) { f[k] = s[k]; });
                 return f;
               }
               var out = [];
               var sig = foreign();
               var brand = !(sig instanceof AbortSignal);
               scheduler.postTask(function() { out.push('ran'); }, { signal: sig })
                 .then(function() { out.push('ok'); }, function(e) { out.push(e.name); });
               var any = TaskSignal.any([sig]);"#,
            2,
            "[brand, out.join('+'), any instanceof TaskSignal].join()",
        );
        assert_str(v, "true,ran+ok,true");
    }

    /// BUG-665 §3: `yield()` inherits the running task's priority across
    /// `await` (continuation-preserved embedder data) and its continuation
    /// outranks ordinary tasks of the same priority.
    #[test]
    fn yield_inherits_priority_across_await() {
        for (priority, expected) in [
            ("user-visible", "ub1,ub2,y0,y1,y2,y3,uv1,uv2,bg1,bg2"),
            ("user-blocking", "y0,y1,y2,y3,ub1,ub2,uv1,uv2,bg1,bg2"),
            ("background", "ub1,ub2,uv1,uv2,y0,y1,y2,y3,bg1,bg2"),
        ] {
            let v = run(
                &format!(
                    r#"var ids = [];
                       scheduler.postTask(async function() {{
                         ids.push('y0');
                         for (var i = 1; i < 4; i++) {{ await scheduler.yield(); ids.push('y' + i); }}
                       }}, {{ priority: '{priority}' }});
                       [['ub1','user-blocking'],['ub2','user-blocking'],['uv1','user-visible'],
                        ['uv2','user-visible'],['bg1','background'],['bg2','background']]
                         .forEach(function(p) {{ scheduler.postTask(function() {{ ids.push(p[0]); }}, {{ priority: p[1] }}); }});"#
                ),
                14,
                "ids.join()",
            );
            assert_str(v, expected);
        }
    }

    /// `yield()` rejects once the inherited abort signal is aborted — both
    /// when it already is at the call and when it aborts while yielded.
    #[test]
    fn yield_rejects_with_inherited_abort() {
        let v = run(
            r#"var out = [];
               var c1 = new TaskController();
               scheduler.postTask(async function() {
                 c1.abort();
                 try { await scheduler.yield(); out.push('no'); } catch (e) { out.push('a:' + e.name); }
               }, { signal: c1.signal }).catch(function() {});
               var c2 = new AbortController();
               scheduler.postTask(async function() {
                 scheduler.postTask(function() { c2.abort(); }, { priority: 'user-blocking' });
                 try { await scheduler.yield(); out.push('no'); } catch (e) { out.push('b:' + e.name); }
               }, { signal: c2.signal });"#,
            6,
            "out.sort().join()",
        );
        assert_str(v, "a:AbortError,b:AbortError");
    }

    /// The state of a background task does not leak into a timer it queued:
    /// the timer's `yield()` continuation is `user-visible` and beats a
    /// `user-visible` task.
    #[test]
    fn scheduling_state_does_not_leak_into_timers() {
        let v = run(
            r#"var ids = [];
               scheduler.postTask(function() {
                 setTimeout(async function() {
                   scheduler.postTask(function() { ids.push('task'); }, { priority: 'user-visible' });
                   await scheduler.yield();
                   ids.push('continuation');
                 });
               }, { priority: 'background' });"#,
            6,
            "ids.join()",
        );
        assert_str(v, "continuation,task");
    }
}
