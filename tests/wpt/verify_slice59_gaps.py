#!/usr/bin/env python3
"""WPT-RUN-6 slice 59: the current 25-id residual (`.tmp/wpt-corpus`'s
`timeout_audit.py --json`) turned out to be **fully mined** — every single id
already has a probe or a bug-file mention from slices 30-58 (checked by
grepping `bugs/*.md` and every `tests/wpt/verify_slice*_gaps.py` for each id's
basename first, the lesson slice 56/57 both record). Four of those 25 are the
documented "probe-tool gap" class (`docs/probe-method.md`'s `serve_wpt_like.py`
gotcha: no CGI, no `?pipe=`, no `{{domains[..]}}`, no `.any.js` on-the-fly
generation): `network-efficiency-guardrails-json.tentative.html`,
`dictionary-decompression.tentative.https.h2.html`,
`fetch/orb/tentative/status.sub.html`, `xhr/cors-expose-star.sub.any.html`.

This slice escalates past that gap for the first time: instead of the bare
`serve_wpt_like.py` probe, it drives the **real** `wptrunner`+`wptserve` stack
through `run_report.py --all --root <dir> --recursive`, which does implement
CGI handlers, `?pipe=`, `{{domains[..]}}` substitution and `.any.js`
generation — i.e. exactly the four features `serve_wpt_like.py` cannot do.
This is slower (a live browser + real wptserve per id, not a single static
file server) but is the only way to get real evidence for these four ids.

## Results (2026-09-04, dev-release, Linux, `main` = `3b9a288d0`)

One real finding, no new bug (an existing static-read gap now has its first
live confirmation):

- `document-policy/experimental-features/*.tentative.html` (all three files
  in the directory, run together since `--root` is a directory, not a single
  id) — genuinely TIMEOUT under the real stack (10 s each, 0/3 harness OK).
  Every one awaits a `document-policy-violation` report via
  `ReportingObserver`; `ReportingObserver` itself works
  (`crates/js/src/reporting_api.rs`), but nothing in the workspace ever calls
  `_lumen_deliver_report` with that type — Document-Policy header parsing and
  violation detection are simply unimplemented. Exactly the mechanism
  [BUG-953](../../bugs/BUG-953-OPEN.md) already documented from a slice-32
  static grep, without a single live measurement or a `timeout_audit.py`
  entry. Classified via `_exact_id_marker`
  (`document-policy-violation-report-missing`), placed *before* the older
  `cssom-stylesheets-missing` regex in `SOURCE_MARKERS` on purpose: two of
  the three files also reference `document.styleSheets[0]` inside their own
  `check_report_format` helper (to read the stylesheet's `href` for the
  report's expected `sourceFile`), which had been silently misattributing
  their hang to the CSSOM gap (BUG-471) — a genuine mechanism ordering bug
  in the classifier itself, not just a missing entry, since `SOURCE_MARKERS`
  is first-match-wins by list position (`classify_source`).

Three of the four probe-tool-gap ids resolved without a new finding:

- `fetch/orb/tentative/status.sub.html` — **already fixed**, no longer
  reproduces: `harness-complete`-equivalent PASS under the real stack
  (`video.onerror` fires for the malformed-status media request as
  expected). The audit's residual list is built from the WPT-RUN-5 snapshot,
  which predates whatever later slice's fix made this pass; stays
  unclassified (nothing to classify — it is not live anymore), same
  "already-probed-clean" shape as `video_crash_empty_src.html`/
  `canvas-with-padding.html`.
- `fetch/orb/tentative/status.sub.any.html` (the sibling `.any.js`
  expansion, not itself in the residual list but run alongside
  `status.sub.html` in the same `--root`) — TIMEOUTs, but already explained
  by the WPT-RUN-10 `foreign-host-unresolvable` mechanism (needs
  `www1.<host>` DNS, a known, already-tracked gap) — not a new finding, and
  the classifier already has it right.
- `xhr/cors-expose-star.sub.any.html` was not run this slice (out of budget
  after the two directory runs above) — still an open probe-tool-gap
  candidate for a future slice.

`dictionary-decompression.tentative.https.h2.html` was not run either — it
needs a real h2/https origin, which `run_report.py`'s plain-http `--root`
run does not provide; a future slice needs a dedicated https/h2 harness
invocation, not covered here.

Net: unclassified 25 → 24 (only the `-json` variant was actually counted as
unclassified before this slice; the other two `network-efficiency-guardrails`
files were miscounted under `cssom-stylesheets-missing`, fixed by the same
commit). BUG-953 gets its first live confirmation, no new bug filed.

Usage (from repo root, needs the WPT venv — `serve_wpt_like.py` cannot serve
these, see above):

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root document-policy/experimental-features --recursive \\
        --log-raw .tmp/wpt-corpus/s59-docpolicy.raw.jsonl \\
        --out .tmp/wpt-corpus/s59-docpolicy.html

    tests/wpt/.venv/bin/python tests/wpt/run_report.py --binary target/dev-release/lumen \\
        --all --root fetch/orb/tentative --recursive \\
        --log-raw .tmp/wpt-corpus/s59-orb.raw.jsonl \\
        --out .tmp/wpt-corpus/s59-orb.html

Then re-run the classifier to confirm the reattribution:

    python3 tests/wpt/timeout_audit.py --out-dir .tmp/wpt-corpus --json /tmp/audit.json

This script itself just re-runs `timeout_audit.py --selftest` (the new
`Mechanism` entry's own guard) — the actual evidence is the two `run_report.py`
invocations above, not something this file automates end to end (unlike the
`--mcp-live-port` probes of slices 30-58, a directory-level `run_report.py`
run takes tens of seconds to minutes and manages its own wptserve/wptrunner
lifecycle, so re-wrapping it here would only add a layer with nothing to
verify beyond "the classifier still parses").
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
