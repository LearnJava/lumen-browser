#!/usr/bin/env python3
"""WPT-RUN-4 (`docs/tasks/p2-wpt-runner-throughput.md`): run the whole vendored
WPT corpus in shards and aggregate one pass-rate out of it.

Relationship to the existing scripts: `run_suite.py` gates the hand-curated
`dom/nodes` subset, `run_report.py` renders one category as HTML. Neither can
answer "what is Lumen's WPT pass-rate" — they select tests by globbing the file
system and report per invocation. This script selects from `MANIFEST.json`
(`corpus_stats.py`, the only denominator comparable to wpt.fyi/Servo/Ladybird)
and drives `run_smoke.py` once per shard.

Why shards and subprocesses rather than one in-process run:

* `css` alone is 37k ids. One `wptreport.json` is written at the *end* of a
  run, so a single process for a category that size means a crash or a hang
  five hours in loses everything. A shard is a checkpoint.
* A subprocess can be killed. A hung `wptrunner` inside this interpreter
  cannot — and a corpus run must survive one bad shard, not abort on it
  (`--category-timeout`).
* `MANIFEST.json` is updated exactly once up front (`--no-manifest-update` on
  every shard afterwards). Left at wptrunner's default, each of the ~300 shards
  would rescan 72k files, which costs minutes per shard and dwarfs the actual
  testing.

Scoring is deliberately the same shape wpt.fyi uses, so the number means the
same thing (see `docs/wpt/pass-rate.md` for the written-down methodology):
a test with subtests scores `passed_subtests / total_subtests`; a test without
scores 1 if it PASSed; **every manifest id that never ran scores 0** — reftests
we have no executor for (`TEST-4`), shards that timed out, tests skipped by the
runner. Not running something is not the same as it not counting.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/run_corpus.py --binary PATH [--pilot | --all |
        --categories a,b,c] [--processes N] [--out-dir DIR] [--resume]
        [--aggregate-only] [--run-json PATH]
"""

import argparse
import json
import os
import subprocess
import sys
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
METADATA_ROOT = os.path.join(TESTS_ROOT, "metadata")
MANIFEST_PATH = os.path.join(METADATA_ROOT, "MANIFEST.json")
DEFAULT_OUT_DIR = os.path.join(REPO_ROOT, ".tmp", "wpt-corpus")

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import corpus_stats  # noqa: E402

#: Pilot selection (WPT-RUN-4 slice 2): ten categories picked to exercise the
#: *orchestrator*, not the engine — one per hazard we expect to hit in the full
#: run. Sizes are from `corpus_stats.py` at the time of writing.
#:   dom            — the known-good baseline (the curated gate lives here)
#:   encoding       — large, flat, pure testharness (1338): throughput check
#:   FileAPI        — organised into subdirectories: shard-splitting check
#:   WebCryptoAPI   — `.https.`-only (313): exercises the pregenerated certs
#:   websockets     — needs the ws/wss servers (the `pywebsocket3` gotcha)
#:   workers        — spawns real workers: multi-context lifetime
#:   xhr            — network-heavy against wptserve
#:   shadow-dom     — mixed testharness/reftest/crashtest in one category
#:   mathml         — reftest-dominated (278 reftest vs 196 testharness)
#:   webdriver      — 626 `wdspec` ids we have no executor for: must score 0,
#:                    not crash the run
PILOT_CATEGORIES = [
    "dom", "encoding", "FileAPI", "WebCryptoAPI", "websockets",
    "workers", "xhr", "shadow-dom", "mathml", "webdriver",
]

#: Above this many ids a category is split into shards by its second path
#: component. Keeps a single `wptreport.json` (and a single failure) bounded.
SHARD_THRESHOLD = 2000

#: Statuses that mean the test itself finished cleanly. Subtest failures are
#: scored separately; this only says the harness completed.
HARNESS_OK = frozenset({"OK", "PASS"})


