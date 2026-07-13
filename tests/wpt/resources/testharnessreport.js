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
})();
