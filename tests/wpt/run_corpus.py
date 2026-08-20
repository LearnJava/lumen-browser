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
        [--retry-timeouts] [--aggregate-only] [--run-json PATH]
"""

import argparse
import json
import os
import signal
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

#: Suffix of the parallel mozlog stream each shard writes next to its report.
#: Named once because aggregation matches reports and streams by name.
RAW_SUFFIX = ".raw.jsonl"

#: Suffix a shard's previous mozlog stream is rotated to before the shard is
#: run again. mozlog opens `--log-raw` with mode `"w"` (`mozlog/commandline.py`),
#: so a retry truncates the stream *before* producing anything: without this
#: rotation a retry that gets less far than the first attempt permanently
#: destroys results the run already had. Aggregation reads both and lets the
#: newer stream win per id, so a retry can only ever add.
RAW_PREV_SUFFIX = RAW_SUFFIX + ".prev"

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


def _snapshot_commit(path: str | None) -> str | None:
    """Commit recorded by a previously written run snapshot, if it is readable.

    Used only as a fallback for `--aggregate-only` over a checkpoint that
    carries no commit: rescoring must not silently re-stamp a number with the
    checkout doing the scoring.
    """
    if not path or not os.path.isfile(path):
        return None
    try:
        with open(path, encoding="utf-8") as fh:
            commit = json.load(fh).get("commit")
    except (OSError, ValueError):
        return None
    if not isinstance(commit, str) or not commit or commit.startswith("unknown"):
        return None
    return commit


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

    A shard is `{"name", "prefix", "ids", "auto_ids"}` where `prefix` is what
    gets passed to wptrunner as a positional test filter. Categories under
    `SHARD_THRESHOLD` ids stay whole; larger ones split on their second path
    component so that no single `wptreport.json` — and no single timeout —
    covers more than a slice.

    A shard with `auto_ids == 0` is dropped: wptrunner's default test types
    exclude `manual`/`visual`, so a directory holding nothing else has no test
    wptrunner will even look at — it answers "Unable to find any tests at the
    path(s)" and leaves a zero-byte report. Corpus-wide that is 14 shards /
    260 ids (`appmanifest`, `css/CSS2/i18n`, `annotation-*`, …), each paying a
    full wptserve boot to produce nothing and then showing up in the summary's
    "ran nothing" line as if a filter had eaten them. They are not in the
    scored denominator either (`score_reports` skips the same two types), so
    dropping them changes no number, only the noise.

    `ids` deliberately stays the *full* id count, `manual`/`visual` included:
    it feeds `shard_timeout`, which budgets wall-clock, and the measured rate
    (3.0 s/id) is already above the 2.0 s/id default — the slack a
    manual-heavy directory carries is protective, and tightening it here would
    buy nothing but fresh budget kills.
    """
    by_category = {}
    automatable = set()
    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        by_category.setdefault(category, []).append(test_id)
        if test_type not in corpus_stats.NON_AUTOMATABLE_TYPES:
            automatable.add(test_id)

    shards = []
    dropped = 0
    for category in categories:
        ids = by_category.get(category)
        if not ids:
            print(f"warning: category not in manifest, skipped: {category}", file=sys.stderr)
            continue
        for shard in _split([category], ids, automatable):
            if shard["auto_ids"]:
                shards.append(shard)
            else:
                dropped += 1
    if dropped:
        print(f"{dropped} shards hold only manual/visual tests — not planned "
              f"(wptrunner runs neither; they are not in the denominator)", file=sys.stderr)
    return shards


def _split(prefix_parts: list, ids: list, automatable: set) -> list:
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
        return [_shard(name, f"/{name}/", ids, automatable)]

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
        return [_shard(name, f"/{name}/", ids, automatable)]

    shards = []
    for key, group_ids in sorted(groups.items()):
        if not key:
            # Files sitting directly in a directory that also has subdirectories
            # cannot be addressed by prefix — `/css/CSS2/` would re-select every
            # subdirectory we just split out, running them twice. Corpus-wide
            # this is 5 such nodes totalling ~100 ids, so listing them
            # explicitly is both exact and short enough for a command line.
            shard = _shard(f"{name} (bare)", None, group_ids, automatable)
            shard["test_ids"] = sorted(group_ids)
            shards.append(shard)
        else:
            shards.extend(_split(prefix_parts + [key], group_ids, automatable))
    return shards