def update_manifest() -> None:
    """Refresh `MANIFEST.json` once, up front, so shards can skip it."""
    sys.path[:0] = [
        REPO_ROOT,
        os.path.join(REPO_ROOT, "tools"),
        os.path.join(REPO_ROOT, "tools", "wptserve"),
        os.path.join(REPO_ROOT, "tools", "webdriver"),
        os.path.join(REPO_ROOT, "tools", "wptrunner"),
    ]
    import localpaths  # noqa: F401
    from manifest import manifest as wptmanifest

    print("updating MANIFEST.json (once for the whole run) ...", flush=True)
    started = time.time()
    wptmanifest.load_and_update(TESTS_ROOT, MANIFEST_PATH, "/", update=True,
                                metadata_path=METADATA_ROOT, parallel=True)
    print(f"manifest updated in {time.time() - started:.0f}s", flush=True)


def _git_head() -> str:
    """Short SHA of the checkout the run was made from, or `unknown`."""
    try:
        out = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=REPO_ROOT,
                             capture_output=True, text=True, check=False)
        return out.stdout.strip() or "unknown"
    except OSError:
        return "unknown"


def load_manifest() -> dict:
    with open(MANIFEST_PATH, encoding="utf-8") as fh:
        return json.load(fh)


def plan_shards(manifest: dict, categories: list) -> list:
    """Split the selected categories into runnable shards.

    A shard is `{"name", "prefix", "ids"}` where `prefix` is what gets passed
    to wptrunner as a positional test filter. Categories under
    `SHARD_THRESHOLD` ids stay whole; larger ones split on their second path
    component so that no single `wptreport.json` — and no single timeout —
    covers more than a slice.
    """
    by_category = {}
    for _test_type, category, test_id in corpus_stats.iter_ids(manifest):
        by_category.setdefault(category, []).append(test_id)

    shards = []
    for category in categories:
        ids = by_category.get(category)
        if not ids:
            print(f"warning: category not in manifest, skipped: {category}", file=sys.stderr)
            continue
        shards.extend(_split([category], ids))
    return shards


def _split(prefix_parts: list, ids: list) -> list:
    """Recursively split a directory's ids until each shard fits the threshold.

    One level of splitting is not enough: `css/CSS2` alone is 9228 ids, which
    at the measured rate budgets over five hours — and a shard that dies takes
    its whole budget's worth of work with it. Descends until either the shard
    fits or the directory has no deeper level left to split on (a flat
    category like `encoding`, where the only option would be splitting the
    file list itself — deliberately not done, since a path prefix is what
    wptrunner filters on).
    """
    depth = len(prefix_parts)
    name = "/".join(prefix_parts)
    if len(ids) <= SHARD_THRESHOLD:
        return [{"name": name, "prefix": f"/{name}/", "ids": len(ids)}]

    groups = {}
    for test_id in ids:
        parts = test_id.strip("/").split("/")
        # A test file sitting directly in this directory has no deeper
        # component to group on; it stays here.
        key = parts[depth] if len(parts) > depth + 1 else ""
        groups.setdefault(key, []).append(test_id)

    if len(groups) == 1 and "" in groups:
        # Flat directory, nothing deeper to split on — accept the oversized
        # shard rather than inventing a split wptrunner can't express.
        return [{"name": name, "prefix": f"/{name}/", "ids": len(ids)}]

    shards = []
    for key, group_ids in sorted(groups.items()):
        if not key:
            # Files sitting directly in a directory that also has subdirectories
            # cannot be addressed by prefix — `/css/CSS2/` would re-select every
            # subdirectory we just split out, running them twice. Corpus-wide
            # this is 5 such nodes totalling ~100 ids, so listing them
            # explicitly is both exact and short enough for a command line.
            shards.append({"name": f"{name} (bare)", "prefix": None,
                           "test_ids": sorted(group_ids), "ids": len(group_ids)})
        else:
            shards.extend(_split(prefix_parts + [key], group_ids))
    return shards


def shard_report_path(out_dir: str, shard: dict) -> str:
    return os.path.join(out_dir, shard["name"].replace("/", "__") + ".json")


