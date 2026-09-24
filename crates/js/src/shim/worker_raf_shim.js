// requestAnimationFrame / cancelAnimationFrame for DedicatedWorkerGlobalScope
// (HTML LS §8.12 `AnimationFrameProvider` mixin — `Window` and
// `DedicatedWorkerGlobalScope` only, NOT `SharedWorkerGlobalScope` or
// `ServiceWorkerGlobalScope`, so this file is evaluated only from
// `install_worker_globals_v8`, never from `shared_worker.rs`/`sw_worker.rs`).
//
// A worker has no rasterised frame of its own to hook a callback to (the page
// shim's `requestAnimationFrame`, `web_api_shim_mid_b.js`, ties into the
// shell's real paint loop via `_lumen_mark_raf_pending`/
// `_lumen_run_raf_callbacks`, none of which exist here). The only case a
// worker's callback has an actual frame behind it is a transferred
// `OffscreenCanvas` driving its own paint — not wired to any host loop yet.
// Until that lands, this schedules through the same due-time queue
// `setTimeout` already uses (`WORKER_TIMERS_SHIM`, evaluated just before this
// file), at a ~60fps cadence, so a script that merely waits for "the next
// frame" (BUG-959) gets a timestamp-bearing callback instead of a permanent
// `ReferenceError`.
(function() {
  var _seq = 1;
  var _pending = {}; // rafId -> underlying setTimeout id

  globalThis.requestAnimationFrame = function(fn) {
    if (typeof fn !== 'function') return 0;
    var id = _seq++;
    var timeoutId = setTimeout(function() {
      delete _pending[id];
      fn(performance.now());
    }, 16);
    _pending[id] = timeoutId;
    return id;
  };

  globalThis.cancelAnimationFrame = function(id) {
    var timeoutId = _pending[id];
    if (timeoutId !== undefined) {
      clearTimeout(timeoutId);
      delete _pending[id];
    }
  };
})();