def _shard(name: str, prefix, ids: list, automatable: set) -> dict:
    """One shard record: full id count for the budget, automatable count for
    the decision whether the shard is worth running at all."""
    return {"name": name, "prefix": prefix, "ids": len(ids),
            "auto_ids": sum(1 for test_id in ids if test_id in automatable)}


def shard_report_path(out_dir: str, shard: dict) -> str:
    return os.path.join(out_dir, shard["name"].replace("/", "__") + ".json")


def _descendant_pids(root: int) -> list:
    """Every PID in the tree rooted at `root` (root included), via `ps -eo pid,ppid`.

    Portable across Linux/macOS, unlike `/proc` (Linux-only). A one-shot
    snapshot, not a live walk — fine here since the tree is about to be killed,
    not inspected repeatedly.
    """
    try:
        out = subprocess.run(["ps", "-eo", "pid,ppid"], capture_output=True,
                             text=True, check=False).stdout
    except OSError:
        return [root]
    children = {}
    for line in out.splitlines()[1:]:
        parts = line.split()
        if len(parts) != 2:
            continue
        try:
            pid, ppid = int(parts[0]), int(parts[1])
        except ValueError:
            continue
        children.setdefault(ppid, []).append(pid)

    result = []
    stack = [root]
    while stack:
        pid = stack.pop()
        result.append(pid)
        stack.extend(children.get(pid, []))
    return result


def kill_tree(proc) -> None:
    """Kill the shard subprocess *and* the browser processes it spawned.

    `Popen.kill()` only reaps the Python child; every `lumen` wptrunner
    started stays alive and keeps holding its BiDi port, which then breaks the
    next shard. Killing by PID tree (never by image name — that would take out
    unrelated browser windows, including another session's).

    On POSIX the tree is *not* a process group: `run_shard` spawning
    `run_smoke.py` with `start_new_session=True` only makes `run_smoke.py`
    itself a group leader — wptrunner then puts each `lumen` it launches in
    its *own* group (verified: PGID == PID on every orphaned `lumen`, not
    `run_smoke.py`'s), so `os.killpg` on `run_smoke.py`'s group misses them
    all. Confirmed the hard way running WPT-RUN-5 on Linux: a WebCryptoAPI
    timeout left 6 `lumen` processes running past their shard, which then
    piled up under `accelerometer`'s own 6 and pushed the machine (7.6 GB RAM)
    into OOM territory, killing `run_corpus.py` itself. Walking the real PPID
    tree (what `taskkill /F /T` does on Windows) is the fix that actually
    reaches them, since they're still direct children of `run_smoke.py`'s PID
    by the kernel's own bookkeeping regardless of which group they sit in.
    """
    if os.name == "nt":
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                       capture_output=True, check=False)
    else:
        for pid in reversed(_descendant_pids(proc.pid)):
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def shard_timeout(shard: dict, base: int, per_id: float) -> int:
    """Budget a shard's wall-clock by its size, not by a flat constant.

    WPT-RUN-4 pilot: a flat 1200s killed `encoding` (1343 ids) mid-run while
    being ten times more than `FileAPI` (125 ids) ever needed. A killed shard
    loses everything it had done — `wptreport.json` is only written at the end —
    so the budget has to scale with the shard. Observed rate at
    `--processes=6` was ~0.8s/id, so the default 2s/id is ~2.5x headroom.
    """
    return int(base + shard["ids"] * per_id)