def kill_tree(proc) -> None:
    """Kill the shard subprocess *and* the browser processes it spawned.

    `Popen.kill()` only reaps the Python child; every `lumen.exe` wptrunner
    started stays alive and keeps holding its BiDi port, which then breaks the
    next shard. Killing by PID tree (never by image name — that would take out
    unrelated browser windows, including another session's).
    """
    if os.name == "nt":
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                       capture_output=True, check=False)
    else:
        proc.kill()


def shard_timeout(shard: dict, base: int, per_id: float) -> int:
    """Budget a shard's wall-clock by its size, not by a flat constant.

    WPT-RUN-4 pilot: a flat 1200s killed `encoding` (1343 ids) mid-run while
    being ten times more than `FileAPI` (125 ids) ever needed. A killed shard
    loses everything it had done — `wptreport.json` is only written at the end —
    so the budget has to scale with the shard. Observed rate at
    `--processes=6` was ~0.8s/id, so the default 2s/id is ~2.5x headroom.
    """
    return int(base + shard["ids"] * per_id)


def run_shard(shard: dict, binary: str, out_dir: str, processes: int, timeout: int) -> dict:
    """Run one shard as a subprocess; never raises on a failing shard."""
    report_path = shard_report_path(out_dir, shard)
    log_path = os.path.splitext(report_path)[0] + ".log"
    argv = [
        sys.executable,
        os.path.join(TESTS_ROOT, "run_smoke.py"),
        f"--binary={binary}",
        f"--log-wptreport={report_path}",
        "--no-manifest-update",
    ]
    if processes:
        argv.append(f"--processes={processes}")
    argv.extend(shard["test_ids"] if shard.get("test_ids") else [shard["prefix"]])

    started = time.time()
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, cwd=REPO_ROOT)
        try:
            returncode = proc.wait(timeout=timeout)
            outcome = "ran"
        except subprocess.TimeoutExpired:
            kill_tree(proc)
            proc.wait()
            returncode = None
            outcome = "timeout"
    elapsed = time.time() - started

    # A killed shard leaves the empty file wptrunner opened up front; it would
    # otherwise show up as an unreadable report at aggregation time.
    if outcome == "timeout" and os.path.isfile(report_path) and os.path.getsize(report_path) == 0:
        os.remove(report_path)

    if outcome == "ran" and not os.path.isfile(report_path):
        # wptrunner exited without writing a report — a crash during startup
        # (port conflict, missing cert, bad filter). Distinguished from a run
        # that legitimately found no tests, which does write an empty report.
        outcome = "no-report"
    return {"name": shard["name"], "prefix": shard["prefix"], "ids": shard["ids"],
            "outcome": outcome, "returncode": returncode, "seconds": round(elapsed, 1),
            "report": os.path.relpath(report_path, REPO_ROOT) if os.path.isfile(report_path) else None}


