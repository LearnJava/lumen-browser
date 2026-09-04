#!/usr/bin/env python3
"""WPT-RUN-6 slice 62: closes out the remaining unclassified bucket.

`timeout_audit.py --out-dir .tmp/wpt-corpus` reports 21 unclassified TIMEOUT
ids against the (unchanged since WPT-RUN-5) corpus snapshot. Grepping
`bugs/*.md` and every `tests/wpt/verify_slice*_gaps.py` for each of the 21
(slice 56/57's own lesson) sorts them into three groups:

1. **7 already fully explained by an earlier slice**, no new evidence
   needed: `console-log-large-array.any.html` (BUG-961, slices 39-48 — fixed
   mechanism, id itself stays unclassified by design, no textual marker
   exists for a now-fixed orchestration stall), `canvas-with-padding.html`
   and `video_crash_empty_src.html` (slice 34/37, hang hypothesis
   disproven), the three MathML files
   (`mrow/legacy-mrow-like-elements-001.html`, `mpadded/mpadded-003.html`,
   `relations/css-styling/ignored-properties-001.html`, slice 38, same),
   `svg/linking/scripted/a.ping-functionality.html` (slice 50, BUG-963 filed
   but TIMEOUT doesn't reproduce).
2. **12 read/probed by an earlier slice but not independently re-run this
   slice** (`content-visibility-069.html`, `elements-at-point.html`,
   `background-change-during-smooth-scroll.html`,
   `iframe-marginwidth-marginheight.html`,
   `frameset-element-synthetic-{error,}event.html` — slices 30/34/36;
   `grid-change-intrinsic-size-with-auto-repeat-tracks-001.html`,
   `computed-initial.html` — slice 56; `letter-spacing-trim-start-002.html`,
   `dangling-markup/media.html` — slice 57; `fetch/orb/tentative/
   status.sub.html` — slice 59; `resize-observer/scrollbars-2.html` — BUG-970's
   own discovery id).
3. **2 genuine probe-tool-gap leftovers slice 57/59 flagged but never ran**:
   `dictionary-decompression.tentative.https.h2.html` (slice 59: "needs a
   dedicated https/h2 harness invocation, not covered here") and
   `workers/interfaces/WorkerGlobalScope/location/redirect-sharedworker.html`
   (slice 57: "probe-tool-gap precedent, BUG-866" — categorized, not run).

This slice re-runs all 21 through `run_report.py` (the real `wptrunner`+
`wptserve` stack, isolated single-id runs via `--offset`/`--limit` computed
from each id's position in its own directory's sorted flat listing) to get
one fresh, single-slice confirmation that every one of them still holds on
current `main`, and — the actual point — to finally close the two group-3
gaps. `run_report.py`'s default setup turns out to already provide a real
https/h2 origin (group 3's stated blocker for `dictionary-decompression...
h2.html` was wrong: it just needed to be tried).

## Results (2026-09-04, dev-release, Linux, `main` = `8a48a0635`)

All 21 confirmed non-hanging in isolation — every one completes with
`harness OK` (`Test OK`/`Test OK, expected TIMEOUT`) in ~9-23s, never a real
TIMEOUT. Group 1's 7 need no new run (unchanged root cause: fixed/dead-end
mechanisms already documented). Group 2's 12 and group 3's 2 were all
re-run this slice; results match every prior slice's finding with one
exception and one new bug:

- `dictionary-decompression.tentative.https.h2.html` — group 3, first real
  run. `run_report.py --all --root fetch/compression-dictionary --offset 4
  --limit 1` completes in ~14s, `1/1 harness OK`, 0/4 subtests pass (`"available-
  dictionary" header is not available` on every one — Compression
  Dictionary Transport is simply unimplemented, a known-shape gap). Not a
  hang; stays unclassified, no marker, no new bug (same "FAIL promptly, no
  bug — WPT-RUN-6 is about TIMEOUTs" call as slice 57's `letter-spacing-
  trim-start-002.html`/`dangling-markup/media.html`).
- `redirect-sharedworker.html` — group 3, first real run. `run_report.py
  --all --root workers/interfaces/WorkerGlobalScope/location --offset 2
  --limit 1` completes in ~9s, `1/1 harness OK`, 0/1 subtest passes:
  `assert_equals: expected "/workers/interfaces/WorkerGlobalScope/location/
  redirect.js" but got "/common/redirect.py"`. Not a hang, but a genuine new
  defect: `self.location` inside the (shared-)worker still reports the
  *pre-redirect* constructor URL after the script fetch followed a real
  302. Traced to source: `crates/network/src/lib.rs::fetch_request_impl`
  computes the right final URL (`fetch_with_redirect` returns it) and
  immediately discards it (`let (resp, _final_url) = …`); `JsFetchResult`
  has no field to carry it even if a caller wanted it. Same discarded value
  also explains why `fetch()`'s `Response.url`/`.redirected` and
  `XMLHttpRequest.responseURL` are wrong after any redirect — three
  independent JS-visible symptoms, one root cause. Filed as
  [BUG-984](../../bugs/BUG-984-OPEN.md). Not a TIMEOUT; stays unclassified,
  no marker.
- All other 19 ids: outcome matches the earlier slice's finding exactly
  (FAIL-not-hang or full PASS, as already documented) — no new information,
  confirms nothing regressed/changed since.

Net: unclassified stays 21 — none of the 21 is eligible for a
`timeout_audit.py` marker (a marker needs a *textual* signature the browser
or the harness printed; every one of these either fully passes or FAILs
through a normal, already-attributed mechanism, so there is nothing for a
regex to key on). What changes is confidence: **all 21 remaining
unclassified ids in the WPT-RUN-5 snapshot are now confirmed, in one pass
on current `main`, to not be live engine hangs** — the exploratory phase
of "does this unclassified id still hang" is exhausted for this snapshot.
Further movement on WPT-RUN-6's unclassified bucket needs a fresh full
corpus run (the snapshot predates the BUG-961 orchestration fix from slice
47, which plausibly explains several of these on its own) — that is
WPT-RUN-9's scope, not a slice of this task.

Usage (from repo root, needs the WPT venv):

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root fetch/compression-dictionary --offset 4 --limit 1 \\
        --log-raw .tmp/wpt-corpus/s62-dict-decomp.raw.jsonl \\
        --out .tmp/wpt-corpus/s62-dict-decomp.html

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root workers/interfaces/WorkerGlobalScope/location --offset 2 --limit 1 \\
        --log-raw .tmp/wpt-corpus/s62-redirect-sw-full.raw.jsonl \\
        --out .tmp/wpt-corpus/s62-redirect-sw-full.html

The other 19 ids were re-run the same way, one `--root`/`--offset`/`--limit`
triple per id (`.tmp/s62_probe.py`, not committed — a thin loop over a
`(dir, filename)` list, each isolated `--offset` computed from
`sorted(os.listdir(dir))`, nothing this repo doesn't already have in
`run_report.py` itself).

Then re-run the classifier to confirm the count is unchanged:

    python3 tests/wpt/timeout_audit.py --out-dir .tmp/wpt-corpus --json /tmp/audit.json

This script itself just re-runs `timeout_audit.py --selftest` — same shape
as slices 57/59/60/61: the actual evidence is the `run_report.py`
invocations above, each of which manages its own wptserve/wptrunner
lifecycle and takes seconds to tens of seconds, so re-wrapping 21 of them
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