def https_ids(manifest: dict, scope: set = None) -> list:
    """Every `.https.` test id.

    BUG-785 (fixed 2026-08-20) made these unreachable at the TLS layer,
    whatever the engine did above it — `--skip-https` below dates from when
    that was still true unconditionally. Now `LUMEN_EXTRA_CA_CERT` lets the
    browser trust WPT's test CA, so these ids run for real; the flag stays as
    an opt-in for a fast/partial run, not because the ids are still
    unreachable by construction.

    Kept as its own function because these ids stay in the *denominator* while
    being skippable in the *run*: `--skip-https` trades observation for hours
    of wall-clock, and must never quietly shrink what the pass-rate divides by.
    """
    return sorted({i for t, c, i in corpus_stats.iter_ids(manifest)
                   if ".https." in i
                   and t not in corpus_stats.NON_AUTOMATABLE_TYPES
                   and (scope is None or c in scope)})


def run_shard(shard: dict, binary: str, out_dir: str, processes: int, timeout: int,
              exclude_file: str = None) -> dict:
    """Run one shard as a subprocess; never raises on a failing shard."""
    report_path = shard_report_path(out_dir, shard)
    log_path = os.path.splitext(report_path)[0] + ".log"
    raw_path = os.path.splitext(report_path)[0] + RAW_SUFFIX
    argv = [
        sys.executable,
        os.path.join(TESTS_ROOT, "run_smoke.py"),
        f"--binary={binary}",
        f"--log-wptreport={report_path}",
        # `--log-wptreport` is written once, at the end. A shard killed on its
        # time budget (pilot: `encoding`, `WebCryptoAPI`) therefore lost every
        # result it had already produced — 1656 ids silently became NOT-RUN.
        # mozlog's raw stream is written per event, so it survives the kill and
        # `results_from_raw_log` reconstructs whatever finished.
        f"--log-raw={raw_path}",
        "--no-manifest-update",
    ]
    if exclude_file:
        argv.append(f"--exclude-file={exclude_file}")
    if processes:
        argv.append(f"--processes={processes}")
    argv.extend(shard["test_ids"] if shard.get("test_ids") else [shard["prefix"]])

    # Rotate whatever the previous attempt salvaged out of the way — see
    # RAW_PREV_SUFFIX. Only a non-empty stream is worth keeping, and only one
    # generation: the older it gets the less it can add over the newer runs.
    prev_path = os.path.splitext(report_path)[0] + RAW_PREV_SUFFIX
    if os.path.isfile(raw_path) and os.path.getsize(raw_path) > 0:
        os.replace(raw_path, prev_path)

    started = time.time()
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, cwd=REPO_ROOT,
                                 start_new_session=(os.name != "nt"))
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
            "auto_ids": shard.get("auto_ids"),
            "outcome": outcome, "returncode": returncode, "seconds": round(elapsed, 1),
            "report": os.path.relpath(report_path, REPO_ROOT) if os.path.isfile(report_path) else None}


