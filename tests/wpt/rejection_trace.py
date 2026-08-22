#!/usr/bin/env python3
"""Make an unhandled promise rejection visible in a WPT run (WPT-RUN-6 slice 10).

At the time this script was written, Lumen never dispatched
`unhandledrejection` ([BUG-716](../../bugs/BUG-716-FIXED.md), fixed
2026-08-22: `v8::Isolate::set_promise_reject_callback` in `v8_runtime.rs`),
so a promise that rejects with nobody chained onto it produced no event, no
console line and no stderr output. `testharness.js` reports a test that fails
inside a promise chain *through* that event
(`addEventListener("unhandledrejection", ...)` → `error_handler`), so before
the fix such a test did not FAIL — it went quiet and was killed by the
harness timeout. The verdict was TIMEOUT, the browser log was clean, and
`timeout_audit.py` filed it under `unclassified`. With BUG-716 fixed, this
specific TIMEOUT class should now surface as an ordinary FAIL on its own —
this tracer remains useful as an independent, finer-grained trace of
rejection activity regardless (pending re-measurement of how much of the
`unclassified` bucket it now explains).

This script instruments a run so that class becomes measurable: it appends a
tracker to Lumen's own `tests/wpt/resources/testharnessreport.js` (loaded by
every testharness test, before the test body) that reimplements the tracking
part of HTML LS §8.1.7.5 in JS — `then` marks its receiver as chained, every
derived promise is watched, and a rejection nobody chained onto is printed as
`LUMEN_UNHANDLED_REJECTION: <reason>`. Lumen prints `console.log` to stderr,
wptrunner captures stderr as `process_output`, and `timeout_audit.py` attributes
it to the test that was running — so the mechanism lands in the normal table.

**An instrumented run is not a measurement of the corpus.** Wrapping
`Promise.prototype.then` changes the identity, `name` and `length` of a
function some tests inspect, and attaches a rejection handler to every derived
promise. Use it to explain TIMEOUTs, then turn it off; never leave it on for a
pass-rate run.

Usage (from the repo root):

    python tests/wpt/rejection_trace.py --on
    <venv>/python tests/wpt/run_report.py --all --root <category> --recursive \\
        --log-raw .tmp/trace/run.raw.jsonl --binary target/dev-release/lumen
    python tests/wpt/rejection_trace.py --off
    <venv>/python tests/wpt/timeout_audit.py --out-dir .tmp/trace

`--status` reports whether the harness is currently instrumented.
"""

import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPORT_JS = os.path.join(HERE, "resources", "testharnessreport.js")

BEGIN = "  // BEGIN rejection_trace.py -- NOT COMMITTED WITH THIS BLOCK IN PLACE"
END = "  // END rejection_trace.py"

#: The tracker itself. No percent sign anywhere: `StaticHandler` runs this file
#: through Python string interpolation before serving it (see the warning in
#: `testharnessreport.js`), and a bare percent breaks that at request time.
TRACER = BEGIN + """
  // WPT-RUN-6 slice 10: BUG-716 leaves an unhandled rejection completely
  // silent, which turns a failed assertion inside a promise chain into a
  // TIMEOUT with an empty log. Reimplement the tracking half of HTML LS
  // 8.1.7.5 in JS so the run has evidence to classify.
  (function () {
    var origThen = Promise.prototype.then;
    var origReject = Promise.reject;
    function describe(reason) {
      try {
        if (reason && reason.message) { return String(reason.message); }
        return String(reason);
      } catch (e) { return '<unstringifiable>'; }
    }
    function watch(p) {
      // origThen, not the patched then: watching must not mark p as chained.
      origThen.call(p, undefined, function (reason) {
        // Grace period: a handler attached in a later turn (t.step_timeout,
        // an await resumed by an event) is legitimate and must not count.
        setTimeout(function () {
          if (!p.__lumen_chained) {
            console.log('LUMEN_UNHANDLED_REJECTION: ' + describe(reason));
          }
        }, 200);
      });
    }
    Promise.prototype.then = function (onFulfilled, onRejected) {
      this.__lumen_chained = true;
      var derived = origThen.call(this, onFulfilled, onRejected);
      watch(derived);
      return derived;
    };
    Promise.reject = function (reason) {
      var p = origReject.call(this, reason);
      watch(p);
      return p;
    };
  })();
""" + END + "\n"


def read_report():
    with open(REPORT_JS, encoding="utf-8") as fh:
        return fh.read()


def write_report(text):
    with open(REPORT_JS, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def instrumented(text):
    return BEGIN in text


def turn_on():
    text = read_report()
    if instrumented(text):
        print("already instrumented — nothing to do")
        return 0
    if not text.endswith("\n"):
        text += "\n"
    write_report(text + TRACER)
    print(f"instrumented {os.path.relpath(REPORT_JS, os.path.dirname(HERE))}")
    print("REMEMBER: --off before any run whose numbers are meant to be trusted")
    return 0


def turn_off():
    text = read_report()
    if not instrumented(text):
        print("not instrumented — nothing to do")
        return 0
    head, _, rest = text.partition(BEGIN)
    _, _, tail = rest.partition(END)
    write_report(head.rstrip("\n") + "\n" + tail.lstrip("\n"))
    print("instrumentation removed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--on", action="store_true", help="add the tracker")
    group.add_argument("--off", action="store_true", help="remove the tracker")
    group.add_argument("--status", action="store_true",
                       help="report whether the tracker is in place")
    args = parser.parse_args()
    if args.status:
        print("instrumented" if instrumented(read_report()) else "clean")
        return 0
    return turn_on() if args.on else turn_off()


if __name__ == "__main__":
    sys.exit(main())
