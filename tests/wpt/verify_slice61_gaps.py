#!/usr/bin/env python3
"""WPT-RUN-6 slice 61: the last two `content-security-policy/frame-ancestors/
report-*.sub.html` ids in the residual list — both run the identical
`checkReport.sub.js` idiom (an XHR to `reporting/resources/report.py?
op=retrieve_report&timeout=20`, which the server holds open for up to 20 real
seconds via a 0.5s poll loop). Same escalation as slice 59/60: `serve_wpt_like.
py` cannot serve `report.py` (a real wptserve CGI-style handler) or apply the
`{{$id:uuid()}}`/`{{GET[...]}}` substitutions the two test files and
`checkReport.sub.js` depend on, so this slice drives the real `wptrunner`+
`wptserve` stack via `run_report.py --all --root content-security-policy/
frame-ancestors --recursive`.

## Results (2026-09-04, dev-release, Linux, `main` = `874b941bc`)

New bug filed: [BUG-981](../../bugs/BUG-981-OPEN.md). Neither id actually
hangs from a CSP/report defect — CSP `frame-ancestors` enforcement does not
exist at all (BUG-811/GAP-CSPENF), so the report never arrives regardless;
`checkReport.sub.js`'s own XHR reads back `[]` after the full 20s
server-side wait and throws (`Cannot read properties of undefined (reading
'body')` on `data[0]`, `data` being empty) inside `report.onload` —
`testharness.js`'s `Test.prototype.step` catches that and reports a normal
`FAIL`, not a hang (`resources/testharness.js:2868-2880`). What actually
produces the corpus-visible TIMEOUT is a **different, engine-transport**
mechanism found while triaging why one of the two runs came back `ERROR`
instead of `FAIL`:

- Full 34-file directory run
  (`.tmp/wpt-corpus/s61-frame-ancestors.raw.jsonl`): test #33
  `report-blocked-frame.sub.html` completes `FAIL` after 20.32s (the
  connection is still young enough to survive). Test #34
  `report-only-frame.sub.html`, `navigate()`d 0ms later, fails at +20.27s
  with `ExecutorException: ('ERROR', 'browsingContext.navigate(...) failed:
  unknown error (WebSocket connection closed)')` — yet the browser's own
  `process_output` log shows it kept fetching/parsing normally up to and
  past that instant (same `browser_pid`, no restart).
- Isolated solo run (`--offset 33 --limit 1`, fresh session): completes
  fine, `FAIL` not `ERROR`, 20.39s, `1/1 harness OK`.
- Isolated pair run (`--offset 32 --limit 2`, fresh session): both files
  back-to-back, no prior tests — completes fine, `2/2 harness OK`, no
  `ERROR`.

Root cause pinned by source reading, not just the timing coincidence:
`crates/bidi-server/src/transport.rs::handle`'s loop only answers WebSocket
Ping (`crates/devtools/src/ws.rs::read_text_frame`, opcode `0x9` →
immediate `0xA`) while it is calling `read_text_frame` — not while
`dispatch()` (a `navigate` with `wait: "complete"`) is blocked processing
the page's own ~20s-long synchronous XHR (BUG-980). `wptrunner`'s BiDi
client (`tools/webdriver/webdriver/bidi/transport.py:47`,
`websockets.connect` with no override) uses the `websockets` library's
defaults — confirmed in the project's own venv, `websockets==16.1`:
`ping_interval=20`, `ping_timeout=20`. If the client's periodic Ping lands
inside a `dispatch()` window that outlasts its own 20s `ping_timeout`
before a Pong arrives, the client itself tears the connection down —
timing-dependent (which of the two ids absorbs it depends on connection
age when the block starts), not a deterministic property of either file.

Classified via `_exact_id_marker` in `timeout_audit.py`
(`bidi-ping-starved-by-blocking-dispatch`) for both ids. unclassified
23 → 21 this slice.

Usage (from repo root, needs the WPT venv):

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root content-security-policy/frame-ancestors --recursive \\
        --log-raw .tmp/wpt-corpus/s61-frame-ancestors.raw.jsonl \\
        --out .tmp/wpt-corpus/s61-frame-ancestors.html

    # isolate the pair to check reproducibility (offsets from the run above):
    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root content-security-policy/frame-ancestors --recursive \\
        --offset 33 --limit 1   # report-only-frame.sub.html alone
    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root content-security-policy/frame-ancestors --recursive \\
        --offset 32 --limit 2   # both report-*.sub.html back-to-back, fresh session

Then re-run the classifier to confirm the reattribution:

    python3 tests/wpt/timeout_audit.py --out-dir .tmp/wpt-corpus --json /tmp/audit.json

Same shape as slice 59/60: this script itself just re-runs `timeout_audit.py
--selftest` (the new `Mechanism` entry's own guard) — the actual evidence is
the three `run_report.py` invocations above, each of which manages its own
wptserve/wptrunner lifecycle and takes tens of seconds, so re-wrapping them
here would only add a layer with nothing to verify beyond "the classifier
still parses".
"""
import subprocess
import sys
import os

HERE = os.path.dirname(os.path.abspath(__file__))


def main():
    result = subprocess.run(
        [sys.executable, os.path.join(HERE, "timeout_audit.py"), "--selftest"],
        check=False)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