def score_reports(manifest: dict, out_dir: str, scope: set = None) -> dict:
    """Score every automatable manifest id against whatever the shards produced.

    Ids with no result score 0 — that is the whole point of scoring against the
    manifest rather than against the reports.

    `scope` limits the denominator to the categories that were actually part of
    this run. Without it a 10-category pilot scores itself against all 273
    categories and reports 0.58%, which is arithmetically true and completely
    misleading: the other 263 categories were never asked to run. A full run
    passes `scope=None` and gets the whole corpus, which is the real number.
    """
    expected = {}
    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        if scope is not None and category not in scope:
            continue
        expected[test_id] = {"type": test_type, "category": category}

    results = {}
    for entry in sorted(os.listdir(out_dir)):
        if not entry.endswith(".json") or entry == "state.json":
            continue
        with open(os.path.join(out_dir, entry), encoding="utf-8") as fh:
            try:
                report = json.load(fh)
            except json.JSONDecodeError:
                print(f"warning: unreadable report, ignored: {entry}", file=sys.stderr)
                continue
        for result in report.get("results", []):
            results[result["test"]] = result

    per_category = {}
    totals = {"ids": 0, "score": 0.0, "ran": 0, "not_run": 0, "harness_ok": 0,
              "subtests_total": 0, "subtests_passed": 0}
    status_counts = {}

    for test_id, meta in expected.items():
        category = meta["category"]
        row = per_category.setdefault(category, {
            "ids": 0, "score": 0.0, "ran": 0, "not_run": 0, "harness_ok": 0,
            "subtests_total": 0, "subtests_passed": 0, "by_type": {}})
        row["ids"] += 1
        totals["ids"] += 1
        row["by_type"][meta["type"]] = row["by_type"].get(meta["type"], 0) + 1

        result = results.get(test_id)
        if result is None:
            row["not_run"] += 1
            totals["not_run"] += 1
            status_counts["NOT-RUN"] = status_counts.get("NOT-RUN", 0) + 1
            continue

        row["ran"] += 1
        totals["ran"] += 1
        status = result.get("status", "")
        status_counts[status] = status_counts.get(status, 0) + 1
        if status in HARNESS_OK:
            row["harness_ok"] += 1
            totals["harness_ok"] += 1

        subtests = result.get("subtests") or []
        if subtests:
            passed = sum(1 for s in subtests if s.get("status") == "PASS")
            row["subtests_total"] += len(subtests)
            row["subtests_passed"] += passed
            totals["subtests_total"] += len(subtests)
            totals["subtests_passed"] += passed
            score = passed / len(subtests)
        else:
            score = 1.0 if status == "PASS" else 0.0
        row["score"] += score
        totals["score"] += score

    for row in per_category.values():
        row["score"] = round(row["score"], 2)
        row["pass_rate"] = round(row["score"] / row["ids"], 4) if row["ids"] else 0.0
    totals["score"] = round(totals["score"], 2)
    totals["pass_rate"] = round(totals["score"] / totals["ids"], 4) if totals["ids"] else 0.0

    return {"totals": totals, "status_counts": status_counts, "per_category": per_category}


