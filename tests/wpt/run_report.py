#!/usr/bin/env python3
"""Run WPT tests against Lumen over WebDriver BiDi and write a self-contained
HTML report (pass/fail counts, per-test and per-subtest breakdown).

Unlike `run_suite.py` (the S7 CI gate — exit 0 iff 0 unexpected against the
committed `.ini` expectations) and `run_smoke.py` (its underlying single-run
driver), this script is for *inspection*, not gating: it always writes a
report and always exits 0 unless the run itself couldn't start, regardless of
how many tests failed. Reuses `run_smoke.run()` for the actual wptrunner
invocation — no protocol/runner code duplicated here, only report rendering.

By default it runs the curated subset (`run_suite.curated_test_ids()` — the
same ~20 `dom/nodes/` tests the S7 gate covers, the only ones vetted to run
cleanly against this BiDi-only executor). `--all` instead discovers every
vendored/generatable test under `tests/wpt/<root>/` (`--root`, default
`dom/nodes`, 168 files) — most of those were never vetted for this project's
minimal executor (no `test_driver.*`, no multi-window, no iframes), so expect
ERROR/TIMEOUT noise, not failures worth filing bugs over; use it to survey,
not to gate.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/run_report.py [--binary PATH] [--out PATH] [--all] [--root DIR] [--recursive]

On Windows Git Bash also set `MSYS2_ARG_CONV_EXCL='/dom'` (see README) so the
leading-slash test ids aren't mangled into Windows paths.
"""

import argparse
import glob
import html
import json
import os
import sys
import tempfile
from datetime import datetime, timezone

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_smoke  # noqa: E402
import run_suite  # noqa: E402

REPO_ROOT = run_smoke.REPO_ROOT
DEFAULT_OUT = os.path.join(REPO_ROOT, ".tmp", "wpt-report.html")

# Test-level statuses that mean "the harness itself completed cleanly" —
# individual subtest failures are reported separately, this only reflects
# whether the test *finished* rather than erroring/timing out/crashing.
HARNESS_OK_STATUSES = {"OK", "PASS"}
SUBTEST_PASS_STATUSES = {"PASS"}


def all_vendored_test_ids(root: str = "dom/nodes", recursive: bool = False) -> list:
    """Every runnable test id under `tests/wpt/<root>/`, curated or not.

    Flat by default (`glob` at the top level only) — this is the original
    `dom/nodes` behavior (168 files) and must stay exact: that category's
    subdirectories (`crashtests/` — non-testharness crash-only pages;
    `moveBefore/`, `insertion-removing-steps/`, … — separate, never-vetted
    sub-suites) were never part of its documented scope, so silently pulling
    them in would inflate a number several docs (README.md, VENDOR.md,
    wpt-status.md) cite verbatim.

    `recursive=True` (opt in for categories that are organized into
    subdirectories, e.g. `FileAPI`) walks the whole tree instead. `.any.js`/
    `.window.js` files are testharness "multi-global" templates that
    wptserve's `AnyHtmlHandler`/`WindowHandler` (`tools/serve/serve.py`) wrap
    into a `.any.html`/`.window.html` response on request, so those are
    runnable ids too even though only the `.js` source is vendored — plain
    `.worker.js`/`.sub.js` helper scripts are not (nothing serves them as a
    standalone top-level test). `support/`/`resources/` hold fixtures, not
    tests, and `-manual.html` tests need human interaction this automated
    executor can't drive — both are skipped.
    """
    subdir = os.path.join(run_smoke.TESTS_ROOT, *root.split("/"))
    if not recursive:
        return sorted(
            f"/{root}/" + os.path.basename(p) for p in glob.glob(os.path.join(subdir, "*.html"))
        )
    ids = []
    for dirpath, dirnames, filenames in os.walk(subdir):
        dirnames[:] = sorted(d for d in dirnames if d not in ("support", "resources"))
        rel_dir = os.path.relpath(dirpath, run_smoke.TESTS_ROOT).replace(os.sep, "/")
        for fn in sorted(filenames):
            if fn.endswith((".any.js", ".window.js")):
                out = fn[: -len(".js")] + ".html"
            elif fn.endswith(".html") and "-manual" not in fn:
                out = fn
            else:
                continue
            ids.append(f"/{rel_dir}/{out}")
    return ids


def load_wptreport(path: str) -> dict:
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def esc(s) -> str:
    return html.escape("" if s is None else str(s))


def status_class(status: str, expected: str = None) -> str:
    if expected is not None and status != expected:
        return "unexpected"
    if status in ("PASS", "OK"):
        return "pass"
    if status in ("SKIP", "NOTRUN"):
        return "skip"
    return "fail"


