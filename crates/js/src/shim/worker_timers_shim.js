(function() {
  // {id, fn, args, due (epoch ms), delay, interval (bool), nesting, seq}
  var _timers = [];
  var _micro = [];
  var _nextId = 1;
  // HTML LS §8.6 "timer nesting level" of the task currently running, so a
  // timer armed from inside a callback inherits its parent's depth.
  var _nesting = 0;

  function _report(e) {
    var r = globalThis._lumen_worker_exception_reporter;
    if (typeof r === 'function') { try { r(e); } catch (_e) {} }
  }

  // WebIDL `long`: ToInt32 (so 2^32 becomes 0, matching every other engine),
  // then HTML LS §8.6 step 5 — a negative timeout becomes 0.
  function _toDelay(v) {
    var n = Number(v) | 0;
    return n < 0 ? 0 : n;
  }

  function _clamp(delay, nesting) {
    return (nesting > 5 && delay < 4) ? 4 : delay;
  }

  // HTML LS §8.6: a non-Function handler is a string run as a classic script
  // when the timer fires, recompiled on every firing of an interval (BUG-831).
  // Indirect eval is what makes it global-scope: a direct call would evaluate
  // the code inside this closure. A string handler takes no trailing
  // arguments (§8.6 step 8 hands them to a Function handler only), so the
  // list the dispatch loop `apply`s is emptied here.
  function _stringHandler(code) {
    var src = String(code);
    return function () { (0, eval)(src); };
  }

  function _schedule(fn, delay, args, repeating) {
    if (typeof fn !== 'function') { fn = _stringHandler(fn); args = []; }
    var nesting = _nesting + 1;
    var d = _toDelay(delay);
    var id = _nextId++;
    _timers.push({
      id: id, fn: fn, args: args, delay: d, interval: repeating,
      nesting: nesting, due: Date.now() + _clamp(d, nesting), seq: id,
    });
    return id;
  }

  globalThis.setTimeout = function(fn, delay) {
    return _schedule(fn, delay, Array.prototype.slice.call(arguments, 2), false);
  };
  globalThis.setInterval = function(fn, delay) {
    return _schedule(fn, delay, Array.prototype.slice.call(arguments, 2), true);
  };
  // The handle is a WebIDL `long` exactly as the delay is, so
  // `clearTimeout(String(id))` has to cancel timer `id`: the strict `===`
  // against a raw argument never matched one (BUG-847, the same conversion
  // the page shim was missing on both arguments).
  globalThis.clearTimeout = function(id) {
    var handle = Number(id) | 0;
    for (var i = 0; i < _timers.length; i++) {
      if (_timers[i].id === handle) { _timers.splice(i, 1); return; }
    }
  };
  // One list, one id space — as the spec's single "map of active timers" has.
  globalThis.clearInterval = globalThis.clearTimeout;

  globalThis.queueMicrotask = function(fn) {
    if (typeof fn !== 'function') {
      throw new TypeError('queueMicrotask: callback is not a function');
    }
    _micro.push(fn);
  };

  function _drainMicrotasks() {
    while (_micro.length) {
      var fn = _micro.shift();
      try { fn(); } catch (e) { _report(e); }
    }
  }

  // Index of the due timer that should run next: earliest deadline, ties
  // broken by insertion order (`seq`, re-stamped on every repeat so a
  // reloaded interval queues behind whatever was already waiting).
  function _nextDue(now) {
    var best = -1;
    for (var i = 0; i < _timers.length; i++) {
      var t = _timers[i];
      if (t.due > now) continue;
      if (best === -1 || t.due < _timers[best].due
          || (t.due === _timers[best].due && t.seq < _timers[best].seq)) {
        best = i;
      }
    }
    return best;
  }

  // Run ONE timer that was already due at `limit` (the turn's start time),
  // with the shim's own microtasks drained around it; true if one ran. One
  // per call because V8 runs promise reactions only when the outermost
  // `eval` returns: a Rust call per task is what puts the HTML LS §8.1.7.3
  // microtask checkpoint between two timers, and what lets a timer armed from
  // a `.then()` reach `_lumen_worker_next_wait` at all (WORKER-1 срез 2 — a
  // `WritableStream` whose sink awaits `setTimeout` stalled after one write).
  // `limit` bounds the turn, so a self-rearming zero-delay timer cannot keep
  // the thread from ever reading its message channel.
  globalThis._lumen_worker_run_one_task = function(limit) {
    _drainMicrotasks();
    if (globalThis._lumen_worker_closed === true) return false;
    var i = _nextDue(Math.min(Date.now(), limit));
    if (i === -1) return false;
    var task = _timers[i];
    var now = Date.now();
    if (task.interval) {
      // HTML LS §8.6 step 12: a repeating timer re-runs the initialization
      // steps with the nesting level incremented — which is what puts the
      // 4 ms floor under a zero-delay interval after a few cycles.
      task.nesting += 1;
      task.due = now + _clamp(task.delay, task.nesting);
      task.seq = _nextId++;
    } else {
      _timers.splice(i, 1);
    }
    var outer = _nesting;
    _nesting = task.nesting;
    try { task.fn.apply(globalThis, task.args); } catch (e) { _report(e); }
    _nesting = outer;
    _drainMicrotasks();
    return true;
  };

  // How long the thread may sleep: milliseconds until the next deadline, or
  // -1 when nothing is pending. Asked in an `eval` of its own, after the one
  // that ran the tasks, so the promise reactions those tasks queued have
  // already run and armed whatever timers they arm.
  globalThis._lumen_worker_next_wait = function() {
    if (_micro.length) return 0;
    if (!_timers.length) return -1;
    var soonest = Infinity;
    for (var j = 0; j < _timers.length; j++) {
      if (_timers[j].due < soonest) soonest = _timers[j].due;
    }
    var wait = soonest - Date.now();
    return wait > 0 ? wait : 0;
  };

  // Everything due in one call — for the message-loop eval strings below,
  // which already run inside a larger eval. The task loop itself goes through
  // `run_worker_tasks` on the Rust side instead.
  globalThis._lumen_worker_run_tasks = function() {
    var limit = Date.now();
    while (globalThis._lumen_worker_run_one_task(limit)) {}
    return globalThis._lumen_worker_next_wait();
  };

  // The name the message-loop eval strings have called since before the queue
  // had deadlines; kept so the dispatch path still flushes what is due.
  globalThis._lumen_flush_timers = function() { globalThis._lumen_worker_run_tasks(); };
})();
