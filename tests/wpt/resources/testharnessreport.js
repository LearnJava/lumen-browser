// Lumen's `testharnessreport.js` (P2-wpt S4, `docs/tasks/p2-wpt-integration.md`).
//
// Upstream WPT normally pairs `testharness.js` with a browser-specific
// `testharnessreport.js` that ships results back to the runner over whatever
// channel that runner speaks (a `postMessage`, a WebSocket, a magic global
// polled via `execute_script`, ...). `LumenTestharnessExecutor.do_test`
// (`tools/wptrunner/wptrunner/executors/executorlumen.py`) polls this page
// for results over WebDriver BiDi `script.evaluate`, so the contract here is
// simply: on harness completion, serialize the standard
// `[url, harness_status, harness_message, harness_stack, subtests]` shape
// (same field order `TestharnessResultConverter` in `executors/base.py`
// expects) to JSON and stash it on a window global. `script.evaluate`
// returns primitives as a real BiDi `RemoteValue` (`protocol.rs`), so a JSON
// *string* takes that path rather than the objects/arrays JSON-text
// fallback — verified against a live BiDi response, not assumed.
(function() {
  // Tells `/resources/testdriver.js` (WPT-RUN-2, unmodified vendored file,
  // always served concatenated with wptrunner's own
  // `executors/message-queue.js` + `testdriver-extra.js` —
  // `environment.py::get_routes`) this is the "main" test browsing context,
  // so `test_driver_internal.*` calls queue actions onto
  // `window.__wptrunner_message_queue` (drained by
  // `LumenTestharnessExecutor`, `executorlumen.py`) instead of throwing
  // ("Tried to run in a non-testharness window without a call to
  // set_test_context"). Stock wptrunner's own `testharnessreport.js` (never
  // served here — this file's route replaces it, see the BUG-301 gotcha in
  // `CLAUDE.md`) sets the same flag; mirrored here since testdriver.js reads
  // it regardless of which report script is paired with it.
  window.__wptrunner_is_test_context = true;

  function test_url() {
    // No fragment: WPT test ids never include one.
    return location.pathname + location.search;
  }

  add_completion_callback(function(tests, harness_status) {
    var subtests = tests.map(function(t) {
      return [t.name, t.status, t.message, t.stack];
    });
    window.__lumen_wpt_results = JSON.stringify([
      test_url(), harness_status.status, harness_status.message, harness_status.stack, subtests
    ]);
  });

  // WARNING: this file goes through Python string interpolation
  // (StaticHandler in tools/wptserve/wptserve/handlers.py does
  // 'data = data times format_args' using the four bare tokens below) before
  // being served — a literal percent sign anywhere in this file, including
  // in a comment, breaks that substitution at request time. Escape any
  // future percent sign as a doubled pair; better yet, avoid the character
  // entirely in comments the way this one does.
  //
  // route_builder.add_static (environment.py::get_routes) feeds this file
  // the same testharnessreport_format_args dict it feeds upstream's own
  // testharnessreport.js — but only if the file actually contains the four
  // placeholder tokens below and calls setup() with them, which this file
  // didn't do until now. Without this, --timeout-multiplier silently had no
  // effect on the file-level harness timeout: tests.setup() (testharness.js)
  // is the only thing that both applies the multiplier to
  // Tests.timeout_length AND reschedules the already-running timer
  // (Tests.prototype.setup calls this.set_timeout() at its end) — the Tests
  // singleton's constructor starts that timer immediately at testharness.js
  // parse time, at the unmultiplied default, so anything short of calling
  // setup() again leaves it running on the original deadline. Root-caused by
  // reading source, not guessed, while investigating the WPT-RUN-3
  // css-anchor-position TIMEOUT cluster (slice 40/42, ROADMAP.md): every
  // observed TIMEOUT had duration pinned at roughly 10.1 to 10.6 seconds
  // regardless of --timeout-multiplier, with message: null — exactly
  // Tests.prototype.timeout()'s harness-level signature, fired at the
  // default (unmultiplied) ~10s file timeout before the Python-side executor
  // or manager-watchdog timeouts (both already correctly multiplier-scaled,
  // see executorlumen.py/testrunner.py) were ever reached.
  setup({
    output: %(output)d,
    timeout_multiplier: %(timeout_multiplier)s,
    explicit_timeout: %(explicit_timeout)s,
    debug: %(debug)s
  });
})();