def render_subtests(subtests: list) -> str:
    if not subtests:
        return ""
    rows = []
    for st in subtests:
        cls = status_class(st.get("status", ""), st.get("expected"))
        msg = esc(st.get("message") or "")
        rows.append(
            f'<tr class="sub {cls}"><td class="status">{esc(st.get("status"))}</td>'
            f'<td class="name">{esc(st.get("name"))}</td>'
            f'<td class="msg">{msg}</td></tr>'
        )
    return (
        '<table class="subtests"><thead><tr><th>Status</th><th>Subtest</th>'
        f"<th>Message</th></tr></thead><tbody>{''.join(rows)}</tbody></table>"
    )


def render_report(report: dict, binary: str, test_ids: list) -> str:
    results = sorted(report.get("results", []), key=lambda r: r["test"])
    time_start = report.get("time_start")
    time_end = report.get("time_end")
    duration_s = (time_end - time_start) / 1000.0 if time_start and time_end else None

    total_tests = len(results)
    harness_ok = sum(1 for r in results if r.get("status") in HARNESS_OK_STATUSES)
    unexpected_tests = sum(
        1 for r in results if r.get("expected", r.get("status")) != r.get("status")
    )

    all_subtests = [st for r in results for st in r.get("subtests", [])]
    total_subtests = len(all_subtests)
    subtests_passed = sum(1 for st in all_subtests if st.get("status") in SUBTEST_PASS_STATUSES)
    subtests_unexpected = sum(
        1 for st in all_subtests if st.get("expected", st.get("status")) != st.get("status")
    )

    rows = []
    for r in results:
        subtests = r.get("subtests", [])
        sub_passed = sum(1 for st in subtests if st.get("status") in SUBTEST_PASS_STATUSES)
        cls = status_class(r.get("status", ""), r.get("expected"))
        summary = (
            f'<tr class="test {cls}">'
            f'<td class="status">{esc(r.get("status"))}</td>'
            f'<td class="name"><code>{esc(r.get("test"))}</code></td>'
            f'<td class="subcount">{sub_passed}/{len(subtests)}</td>'
            f'<td class="dur">{(r.get("duration") or 0) / 1000.0:.2f}s</td>'
            f"</tr>"
        )
        detail = render_subtests(subtests)
        if detail:
            msg = r.get("message")
            suffix = f" ({esc(msg)})" if msg else ""
            rows.append(summary)
            rows.append(
                f'<tr class="detail-row {cls}"><td colspan="4">'
                f"<details><summary>{esc(r.get('test'))} — {esc(r.get('status'))}"
                f"{suffix}</summary>{detail}</details></td></tr>"
            )
        else:
            rows.append(summary)

    missing = sorted(set(test_ids) - {r["test"] for r in results})
    missing_html = ""
    if missing:
        missing_html = (
            '<h2>Not run</h2><p>Selected but produced no result (crashed before '
            "test_start, or wptrunner aborted early):</p><ul>"
            + "".join(f"<li><code>{esc(t)}</code></li>" for t in missing)
            + "</ul>"
        )

    generated = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Lumen WPT report — {generated}</title>