def print_summary(scored: dict, shard_states: list) -> None:
    totals = scored["totals"]
    print()
    print("=" * 72)
    print(f"pass-rate: {totals['pass_rate'] * 100:.2f}%  "
          f"({totals['score']:.0f} of {totals['ids']} automatable manifest ids)")
    print(f"  ran:              {totals['ran']}")
    print(f"  never ran:        {totals['not_run']}  (scored 0 — no executor, skipped, or lost shard)")
    print(f"  harness OK:       {totals['harness_ok']}")
    print(f"  subtests:         {totals['subtests_passed']}/{totals['subtests_total']} passed")
    print("  statuses:         " + ", ".join(f"{k}={v}" for k, v in sorted(scored["status_counts"].items())))
    bad = [s for s in shard_states if s["outcome"] != "ran"]
    if bad:
        print(f"  PROBLEM SHARDS:   {len(bad)} — " + ", ".join(f"{s['name']}({s['outcome']})" for s in bad[:8]))
        if len(bad) > 8:
            print(f"                    ... and {len(bad) - 8} more, see state.json")
    print("=" * 72)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=None, help="path to lumen.exe (default: target/$LUMEN_PROFILE/lumen.exe)")
    parser.add_argument("--all", action="store_true", help="run every category in the manifest")
    parser.add_argument("--pilot", action="store_true", help=f"run the pilot selection ({len(PILOT_CATEGORIES)} categories)")
    parser.add_argument("--categories", default=None, help="comma-separated category list")
    parser.add_argument("--processes", type=int, default=6, help="wptrunner --processes per shard (default: 6)")
    parser.add_argument("--out-dir", default=DEFAULT_OUT_DIR)
    parser.add_argument("--shard-timeout-base", type=int, default=600, help="fixed part of a shard's time budget, seconds (default: 600)")
    parser.add_argument("--shard-timeout-per-id", type=float, default=2.0, help="per-id part of a shard's time budget, seconds (default: 2.0)")
    parser.add_argument("--resume", action="store_true", help="skip shards that already produced a report")
    parser.add_argument("--aggregate-only", action="store_true", help="score existing reports in --out-dir, run nothing")
    parser.add_argument("--skip-manifest-update", action="store_true", help="trust MANIFEST.json as-is")
    parser.add_argument("--run-json", default=None, help="write the scored run snapshot here (docs/wpt/runs/<date>.json)")
    args = parser.parse_args()

    binary = args.binary or os.path.join(REPO_ROOT, "target", os.environ.get("LUMEN_PROFILE", "release"), "lumen.exe")
    os.makedirs(args.out_dir, exist_ok=True)
    state_path = os.path.join(args.out_dir, "state.json")

    if not args.aggregate_only:
        if not os.path.isfile(binary):
            print(f"lumen binary not found: {binary}", file=sys.stderr)
            return 1
        if not args.skip_manifest_update:
            update_manifest()

    manifest = load_manifest()

    if args.aggregate_only:
        shard_states = json.load(open(state_path, encoding="utf-8"))["shards"] if os.path.isfile(state_path) else []
    else:
        if args.all:
            categories = sorted({c for _t, c, _i in corpus_stats.iter_ids(manifest)})
        elif args.pilot:
            categories = list(PILOT_CATEGORIES)
        elif args.categories:
            categories = [c.strip() for c in args.categories.split(",") if c.strip()]
        else:
            print("pick a selection: --all, --pilot or --categories", file=sys.stderr)
            return 1

        shards = plan_shards(manifest, categories)
        print(f"{len(shards)} shards, {sum(s['ids'] for s in shards)} manifest ids, "
              f"--processes={args.processes}", flush=True)

        shard_states = []
        if args.resume and os.path.isfile(state_path):
            with open(state_path, encoding="utf-8") as fh:
                shard_states = [s for s in json.load(fh)["shards"] if s["outcome"] == "ran"]
        done = {s["name"] for s in shard_states}

        for index, shard in enumerate(shards, 1):
            if shard["name"] in done:
                print(f"[{index}/{len(shards)}] {shard['name']}: cached", flush=True)
                continue
            budget = shard_timeout(shard, args.shard_timeout_base, args.shard_timeout_per_id)
            print(f"[{index}/{len(shards)}] {shard['name']}: {shard['ids']} ids (budget {budget}s) ...", end="", flush=True)
            state = run_shard(shard, binary, args.out_dir, args.processes, budget)
            shard_states.append(state)
            print(f" {state['outcome']} in {state['seconds']}s", flush=True)
            # Checkpoint after every shard: a corpus run outlives the session
            # that started it, and must be resumable from wherever it stopped.
            with open(state_path, "w", encoding="utf-8") as fh:
                json.dump({"binary": binary, "shards": shard_states}, fh, indent=2)

    # A run only gets to be scored against what it actually covered. The scope
    # is derived from the shards, not from the CLI selection, so a resumed or
    # aggregate-only run reports against the same denominator as the run that
    # produced the shards.
    scope = {s["name"].split(" ")[0].split("/")[0] for s in shard_states} or None
    all_categories = {c for _t, c, _i in corpus_stats.iter_ids(manifest)}
    if scope and scope >= all_categories:
        scope = None
    scored = score_reports(manifest, args.out_dir, scope)
    if scope:
        print(f"\nscope: {len(scope)} of {len(all_categories)} categories "
              f"(partial run — denominator covers only what was selected)")
    print_summary(scored, shard_states)

    if args.run_json:
        os.makedirs(os.path.dirname(os.path.abspath(args.run_json)), exist_ok=True)
        snapshot = {
            "binary": binary,
            # A pass-rate without the build it came from cannot be compared to
            # the next one — two snapshots differing by a commit look exactly
            # like two snapshots differing by an engine change.
            "commit": _git_head(),
            "finished": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "processes": args.processes,
            "scope": sorted(scope) if scope else "full-corpus",
            "shards": shard_states,
            "scored": scored,
        }
        with open(args.run_json, "w", encoding="utf-8") as fh:
            json.dump(snapshot, fh, indent=2, sort_keys=True)
        print(f"run snapshot: {args.run_json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
