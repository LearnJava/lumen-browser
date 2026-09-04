#!/usr/bin/env python3
"""WPT-RUN-6 slice 60: the last of slice 59's four probe-tool-gap candidates
that could actually be run under the real stack — `xhr/cors-expose-star.sub.any.html`
(slice 59 ran out of budget before reaching it; `dictionary-decompression.
tentative.https.h2.html` still needs a dedicated https/h2 harness this slice
does not add either).

## Results (2026-09-04, dev-release, Linux, `main` = `aee74dbba`)

Confirmed live via `run_report.py --all --root xhr --recursive --offset 77
--limit 1` (real `wptrunner`+`wptserve`, offset picked by locating the id in
`all_vendored_test_ids('xhr', recursive=True)`): all 3/3 subtests TIMEOUT,
0/1 harness OK, 10s each.

New bug: [BUG-980](../../bugs/BUG-980-OPEN.md) — `XMLHttpRequest.send()`
(`crates/js/src/xhr.rs`) runs the entire request/response cycle
synchronously inside the call itself (its own comment says so: "Execute
synchronously using the same native fetch bindings"), firing every
readyState transition (`HEADERS_RECEIVED`/`LOADING`/`DONE`) and progress
event before `send()` returns to caller JS. All three of this file's
`async_test`s use the ordering XHR §4.5.6's own examples use — `open()` →
`send()` → `onreadystatechange = ...` — which only works if events are
deferred to the event loop; here the handler is assigned to an
already-finished request and never called.

Confirmed directly with a minimal live probe (`--mcp-live-port`, `eval`,
against a throwaway `python -m http.server` origin, not the WPT stack):

    var x = new XMLHttpRequest();
    x.open('GET', '/data.txt');
    x.send();
    // readyState already 4 (DONE) right here — the whole exchange
    // happened synchronously inside send().
    x.onreadystatechange = function(){ log.push(x.readyState); };
    // log stays [] forever: the handler was attached after every
    // transition had already fired and been discarded.

Attaching the handler *before* `send()` instead makes the same request
log `[2, 3, 4]` immediately (still synchronously, inside `send()` itself —
non-conformant timing, but happens to keep tests using that ordering
green), which is why not every XHR test in the corpus hangs on this.

Classified via `_exact_id_marker` (`xhr-send-runs-synchronously`).
Unclassified 24 → 23. `dictionary-decompression.tentative.https.h2.html`
remains open for a future slice (needs a dedicated https/h2 harness
invocation `run_report.py`'s plain-http `--root` run doesn't provide).

Usage (from repo root, needs the WPT venv):

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root xhr --recursive --offset 77 --limit 1 \\
        --log-raw .tmp/wpt-corpus/s60-cors-expose-star.raw.jsonl \\
        --out .tmp/wpt-corpus/s60-cors-expose-star.html

The offset (77) is `xhr/cors-expose-star.sub.any.html`'s index in
`run_report.all_vendored_test_ids('xhr', recursive=True)` (323 ids total) —
running the whole directory recursively is unnecessary for a single-id
probe and costs much more wall-clock.
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