<style>
  body {{ font: 14px/1.5 -apple-system, Segoe UI, sans-serif; margin: 2rem; color: #1a1a1a; background: #fafafa; }}
  h1 {{ font-size: 1.4rem; margin-bottom: 0.2rem; }}
  .meta {{ color: #666; font-size: 0.85rem; margin-bottom: 1.5rem; }}
  .meta code {{ background: #eee; padding: 0 4px; border-radius: 3px; }}
  .cards {{ display: flex; gap: 1rem; flex-wrap: wrap; margin-bottom: 1.5rem; }}
  .card {{ background: #fff; border: 1px solid #ddd; border-radius: 6px; padding: 0.75rem 1.25rem; min-width: 130px; }}
  .card .n {{ font-size: 1.6rem; font-weight: 600; display: block; }}
  .card .l {{ font-size: 0.8rem; color: #666; }}
  .card.pass .n {{ color: #1a7f37; }}
  .card.fail .n {{ color: #cf222e; }}
  table {{ border-collapse: collapse; width: 100%; background: #fff; }}
  #results {{ border: 1px solid #ddd; border-radius: 6px; overflow: hidden; }}
  #results thead th {{ text-align: left; background: #f0f0f0; padding: 0.5rem 0.75rem; font-size: 0.8rem; }}
  #results td {{ padding: 0.4rem 0.75rem; border-top: 1px solid #eee; vertical-align: top; }}
  tr.test td.status, tr.sub td.status {{ font-weight: 600; width: 5.5rem; }}
  tr.pass td.status {{ color: #1a7f37; }}
  tr.fail td.status {{ color: #cf222e; }}
  tr.unexpected td.status {{ color: #cf222e; }}
  tr.skip td.status {{ color: #9a6700; }}
  tr.detail-row {{ background: #fcfcfc; }}
  tr.detail-row > td {{ padding: 0 0.75rem 0.5rem; border-top: none; }}
  details summary {{ cursor: pointer; color: #444; font-size: 0.85rem; padding: 0.2rem 0; }}
  table.subtests {{ margin: 0.4rem 0 0.6rem; border: 1px solid #eee; }}
  table.subtests th {{ background: #f6f6f6; font-size: 0.75rem; padding: 0.3rem 0.6rem; text-align: left; }}
  table.subtests td {{ padding: 0.3rem 0.6rem; font-size: 0.82rem; border-top: 1px solid #f0f0f0; }}
  td.msg {{ color: #555; font-family: ui-monospace, monospace; font-size: 0.78rem; }}
  code {{ font-family: ui-monospace, Consolas, monospace; }}
</style>
</head>
<body>
<h1>Lumen — WPT report</h1>
<p class="meta">Generated {generated} &middot; binary <code>{esc(binary)}</code>
&middot; {esc(len(test_ids))} test id(s) selected
{f"&middot; run took {duration_s:.1f}s" if duration_s is not None else ""}</p>

<div class="cards">
  <div class="card {'pass' if unexpected_tests == 0 else 'fail'}">
    <span class="n">{total_tests}</span><span class="l">tests run</span>
  </div>
  <div class="card pass"><span class="n">{harness_ok}</span><span class="l">harness OK</span></div>
  <div class="card {'fail' if total_tests - harness_ok else 'pass'}">
    <span class="n">{total_tests - harness_ok}</span><span class="l">harness ERROR/TIMEOUT/CRASH</span>
  </div>
  <div class="card pass"><span class="n">{subtests_passed}</span><span class="l">subtests passed</span></div>
  <div class="card {'fail' if total_subtests - subtests_passed else 'pass'}">
    <span class="n">{total_subtests - subtests_passed}</span><span class="l">subtests failed</span>
  </div>
  <div class="card {'fail' if unexpected_tests or subtests_unexpected else 'pass'}">
    <span class="n">{unexpected_tests + subtests_unexpected}</span><span class="l">unexpected (vs .ini)</span>
  </div>
</div>

<h2>Tests</h2>
<table id="results">
<thead><tr><th>Status</th><th>Test</th><th>Subtests</th><th>Duration</th></tr></thead>
<tbody>
{''.join(rows)}
</tbody>
</table>
{missing_html}
</body>
</html>
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=run_smoke.default_binary())
    parser.add_argument("--out", default=DEFAULT_OUT, help="HTML report path")
    parser.add_argument(
        "--all",
        action="store_true",
        help="run every vendored test under --root, not just the curated/vetted subset",
    )
    parser.add_argument(
        "--root",
        default="dom/nodes",
        help="category subdir under tests/wpt/ to scan with --all (default: dom/nodes)",
    )
    parser.add_argument(
        "--recursive",
        action="store_true",
        help="with --all, walk --root recursively and expand .any.js/.window.js "
        "(needed for categories organized into subdirectories, e.g. FileAPI; "
        "dom/nodes stays flat/168 either way unless this is passed)",
    )
    args = parser.parse_args()

    test_ids = (
        all_vendored_test_ids(args.root, args.recursive) if args.all else run_suite.curated_test_ids()
    )
    if not test_ids:
        print("no tests selected", file=sys.stderr)
        return 1

    kind = f"all vendored ({args.root})" if args.all else "curated"
    print(f"running {len(test_ids)} {kind} WPT tests against {args.binary}", file=sys.stderr)

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with tempfile.NamedTemporaryFile(
        suffix=".json", delete=False, dir=os.path.dirname(os.path.abspath(args.out))
    ) as tmp:
        json_path = tmp.name

    try:
        rv = run_smoke.run(args.binary, test_ids, extra_args=[f"--log-wptreport={json_path}"])
        if not os.path.isfile(json_path) or os.path.getsize(json_path) == 0:
            print("wptrunner produced no report (crashed before suite_end?)", file=sys.stderr)
            return rv or 1

        report = load_wptreport(json_path)
        out_html = render_report(report, args.binary, test_ids)
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(out_html)
    finally:
        try:
            if os.path.isfile(json_path):
                os.remove(json_path)
        except OSError:
            # Windows can keep a brief handle on the file mozlog just closed
            # (AV scan / delayed release); it's scratch space under `.tmp/`,
            # leaving one stray file behind isn't worth failing the report over.
            pass

    results = report.get("results", [])
    total = len(results)
    ok = sum(1 for r in results if r.get("status") in HARNESS_OK_STATUSES)
    all_subtests = [st for r in results for st in r.get("subtests", [])]
    sub_total = len(all_subtests)
    sub_pass = sum(1 for st in all_subtests if st.get("status") in SUBTEST_PASS_STATUSES)
    print(f"tests: {ok}/{total} harness OK; subtests: {sub_pass}/{sub_total} passed", file=sys.stderr)
    print(f"report written to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