def results_from_raw_log(raw_path: str) -> dict:
    """Rebuild `{test_id: result}` from a mozlog raw stream.

    Used when a shard produced no `wptreport.json` (killed on its time budget,
    crashed, or hung): the raw stream is one JSON object per line, flushed as
    events happen, so everything up to the kill is still there. Emits the same
    shape `wptreport.json` does, so the scorer cannot tell the two apart.

    A truncated final line is expected — the process was killed mid-write — and
    is skipped rather than treated as corruption.
    """
    results = {}
    with open(raw_path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            action = event.get("action")
            test_id = event.get("test")
            if not test_id:
                continue
            if action == "test_status":
                entry = results.setdefault(test_id, {"test": test_id, "status": None, "subtests": []})
                entry["subtests"].append({"name": event.get("subtest", ""),
                                          "status": event.get("status", "")})
            elif action == "test_end":
                entry = results.setdefault(test_id, {"test": test_id, "status": None, "subtests": []})
                entry["status"] = event.get("status", "")
    # A test that started but never ended (the one the kill interrupted) has no
    # status — drop it rather than scoring a half-observed test as anything.
    return {k: v for k, v in results.items() if v["status"]}


def rescue_results(raw_path: str) -> dict:
    """Rebuild `{test_id: result}` from a shard's raw stream *and its rotated
    predecessor* (`RAW_PREV_SUFFIX`), newer winning per id.

    A retried shard is not guaranteed to get as far as the attempt before it —
    the budget is wall-clock, so a busier machine salvages fewer ids. Reading
    both generations makes a retry monotone: it can add results, never remove
    them. Everything else in the pipeline calls this rather than
    `results_from_raw_log` directly, so the two attempts are indistinguishable
    from one longer one at scoring time.
    """
    prev_path = raw_path + ".prev" if raw_path.endswith(RAW_SUFFIX) else None
    merged = {}
    if prev_path and os.path.isfile(prev_path):
        merged.update(results_from_raw_log(prev_path))
    if os.path.isfile(raw_path):
        merged.update(results_from_raw_log(raw_path))
    return merged


def load_results(out_dir: str) -> tuple:
    """Load every shard's results, falling back to the raw stream per shard.

    Returns `(results, recovered, empty)`. `recovered` names shards whose
    numbers came from the raw stream, so the summary can say so out loud
    instead of quietly reporting a partial shard as if it were complete.
    A shard killed on its time budget has **no** `wptreport.json` at all
    (`run_shard` deletes the zero-byte file wptrunner opened up front), so the
    raw streams are enumerated in their own right, not merely as a fallback for
    a report that exists — otherwise the shards that most need rescuing, the
    killed ones, are the exact shards the rescue never sees.
    `empty` names shards that legitimately ran nothing — with `--skip-https`
    a category can have every one of its tests excluded, and wptrunner then
    leaves a zero-byte report. That is not a lost shard, and must not be
    reported as one: a run where "recovered" fires on healthy shards trains
    the reader to ignore the line that matters.
    """
    results = {}
    recovered = []
    empty = []
    entries = sorted(os.listdir(out_dir))
    reports = [e for e in entries if e.endswith(".json") and e != "state.json"]
    have_report = set(reports)
    raw_only = [e for e in entries if e.endswith(RAW_SUFFIX)
                and e[: -len(RAW_SUFFIX)] + ".json" not in have_report]
    for entry in reports:
        path = os.path.join(out_dir, entry)
        try:
            with open(path, encoding="utf-8") as fh:
                report = json.load(fh)
            for result in report.get("results", []):
                results[result["test"]] = result
            continue
        except (json.JSONDecodeError, OSError):
            pass

        raw_path = os.path.splitext(path)[0] + RAW_SUFFIX
        rescued = rescue_results(raw_path)
        if rescued:
            results.update(rescued)
            recovered.append((entry[:-5], len(rescued)))
        elif os.path.getsize(path) == 0:
            # Zero-byte report + nothing in the raw stream: wptrunner selected
            # no tests at all (everything excluded or filtered out).
            empty.append(entry[:-5])
        else:
            print(f"warning: unreadable report and no raw log, ignored: {entry}", file=sys.stderr)

    for entry in raw_only:
        name = entry[: -len(RAW_SUFFIX)]
        rescued = rescue_results(os.path.join(out_dir, entry))
        if rescued:
            results.update(rescued)
            recovered.append((name, len(rescued)))
    return results, recovered, empty


def resumable_states(previous: list, out_dir: str, retry_timeouts: bool) -> list:
    """Decide which shards of a previous run `--resume` must not run again.

    `ran` is obvious. The interesting case is `timeout` — a shard killed on its
    wall-clock budget. Replaying it costs the *whole* budget a second time and
    buys nothing: the budget is the same, the machine is no faster, and the
    shard dies at the same wall. Measured on the 2026-08-20 Linux corpus run,
    that was 50 minutes per resume for three shards (`WebCryptoAPI`, `ai`,
    `bluetooth`) whose results had already been salvaged from the raw stream —
    and every resume paid it again, because the old filter kept only `ran`.

    So a killed shard that salvaged something is treated as done, and the run
    says so out loud rather than reporting it as complete. Two deliberate
    exceptions:

    * a killed shard that salvaged **nothing** is retried — there is no partial
      result to protect and one more attempt may be all it needs;
    * `--retry-timeouts` retries them all, which is what a resume with a raised
      `--shard-timeout-per-id` wants (`WPT-RUN-9`). That path is safe because
      `run_shard` rotates the raw stream first, so a shorter second attempt
      cannot destroy the first one's results.

    Any other outcome (`no-report` — wptrunner died at startup) is always
    retried: it failed fast, so retrying is cheap, and it produced nothing.
    """
    states, kept, retried = [], [], []
    for state in previous:
        if state["outcome"] == "ran":
            states.append(state)
            continue
        if state["outcome"] == "timeout" and not retry_timeouts:
            raw_path = os.path.splitext(shard_report_path(out_dir, state))[0] + RAW_SUFFIX
            if rescue_results(raw_path):
                states.append(state)
                kept.append(state["name"])
                continue
        retried.append(state["name"])
    if kept:
        print(f"--resume: {len(kept)} budget-killed shard(s) kept partial rather than replayed "
              f"({', '.join(kept)}) — pass --retry-timeouts to run them again", flush=True)
    if retried:
        print(f"--resume: {len(retried)} shard(s) will run again ({', '.join(retried[:8])})", flush=True)
    return states


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

    results, recovered, empty_shards = load_results(out_dir)

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

    return {"totals": totals, "status_counts": status_counts, "per_category": per_category,
            "recovered_shards": [{"shard": name, "results": n} for name, n in recovered],
            "empty_shards": empty_shards}


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
    empty = scored.get("empty_shards", [])
    if empty:
        print(f"  ran nothing:      {len(empty)} shards had every test excluded/filtered "
              f"(not a loss — an all-https category under --skip-https, or, in an out-dir "
              f"written before manual/visual-only shards stopped being planned, a directory "
              f"wptrunner has no runnable test type for)")
    for entry in scored.get("recovered_shards", []):
        print(f"  RECOVERED:        {entry['shard']} — {entry['results']} results salvaged "
              f"from the raw log (shard did not finish)")
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
    parser.add_argument("--retry-timeouts", action="store_true",
                        help="on --resume, run budget-killed shards again instead of keeping "
                             "the results salvaged from their raw stream (use together with a "
                             "raised --shard-timeout-per-id; the previous stream is rotated, "
                             "so a shorter retry cannot lose results)")
    parser.add_argument("--aggregate-only", action="store_true", help="score existing reports in --out-dir, run nothing")
    parser.add_argument("--skip-manifest-update", action="store_true", help="trust MANIFEST.json as-is")
    parser.add_argument("--run-json", default=None, help="write the scored run snapshot here (docs/wpt/runs/<date>.json)")
    parser.add_argument("--skip-https", action="store_true",
                        help="do not run .https. tests (BUG-785 fixed 2026-08-20 — this now just "
                             "trades a slower, complete run for a faster, partial one); "
                             "they stay in the denominator and score 0, the summary says how many")
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

    # Provenance of an aggregate-only score belongs to the run that produced
    # the shards, not to the checkout that happens to be scoring them: the
    # binary and the commit are read back from the checkpoint. Inventing them
    # here would stamp a number with a build that never ran a single test —
    # exactly the comparison trap `--run-json` carries the fields to prevent.
    run_commit = _git_head()
    if args.aggregate_only:
        checkpoint = json.load(open(state_path, encoding="utf-8")) if os.path.isfile(state_path) else {}
        shard_states = checkpoint.get("shards", [])
        # Same reason as the commit: with no checkpoint there is no build to
        # name, and the CLI default names one that never ran anything.
        binary = args.binary or checkpoint.get("binary") or "unknown"
        if checkpoint.get("commit"):
            run_commit = checkpoint["commit"]
        else:
            # Checkpoint predates commit recording (or was written by a run
            # still in flight under the old code). The snapshot being
            # overwritten was written by that run itself, so its commit is the
            # real one — take it before it is clobbered. Failing that, say
            # "unknown" instead of passing the scoring checkout off as the one
            # that ran the tests.
            inherited = _snapshot_commit(args.run_json)
            if inherited:
                print(f"note: {state_path} records no commit; inheriting "
                      f"{inherited} from {args.run_json}", file=sys.stderr)
                run_commit = inherited
            else:
                print(f"warning: {state_path} records no commit; snapshot will say "
                      f"'unknown (scored at {run_commit})'", file=sys.stderr)
                run_commit = f"unknown (scored at {run_commit})"
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
        print(f"{len(shards)} shards, {sum(s['ids'] for s in shards)} manifest ids "
              f"({sum(s['auto_ids'] for s in shards)} automatable — the scored denominator), "
              f"--processes={args.processes}", flush=True)

        exclude_file = None
        if args.skip_https:
            skipped = https_ids(manifest, set(categories))
            exclude_file = os.path.join(args.out_dir, "exclude-https.txt")
            with open(exclude_file, "w", encoding="utf-8") as fh:
                fh.write("\n".join(skipped) + "\n")
            print(f"--skip-https: {len(skipped)} ids excluded from the run "
                  f"(still in the denominator, scored 0)", flush=True)

        shard_states = []
        if args.resume and os.path.isfile(state_path):
            with open(state_path, encoding="utf-8") as fh:
                previous = json.load(fh)["shards"]
            shard_states = resumable_states(previous, args.out_dir, args.retry_timeouts)
        done = {s["name"] for s in shard_states}

        for index, shard in enumerate(shards, 1):
            if shard["name"] in done:
                print(f"[{index}/{len(shards)}] {shard['name']}: cached", flush=True)
                continue
            budget = shard_timeout(shard, args.shard_timeout_base, args.shard_timeout_per_id)
            print(f"[{index}/{len(shards)}] {shard['name']}: {shard['ids']} ids (budget {budget}s) ...", end="", flush=True)
            state = run_shard(shard, binary, args.out_dir, args.processes, budget, exclude_file)
            shard_states.append(state)
            print(f" {state['outcome']} in {state['seconds']}s", flush=True)
            # Checkpoint after every shard: a corpus run outlives the session
            # that started it, and must be resumable from wherever it stopped.
            with open(state_path, "w", encoding="utf-8") as fh:
                json.dump({"binary": binary, "commit": run_commit, "shards": shard_states,
                           "skipped_https": len(skipped) if args.skip_https else 0}, fh, indent=2)

    # A run only gets to be scored against what it actually covered. The scope
    # is derived from the shards, not from the CLI selection, so a resumed or
    # aggregate-only run reports against the same denominator as the run that
    # produced the shards.
    scope = {s["name"].split(" ")[0].split("/")[0] for s in shard_states} or None
    # Only categories that hold at least one automatable test can ever be
    # planned (`plan_shards` drops the rest) or scored (`score_reports` skips
    # `manual`/`visual`), so the "is this a full run" comparison has to use the
    # same 266 — against all 273 a genuinely full run would forever report
    # itself as partial.
    all_categories = {c for t, c, _i in corpus_stats.iter_ids(manifest)
                      if t not in corpus_stats.NON_AUTOMATABLE_TYPES}
    if scope and scope >= all_categories:
        scope = None
    scored = score_reports(manifest, args.out_dir, scope)
    if scope:
        print(f"\nscope: {len(scope)} of {len(all_categories)} categories "
              f"(partial run — denominator covers only what was selected)")
    print_summary(scored, shard_states)

    # No silent caps: an intentionally unrun slice must be named in the same
    # breath as the number it depresses, or the reader takes 0 for "measured".
    skipped_count = 0
    if os.path.isfile(state_path):
        with open(state_path, encoding="utf-8") as fh:
            skipped_count = json.load(fh).get("skipped_https", 0)
    if skipped_count:
        print(f"  NOT RUN ON PURPOSE: {skipped_count} .https. ids excluded by --skip-https, "
              f"scored 0")

    if args.run_json:
        os.makedirs(os.path.dirname(os.path.abspath(args.run_json)), exist_ok=True)
        snapshot = {
            "binary": binary,
            # A pass-rate without the build it came from cannot be compared to
            # the next one — two snapshots differing by a commit look exactly
            # like two snapshots differing by an engine change.
            "commit": run_commit,
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
