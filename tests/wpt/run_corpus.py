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
import contextlib
import io
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
METADATA_ROOT = os.path.join(TESTS_ROOT, "metadata")
MANIFEST_PATH = os.path.join(METADATA_ROOT, "MANIFEST.json")
DEFAULT_OUT_DIR = os.path.join(REPO_ROOT, ".tmp", "wpt-corpus")

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import browser_rss_cap  # noqa: E402
import corpus_stats  # noqa: E402
import heavy_lock  # noqa: E402
import port_guard  # noqa: E402

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


#: `wptrunner`'s own per-test wall-clock ceilings at `timeout_multiplier=1`:
#: a test the manifest marks `timeout: long` gets 60 s, every other test 10 s,
#: and the executor's deadline is `timeout + extra_timeout` on top
#: (`executors/base.py::extra_timeout = 5`, applied in `executorlumen.py`).
#: 8 196 of the corpus's ids are `long` (WPT-RUN-5 slice 15) and they are not
#: spread evenly — whole categories are 96-99 % `long`, which is what makes a
#: flat per-id budget kill them. The sum of the three is what a hung test
#: actually costs, measured: 15.08 s median for a short one, 65.29 s for a
#: long one, against 15 s and 65 s predicted here.
TIMEOUT_SHORT = 10.0
TIMEOUT_LONG = 60.0
TIMEOUT_EXTRA = 5.0

#: Headroom over the summed ceilings, for imperfect parallelism (measured
#: effective 5.8 of 6 processes) — not for per-test overshoot, which
#: `TIMEOUT_EXTRA` already accounts for exactly.
BUDGET_SLACK = 1.15

#: How much wider this code's budget has to be than the wall a killed shard
#: actually died at before `--resume` calls the difference out. A resumed
#: out-dir can carry shards killed under an older rule — the 2026-08-20 Linux
#: half was started before slice 15 replaced the flat `600 + 2*ids` budget with
#: the derived one, so its seven killed shards would now get 1.6-3.9x the wall
#: they died at (WPT-RUN-5 slice 28). Below this ratio the difference is noise
#: (rounding, a differing `--processes`) and saying so would be wrong.
BUDGET_WIDENED = 1.1


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


def parse_prefixes(text) -> list:
    """`"css/css-flexbox, /css/css-grid/"` -> `["css/css-flexbox", "css/css-grid"]`.

    Path prefixes relative to the WPT root, no leading or trailing slash. An
    empty or `None` input is the empty list, i.e. "no filter".
    """
    if not text:
        return []
    return [p.strip().strip("/") for p in text.split(",") if p.strip().strip("/")]


def _under(test_id: str, prefix: str) -> bool:
    """Whether `test_id` is the path `prefix` or lies below it (a `?variant` of
    the file counts too). `prefix` is a `parse_prefixes` entry."""
    path = test_id.lstrip("/")
    return path == prefix or path.startswith((prefix + "/", prefix + "?"))


def id_selected(test_id: str, prefixes: list, exclude_prefixes: list) -> bool:
    """The `--prefixes` / `--exclude-prefixes` filter: no `prefixes` means
    everything; an exclusion always wins over an inclusion."""
    if exclude_prefixes and any(_under(test_id, p) for p in exclude_prefixes):
        return False
    return not prefixes or any(_under(test_id, p) for p in prefixes)


def plan_shards(manifest: dict, categories: list, prefixes: list = None,
                exclude_prefixes: list = None) -> list:
    """Split the selected categories into runnable shards.

    `prefixes` / `exclude_prefixes` (WPT-RUN-14) narrow the plan to a part of a
    category — `css/css-flexbox` out of the 34 607-id `css`. The category is
    still split exactly as without the filter, so shard names stay the ones a
    full run would produce; a shard the filter only partly covers is then
    re-cut along directories (`_narrow`) into pieces that are wholly inside the
    selection, because wptrunner can only be pointed at a path prefix or an
    explicit id list. Filtering the ids *before* the split would be wrong: a
    selection of 1 400 ids would plan one shard named `css` with prefix `/css/`
    and quietly run all 34 607.

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
    it is what the summary and `state.json` report a shard's size as. The time
    budget no longer comes from it — `shard_timeout` sums the declared per-test
    ceilings of `auto_ids`/`long_ids` instead, which is both tighter (a
    manual-heavy directory stops carrying phantom slack) and safer (a
    long-heavy one stops being under-budgeted); see WPT-RUN-5 slice 15.
    """
    by_category = {}
    automatable = set()
    long_tests = set()
    for test_type, category, test_id, extras in corpus_stats.iter_entries(manifest):
        by_category.setdefault(category, []).append(test_id)
        if test_type not in corpus_stats.NON_AUTOMATABLE_TYPES:
            automatable.add(test_id)
            if extras.get("timeout") == "long":
                long_tests.add(test_id)

    shards = []
    dropped = 0
    filtered = bool(prefixes or exclude_prefixes)
    for category in categories:
        ids = by_category.get(category)
        if not ids:
            print(f"warning: category not in manifest, skipped: {category}", file=sys.stderr)
            continue
        planned = _split([category], ids, automatable, long_tests)
        if filtered:
            selected = {i for i in ids if id_selected(i, prefixes or [], exclude_prefixes or [])}
            planned = [piece for shard in planned
                       for piece in _narrow(shard, ids, selected, automatable, long_tests)]
        for shard in planned:
            if shard["auto_ids"]:
                shards.append(shard)
            else:
                dropped += 1
    if dropped:
        print(f"{dropped} shards hold only manual/visual tests — not planned "
              f"(wptrunner runs neither; they are not in the denominator)", file=sys.stderr)
    return shards


def _narrow(shard: dict, category_ids: list, selected: set, automatable: set,
            long_tests: set) -> list:
    """Cut one planned shard down to the part of it `selected` covers.

    A shard wholly inside the selection comes back unchanged (same name, so a
    filtered run's shards are a subset of the full run's). A shard wholly
    outside comes back as nothing. A partly covered one is re-cut by directory:
    a directory every id of which is selected becomes one prefix shard, a mixed
    directory is descended into, and the files lying directly in a mixed
    directory become one explicit-id `(bare)` shard (they cannot be addressed by
    prefix without re-selecting their subdirectories).
    """
    if shard.get("test_ids"):
        members = shard["test_ids"]
    else:
        members = [i for i in category_ids if i.startswith(shard["prefix"])]
    chosen = [i for i in members if i in selected]
    if not chosen:
        return []
    if len(chosen) == len(members):
        return [shard]
    if shard.get("test_ids"):
        piece = _shard(shard["name"], None, chosen, automatable, long_tests)
        piece["test_ids"] = sorted(chosen)
        return [piece]
    return _cover(shard["prefix"].strip("/").split("/"), members, selected,
                  automatable, long_tests)


def _cover(parts: list, ids: list, selected: set, automatable: set, long_tests: set) -> list:
    """Exact cover of `selected ∩ ids` by prefix shards and one bare shard per
    mixed directory — see `_narrow`."""
    chosen = [i for i in ids if i in selected]
    if not chosen:
        return []
    name = "/".join(parts)
    if len(chosen) == len(ids):
        return [_shard(name, f"/{name}/", ids, automatable, long_tests)]
    depth = len(parts)
    groups = {}
    for test_id in ids:
        segs = test_id.strip("/").split("/")
        groups.setdefault(segs[depth] if len(segs) > depth + 1 else "", []).append(test_id)
    shards = []
    for key, group in sorted(groups.items()):
        if key:
            shards.extend(_cover(parts + [key], group, selected, automatable, long_tests))
        else:
            direct = sorted(i for i in group if i in selected)
            if direct:
                piece = _shard(f"{name} (bare)", None, direct, automatable, long_tests)
                piece["test_ids"] = direct
                shards.append(piece)
    return shards


def _split(prefix_parts: list, ids: list, automatable: set, long_tests: set) -> list:
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
        return [_shard(name, f"/{name}/", ids, automatable, long_tests)]

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
        return [_shard(name, f"/{name}/", ids, automatable, long_tests)]

    shards = []
    for key, group_ids in sorted(groups.items()):
        if not key:
            # Files sitting directly in a directory that also has subdirectories
            # cannot be addressed by prefix — `/css/CSS2/` would re-select every
            # subdirectory we just split out, running them twice. Corpus-wide
            # this is 5 such nodes totalling ~100 ids, so listing them
            # explicitly is both exact and short enough for a command line.
            shard = _shard(f"{name} (bare)", None, group_ids, automatable, long_tests)
            shard["test_ids"] = sorted(group_ids)
            shards.append(shard)
        else:
            shards.extend(_split(prefix_parts + [key], group_ids, automatable, long_tests))
    return shards


def _shard(name: str, prefix, ids: list, automatable: set, long_tests: set) -> dict:
    """One shard record: full id count for reporting, automatable count for the
    decision whether the shard is worth running at all, and how many of those
    declare `timeout: long` — the two numbers `shard_timeout` budgets from."""
    auto = [test_id for test_id in ids if test_id in automatable]
    return {"name": name, "prefix": prefix, "ids": len(ids),
            "auto_ids": len(auto),
            "long_ids": sum(1 for test_id in auto if test_id in long_tests)}


def shard_report_path(out_dir: str, shard: dict) -> str:
    # A batch (`plan_units`) keeps its own report out of `out_dir`'s top level:
    # `load_results` reads every `*.json` there as a shard report, and the
    # batch's verdicts reach it through the per-member files `split_batch`
    # writes instead.
    if shard.get("report_path"):
        return shard["report_path"]
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


def shard_timeout(shard: dict, base: int, per_id, processes: int = 1) -> int:
    """Budget a shard's wall-clock from what the harness itself promises.

    WPT-RUN-4 pilot: a flat 1200s killed `encoding` (1343 ids) mid-run while
    being ten times more than `FileAPI` (125 ids) ever needed. A killed shard
    loses everything it had done — `wptreport.json` is only written at the end —
    so the budget has to scale with the shard.

    Scaling by id count alone is still a guess, and WPT-RUN-5 slice 15 measured
    why it is the wrong one: what a shard costs is set by the *declared*
    per-test timeout, not by how many tests it holds. A test that times out
    burns its whole ceiling — 10 s normally, **60 s** when the manifest marks it
    `timeout: long` — while a test that resolves costs ~0.05 s. So a long-heavy
    category where everything times out (`WebCryptoAPI`, `ai`, `bluetooth`,
    `fledge`: 96-99 % `long`) runs at 9.5-11 s/id at `--processes=6`, 13x the
    0.75 s/id of a healthy shard, and any flat per-id constant below that kills
    it — which is exactly what happened to those five shards on 2026-08-20.

    The manifest already states which tests get the long ceiling, so the budget
    is derived rather than tuned: sum the ceilings of the shard's automatable
    ids, divide by the process count (measured effective parallelism 5.8 of 6),
    add `BUDGET_SLACK` for imperfect parallelism and the fixed `base` for the
    wptserve boot (~38 s measured, the rest is headroom). The ceiling is the
    declared timeout plus wptrunner's own `extra_timeout`, which is what makes
    it match observation to two digits: 15.08 s measured for a short hung test,
    65.29 s for a long one. Nothing is paid unless the shard actually hangs:
    the budget is a kill ceiling, not a pause.

    `per_id` overrides the derivation with the old flat rate when given
    (`--shard-timeout-per-id`), kept for reproducing an earlier run's budgets.
    """
    if per_id is not None:
        return int(base + shard["ids"] * per_id)
    long_ids = shard.get("long_ids", 0)
    short_ids = max(shard.get("auto_ids", shard["ids"]) - long_ids, 0)
    declared = (long_ids * (TIMEOUT_LONG + TIMEOUT_EXTRA)
                + short_ids * (TIMEOUT_SHORT + TIMEOUT_EXTRA))
    return int(base + declared * BUDGET_SLACK / max(processes, 1))


def https_ids(manifest: dict, scope: set = None, prefixes: list = None,
              exclude_prefixes: list = None) -> list:
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
                   and (scope is None or c in scope)
                   and id_selected(i, prefixes or [], exclude_prefixes or [])})


NO_TESTS_MARKERS = ("Unable to find any tests at the path(s)", "No tests ran")
# wptrunner's own exit codes: 0 = everything as expected, 1 = unexpected
# results, 64 = something logged CRITICAL (`wptrunner.py::start`, which asserts
# nothing above 64 is ever returned). Anything else means the process was ended
# from outside rather than deciding to stop.
WPTRUNNER_RETURNCODES = (0, 1, 64)
LOG_TAIL_BYTES = 8192
EMPTY_REPORT_RETRY_PAUSE = 10


def report_is_empty(report_path: str) -> bool:
    """True when a shard's `wptreport.json` carries no verdicts.

    Deliberately cheap — missing file or zero bytes, no parse. wptrunner opens
    the report up front and writes it once at the end, so a shard that dies
    before its first test leaves exactly a zero-byte file, which is the case
    this has to catch. A report big enough to parse is taken at face value:
    checking it properly would mean reading every shard's JSON (hundreds of MB
    across a corpus run) to catch a case that does not happen.
    """
    return not os.path.isfile(report_path) or os.path.getsize(report_path) == 0


def log_says_no_tests(log_path: str) -> bool:
    """True when wptrunner itself said this shard had nothing to run.

    The two markers are the only legitimate ways to end up with no verdicts:
    the path holds no tests at all (a category in `MANIFEST.json` whose files
    are not vendored), or every test in it was filtered out (`--exclude-file`,
    a directory whose only types have no executor). Both are terminal — running
    the shard again produces the same nothing — and both must be told apart
    from a shard that *failed* before running anything, which is not terminal
    and must be retried. Only the tail is read: wptrunner logs these last.
    """
    if not os.path.isfile(log_path):
        return False
    with open(log_path, "rb") as fh:
        fh.seek(max(os.path.getsize(log_path) - LOG_TAIL_BYTES, 0))
        tail = fh.read().decode("utf-8", "replace")
    return any(marker in tail for marker in NO_TESTS_MARKERS)


#: How much of a shard log to read looking for the wptserve startup verdict.
#: The whole block is inside the first few hundred lines; the logs themselves
#: reach 3 MB.
LOG_HEAD_BYTES = 200_000

#: What wptserve says when it could not bind a port it needed. Its own words,
#: because the numbers in the message are the ports and the errno differs per
#: platform (`EADDRINUSE` is 98 on Linux, 10048 on Windows).
PORT_CONFLICT_MARKERS = ("Address already in use",
                         "is something already using that port")


def log_says_port_conflict(log_path: str) -> bool:
    """True when this shard failed to start its own servers.

    On Windows that is fatal (the shard dies with an empty report — slice 16).
    On Linux it is not, because whatever holds the port answers instead, which
    is worse in a different way: the run keeps going, served by a process it
    does not control, and nothing in the report says so. Recording the flag is
    what makes the second case visible at all.
    """
    if not os.path.isfile(log_path):
        return False
    with open(log_path, "rb") as fh:
        head = fh.read(LOG_HEAD_BYTES).decode("utf-8", "replace")
    return any(marker in head for marker in PORT_CONFLICT_MARKERS)


def shard_produced_nothing(out_dir: str, shard: dict) -> bool:
    """True when neither the report nor the raw stream of a shard holds a verdict."""
    report_path = shard_report_path(out_dir, shard)
    if not report_is_empty(report_path):
        return False
    raw_path = os.path.splitext(report_path)[0] + RAW_SUFFIX
    return not rescue_results(raw_path)


def shard_targets(shard: dict) -> list:
    """The positional test filters wptrunner gets for a shard or a batch."""
    members = shard.get("members") or [shard]
    targets = []
    for member in members:
        targets.extend(member["test_ids"] if member.get("test_ids") else [member["prefix"]])
    return targets


#: `--shared-queue`: how a shard's tests are handed to its `--processes`
#: browsers. wptrunner's default (`testloader.SingleTestSource`) deals them out
#: up front, `hash(test.id) % processes`, one fixed list per process — so a
#: process that drew three 60 s TIMEOUTs runs a minute after the other six went
#: idle, and the shard waits for it. Replaying the recorded test durations of
#: the WPT-RUN-9 control runs through a shared queue instead (longest declared
#: timeout first, which is what `TestQueueBuilder.make_queue` sorts by) cuts the
#: test phase of the same shards by 20-30 % (`docs/tasks/p2-wpt-runner-throughput.md`
#: §общая очередь). `run_smoke.py` turns the flag into wptrunner's
#: `--fully-parallel --no-restart-on-new-group` plus a directory interleave
#: (`run_smoke.SHARED_QUEUE_WPT_ARGS`, `interleave_by_directory`); named here
#: rather than imported for the same reason as `SERVER_CONFIG_ENV` below.
SHARED_QUEUE_ARGS = ("--lumen-shared-queue",)


def run_shard(shard: dict, binary: str, out_dir: str, processes: int, timeout: int,
              exclude_file: str = None, extra_env: dict = None, rss_cap=None,
              shared_queue: bool = False) -> dict:
    """Run one shard (or one batch of them, `plan_units`) as a subprocess;
    never raises on a failing shard. `extra_env` carries the parallel lane's
    server config (`run_smoke.SERVER_CONFIG_ENV`); `rss_cap` is the run's
    `browser_rss_cap.BrowserRssCap`, used only to count the browsers it killed
    under this shard. `shared_queue` — see `SHARED_QUEUE_ARGS`."""
    attempt_pids = []
    report_path = shard_report_path(out_dir, shard)
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
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
    if shared_queue:
        argv.extend(SHARED_QUEUE_ARGS)
    argv.extend(shard_targets(shard))
    env = dict(os.environ, **extra_env) if extra_env else None

    # A shard that dies before its first test is worth one immediate second
    # attempt: it costs seconds, and the failure it recovers from is transient.
    # WPT-RUN-5 slice 16 measured what skipping the retry costs — on the
    # Windows half of the 2026-08-20 corpus run 158 shards (17 683 manifest
    # ids, 26 % of the corpus) came back in ~11 s with a zero-byte report and
    # were recorded `ran`, so they scored 0 and `--resume` never looked at them
    # again. The mode matches an exception out of wptserve startup (a port from
    # the previous shard still bound), which the next attempt does not hit.
    for attempt in range(2):
        # Rotate whatever the previous attempt salvaged out of the way — see
        # RAW_PREV_SUFFIX. Only a non-empty stream is worth keeping, and only one
        # generation: the older it gets the less it can add over the newer runs.
        prev_path = os.path.splitext(report_path)[0] + RAW_PREV_SUFFIX
        if os.path.isfile(raw_path) and os.path.getsize(raw_path) > 0:
            os.replace(raw_path, prev_path)

        started = time.time()
        with open(log_path, "w", encoding="utf-8") as log:
            proc = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT, cwd=REPO_ROOT,
                                     start_new_session=(os.name != "nt"), env=env)
            attempt_pids.append(proc.pid)
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

        if outcome == "ran" and returncode not in WPTRUNNER_RETURNCODES:
            # Ended from outside — on the Windows half of the 2026-08-20 corpus
            # run, 41 shards exited 143 (SIGTERM) holding 13 136 ids, of which
            # only what the raw stream had salvaged was ever scored. Recording
            # that as `ran` is what made those ids look like engine failures
            # instead of a shard that never got to them.
            outcome = "signalled"
        if outcome == "ran" and shard_produced_nothing(out_dir, shard):
            # No verdicts at all. Either wptrunner said there was nothing to run
            # — terminal, and not a loss — or the shard failed before its first
            # test, which is a loss and is why `ran` is not the answer here: an
            # outcome of `ran` is what makes `--resume` treat a shard as done.
            outcome = "no-tests" if log_says_no_tests(log_path) else "empty-report"
        if outcome != "empty-report" or attempt:
            break
        print(f"  shard {shard['name']}: no verdicts in {elapsed:.0f}s and no "
              f"'nothing to run' from wptrunner — retrying once", flush=True)
        time.sleep(EMPTY_REPORT_RETRY_PAUSE)

    state = {"name": shard["name"], "prefix": shard["prefix"], "ids": shard["ids"],
             "auto_ids": shard.get("auto_ids"),
             "outcome": outcome, "returncode": returncode, "seconds": round(elapsed, 1),
             "report": os.path.relpath(report_path, REPO_ROOT) if os.path.isfile(report_path) else None}
    if log_says_port_conflict(log_path):
        state["port_conflict"] = True
    if rss_cap is not None:
        killed = sum(len(rss_cap.kills_under(pid)) for pid in attempt_pids)
        if killed:
            # The test that browser was running is reported CRASH by wptrunner;
            # this is what tells such a CRASH apart from an engine crash.
            state["rss_cap_kills"] = killed
    return state


# --- WPT-RUN-9: batching small shards and running lanes in parallel ----------
#
# Measured on the 2026-08-20 corpus runs (`docs/wpt/runs/*.json`): 367 of 479
# shards finished in under 120 s, 186 held at most 20 ids, and each of them
# paid a full wptserve boot (~38 s, WPT-RUN-5 slice 15) for a median of 22-31 s
# of work. wptrunner also caps its process count at the number of directories
# a shard holds, so a small shard leaves most of `--processes` idle. And the
# shards ran one after another on a machine whose CPU sat at ~1.2 busy threads
# of 16 (PERF-10 slice 2): the wall clock is spent waiting on test timeouts.
# Two independent remedies follow, both off by default:
#
# * `--batch-small N` runs consecutive small shards as one wptrunner process
#   and splits the report back per shard afterwards (`split_batch`), so
#   `state.json`, `--resume`, `score_audit.py` and every other reader still see
#   exactly the shards `plan_shards` produced;
# * `--parallel-shards K` runs K units at once, each lane on its own copy of the
#   server ports (`lane_server_config`), which is what makes two wptserves on
#   one machine possible at all.

#: Shards at most this many automatable ids are candidates for a batch.
BATCH_SMALL_DEFAULT = 150
#: A `(bare)` shard is addressed by an explicit id list; past this many ids it
#: is not batched, to keep the shard's command line bounded (Windows: 32 767
#: characters for the whole command).
BATCH_MAX_EXPLICIT_IDS = 50
#: Port offset between lanes. Lane 0 keeps `config.json` as is; lane k adds
#: k * LANE_PORT_STEP to every port. 18300…19000 + 1000·k stays clear of the
#: Hyper-V excluded ranges seen on the dev machine (1077-1703, 11688-12782,
#: 50000-50059) and of every port `lumen-network`'s bad-port list blocks.
LANE_PORT_STEP = 1000
#: Batches live here, inside `out_dir` but out of `load_results`' reach — it
#: reads `out_dir/*.json` only, and a batch report there would be scored twice.
BATCH_SUBDIR = "batches"
#: Environment variable `run_smoke.py` reads a lane's server config from — the
#: same name as `run_smoke.SERVER_CONFIG_ENV`, repeated rather than imported
#: because importing `run_smoke` pulls in the whole of `wptrunner`.
SERVER_CONFIG_ENV = "LUMEN_WPT_SERVER_CONFIG"


def plan_units(shards: list, small: int, max_ids: int, out_dir: str) -> list:
    """Group shards into run units: big shards alone, small ones in batches.

    A unit is a shard dict (run as before) or a batch: a shard-shaped dict with
    `members` (the original shards, untouched), summed `ids`/`auto_ids`/
    `long_ids` for the budget and a `report_path` under `BATCH_SUBDIR`. Small
    shards are taken in plan order — neighbours are usually the same category —
    and a batch closes once adding the next one would pass `max_ids`. Shards
    that are not small, and a batch of one, stay as they were — with
    `small == 0` the result is `shards` itself. Ordering is the caller's.
    """
    units, batch = [], []

    def close():
        if not batch:
            return
        if len(batch) == 1:
            units.append(batch[0])
        else:
            first = batch[0]["name"].replace("/", "__").replace(" ", "_")
            name = f"batch {first} +{len(batch) - 1}"
            units.append({
                "name": name, "prefix": None, "members": list(batch),
                "ids": sum(s["ids"] for s in batch),
                "auto_ids": sum(s.get("auto_ids", s["ids"]) for s in batch),
                "long_ids": sum(s.get("long_ids", 0) for s in batch),
                "report_path": os.path.join(out_dir, BATCH_SUBDIR,
                                            f"{first}+{len(batch) - 1}.json"),
            })
        batch.clear()

    for shard in shards:
        batchable = (small > 0 and shard.get("auto_ids", shard["ids"]) <= small
                     and len(shard.get("test_ids") or ()) <= BATCH_MAX_EXPLICIT_IDS)
        if not batchable:
            close()
            units.append(shard)
            continue
        if batch and sum(s.get("auto_ids", s["ids"]) for s in batch) \
                + shard.get("auto_ids", shard["ids"]) > max_ids:
            close()
        batch.append(shard)
    close()
    return units


def member_owner(members: list):
    """`test_id -> member shard name` for a batch, longest prefix first — the
    same attribution rule `score_audit.shard_index` uses."""
    explicit = {i: m["name"] for m in members for i in (m.get("test_ids") or ())}
    prefixes = sorted(((m["prefix"], m["name"]) for m in members if m.get("prefix")),
                      key=lambda pair: -len(pair[0]))

    def owner(test_id: str):
        if test_id in explicit:
            return explicit[test_id]
        for prefix, name in prefixes:
            if test_id.startswith(prefix):
                return name
        return None
    return owner


def _repo_relative(path: str) -> str:
    """`path` relative to the checkout, or absolute when it lies on another
    drive (a scratch out-dir on Windows, where `relpath` raises)."""
    try:
        return os.path.relpath(path, REPO_ROOT)
    except ValueError:
        return os.path.abspath(path)


def split_batch(batch: dict, state: dict, out_dir: str) -> list:
    """Turn one finished batch back into per-shard states and reports.

    Each member gets `out_dir/<name>.json` holding exactly its own results —
    the file a solo run of that shard would have produced — so nothing
    downstream can tell a batched shard from a solo one. Results come from the
    batch's `wptreport.json`, or from its raw stream when the batch was killed
    (`rescue_results`), in which case the members say `salvaged`.

    A member's outcome is the batch's, with one refinement: a member with no
    verdict at all in a batch that otherwise ran is `no-tests` and gets no
    report file — writing an empty one would read as a hollow shard
    (`score_audit`'s `shard-empty`), and `ran` would make `--resume` trust it.
    Its executable ids, if it has any, still show up as lost in the accounting.
    Seconds are the batch's wall clock shared out by result count, so the sum
    over members is the batch's real cost.
    """
    report_path = shard_report_path(out_dir, batch)
    results, salvaged = [], False
    if not report_is_empty(report_path):
        try:
            with open(report_path, encoding="utf-8") as fh:
                results = json.load(fh).get("results", [])
        except (json.JSONDecodeError, OSError):
            results = []
    if not results:
        raw_path = os.path.splitext(report_path)[0] + RAW_SUFFIX
        results = list(rescue_results(raw_path).values())
        salvaged = bool(results)

    owner = member_owner(batch["members"])
    by_member = {m["name"]: [] for m in batch["members"]}
    for result in results:
        name = owner(result["test"])
        if name in by_member:
            by_member[name].append(result)

    total = max(len(results), 1)
    states = []
    for member in batch["members"]:
        mine = by_member[member["name"]]
        member_path = os.path.join(out_dir, member["name"].replace("/", "__") + ".json")
        outcome = state["outcome"]
        if mine:
            with open(member_path, "w", encoding="utf-8") as fh:
                json.dump({"results": mine}, fh)
        elif outcome == "ran":
            outcome = "no-tests"
        member_state = {
            "name": member["name"], "prefix": member["prefix"], "ids": member["ids"],
            "auto_ids": member.get("auto_ids"), "outcome": outcome,
            "returncode": state["returncode"],
            "seconds": round(state["seconds"] * len(mine) / total, 1),
            "report": _repo_relative(member_path) if mine else None,
            "batch": batch["name"],
        }
        if salvaged and mine:
            member_state["salvaged"] = True
        if state.get("port_conflict"):
            member_state["port_conflict"] = True
        if state.get("rss_cap_kills"):
            # Counted per wptrunner process, so per batch: which member's test
            # the killed browser was running is in `rss-cap-kills.jsonl` time
            # order, not here.
            member_state["batch_rss_cap_kills"] = state["rss_cap_kills"]
        states.append(member_state)
    return states


def lane_server_config(lane: int, out_dir: str, base_path: str = None) -> str:
    """Write lane `lane`'s server config and return its path (lane 0: None).

    Only `ports` is written: `run_smoke` merges this file over the normal
    `tests/wpt/config.json`, so everything else stays exactly as a solo run
    has it. Every port of the base config moves by `lane * LANE_PORT_STEP`.
    """
    if lane == 0:
        return None
    with open(base_path or port_guard.CONFIG_PATH, encoding="utf-8") as fh:
        base = json.load(fh)
    ports = {}
    for kind, values in (base.get("ports") or {}).items():
        ports[kind] = [v + lane * LANE_PORT_STEP if isinstance(v, int) else v
                       for v in values]
    path = os.path.join(out_dir, BATCH_SUBDIR, f"lane{lane}-config.json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump({"ports": ports}, fh, indent=2)
    return path


def lane_ports(config_path) -> list:
    """The TCP ports a lane's wptserve binds — what its port guard checks."""
    return port_guard.configured_ports(config_path or port_guard.CONFIG_PATH)


def available_memory_gb():
    """Free physical memory in GB, or None when psutil is not installed."""
    try:
        import psutil  # noqa: PLC0415 — optional, present in tests/wpt/.venv
    except ImportError:
        return None
    return psutil.virtual_memory().available / 2**30


class SharedHeavyLock:
    """`heavy_lock` held while *any* lane runs, released when all are idle.

    Taking it per unit, as the sequential loop does, cannot work with lanes:
    on Windows `msvcrt.locking` refuses a second handle of the same process,
    so lane 2 would sit out the 300 s courtesy timeout and then run unlocked
    with a warning. Counting holders keeps the BUG-1029 courtesy — a build
    waiting on `scripts/cargo-heavy.sh` still gets in whenever the run drains.
    """

    def __init__(self, owner: str):
        self._owner = owner
        self._count = 0
        self._mutex = threading.Lock()
        self._stack = None

    def acquire(self):
        with self._mutex:
            if self._count == 0:
                self._stack = contextlib.ExitStack()
                self._stack.enter_context(heavy_lock.heavy_lock(self._owner))
            self._count += 1

    def release(self):
        with self._mutex:
            self._count -= 1
            if self._count == 0 and self._stack is not None:
                self._stack.close()
                self._stack = None


#: Poll interval of the memory gate, seconds.
MEMORY_GATE_POLL = 10.0


def run_units_parallel(units: list, binary: str, args, exclude_file, finish,
                       rss_cap=None) -> int:
    """Run `units` on `args.parallel_shards` lanes pulling from one queue.

    Each lane is a thread driving one `run_shard` subprocess at a time on its
    own server ports (`lane_server_config`); verdicts cannot depend on the lane
    because nothing but the port numbers differs (`--self-test` and the A/B in
    `docs/tasks/p2-wpt-runner-throughput.md` WPT-RUN-9 check that). The queue is
    ordered longest budget first, which bounds the makespan by the longest
    shard instead of leaving it to whichever lane drew it last.

    Two guards the sequential loop does not need:

    * orphaned `lumen` processes are reaped once, before any lane starts —
      between units it would be wrong, since the reaper counts every browser
      under this process as `stale`, the running lanes' included;
    * a memory gate: a lane does not start a unit while less than
      `--min-free-gb` of physical memory is available and another lane is
      running. Paging stretches exactly the wall-clock timeouts the verdicts
      depend on (PERF-10 slice 2 measured 18-21 GB peak for one lane at
      `--processes 7`, and hit WinError 1455 on a 2 GB page file).

    Returns 0, or 1 when a lane had to stop because its ports were taken.
    """
    lanes = args.parallel_shards
    if not args.no_port_guard:
        port_guard.reap_lumen_orphans(own_pid=os.getpid())
    configs = [lane_server_config(lane, args.out_dir) for lane in range(lanes)]

    def budget_of(unit):
        return shard_timeout(unit, args.shard_timeout_base, args.shard_timeout_per_id,
                             args.processes)

    queue = sorted(units, key=budget_of, reverse=True)
    total = len(queue)
    mutex = threading.Lock()
    running = [0]
    started = [0]
    failures = []
    shared_lock = SharedHeavyLock(f"run_corpus.py pid={os.getpid()} --parallel-shards {lanes}")

    def take():
        with mutex:
            if failures or not queue:
                return None, 0
            started[0] += 1
            return queue.pop(0), started[0]

    def wait_for_memory(lane):
        warned = False
        while args.min_free_gb > 0:
            free = available_memory_gb()
            with mutex:
                others = running[0]
            if free is None or free >= args.min_free_gb or others == 0:
                return
            if not warned:
                print(f"  lane {lane}: {free:.1f} GB free < --min-free-gb "
                      f"{args.min_free_gb} with {others} lane(s) busy — waiting", flush=True)
                warned = True
            time.sleep(MEMORY_GATE_POLL)

    def lane_main(lane):
        config = configs[lane]
        extra_env = {SERVER_CONFIG_ENV: config} if config else None
        ports = lane_ports(config)
        while True:
            wait_for_memory(lane)
            unit, index = take()
            if unit is None:
                return
            if not args.no_port_guard:
                try:
                    port_guard.ensure_free(ports=ports, own_pid=os.getpid())
                except port_guard.PortsBusy as exc:
                    with mutex:
                        failures.append(f"lane {lane}: {exc}")
                        queue.insert(0, unit)
                    return
            budget = budget_of(unit)
            print(f"[{index}/{total}] lane {lane}: {unit['name']}: {unit['ids']} ids "
                  f"(budget {budget}s) started", flush=True)
            with mutex:
                running[0] += 1
            shared_lock.acquire()
            try:
                state = run_shard(unit, binary, args.out_dir, args.processes, budget,
                                  exclude_file, extra_env, rss_cap, args.shared_queue)
            finally:
                shared_lock.release()
                with mutex:
                    running[0] -= 1
            with mutex:
                finish(unit, state)
            print(f"[{index}/{total}] lane {lane}: {unit['name']}: {state['outcome']} "
                  f"in {state['seconds']}s", flush=True)

    threads = [threading.Thread(target=lane_main, args=(lane,), daemon=True,
                                name=f"lane{lane}") for lane in range(lanes)]
    for thread in threads:
        thread.start()
        # Staggered start: wptserve boots and the first browser launches are
        # the burst of the whole unit; overlapping K of them buys nothing.
        time.sleep(2)
    for thread in threads:
        while thread.is_alive():
            thread.join(timeout=1.0)
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        print(f"stopped with {len(queue)} unit(s) not run — rerun the same command with "
              f"--resume", file=sys.stderr, flush=True)
        return 1
    return 0


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


def resumable_states(previous: list, out_dir: str, retry_timeouts: bool,
                     budgets: dict = None) -> list:
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
    * `--retry-timeouts` retries them all, which is what a resume with a wider
      budget wants (`WPT-RUN-9` — since WPT-RUN-5 slice 15 that means the
      derived budget rather than a raised `--shard-timeout-per-id`, which now
      only exists to reproduce an older run). That path is safe because
      `run_shard` rotates the raw stream first, so a shorter second attempt
      cannot destroy the first one's results.

    Any other outcome (`no-report`/`empty-report` — wptrunner died at startup)
    is always retried: it failed fast, so retrying is cheap, and it produced
    nothing.

    `ran` is also *verified* rather than believed, because the outcome recorded
    by an older run cannot be trusted to mean what it says: before slice 16 a
    shard that failed before its first test was recorded `ran` with a zero-byte
    report, and the run's own summary counted it as complete. Re-checking the
    disk is what lets such a state file heal on the next `--resume` instead of
    keeping its hole forever — the Windows half of the 2026-08-20 run has 158
    of them, 17 683 manifest ids. The check is one `stat` per shard unless the
    report really is empty, so it costs nothing on a healthy run.

    `budgets` (`{shard name: seconds}`, what *this* code would give each shard)
    is what turns "pass --retry-timeouts" from advice into a price. An out-dir
    outlives the code that filled it: the 2026-08-20 Linux half was launched
    before slice 15 derived the budget from the manifest's declared ceilings,
    so its killed shards were cut off at the old flat rule's wall and this code
    would now let them run 1.6-3.9x longer — there a retry is a real second
    chance, not a replay of the same death. Without the comparison the operator
    cannot tell that case from the one described above (same budget, same
    machine, same wall), and the two want opposite decisions.
    """
    states, kept, retried, hollow = [], [], [], []
    for state in previous:
        if state["outcome"] == "ran":
            if shard_produced_nothing(out_dir, state) and not log_says_no_tests(
                    os.path.splitext(shard_report_path(out_dir, state))[0] + ".log"):
                hollow.append(state["name"])
                retried.append(state["name"])
                continue
            if state.get("returncode") not in WPTRUNNER_RETURNCODES:
                # An older state file recorded `ran` for a shard the OS ended
                # (see WPTRUNNER_RETURNCODES); re-read it as what it was, so
                # that a partial shard is at least visible and `--retry-timeouts`
                # can reach it.
                state = dict(state, outcome="signalled")
            else:
                states.append(state)
                continue
        if state["outcome"] == "no-tests":
            states.append(state)
            continue
        if state["outcome"] in ("timeout", "signalled") and not retry_timeouts:
            report_path = shard_report_path(out_dir, state)
            raw_path = os.path.splitext(report_path)[0] + RAW_SUFFIX
            # A report that survived is protected the same way a salvaged raw
            # stream is: wptrunner truncates `--log-wptreport` when it opens it,
            # so replaying a shard destroys what it already had before it can
            # add anything. `--retry-timeouts` is the way to say that trade is
            # worth making.
            if rescue_results(raw_path) or not report_is_empty(report_path):
                states.append(state)
                kept.append((state["name"], state.get("seconds") or 0.0))
                continue
        retried.append(state["name"])
    if kept:
        print(f"--resume: {len(kept)} killed shard(s) kept partial rather than replayed "
              f"({', '.join(name for name, _ in kept)}) — pass --retry-timeouts to run them "
              f"again", flush=True)
        widened = [(name, wall, budgets[name]) for name, wall in kept
                   if budgets and budgets.get(name)
                   and budgets[name] > wall * BUDGET_WIDENED]
        if widened:
            detail = ", ".join(f"{name} {wall:.0f}s -> {budget}s"
                               for name, wall, budget in widened[:6])
            more = f", and {len(widened) - 6} more" if len(widened) > 6 else ""
            hours = sum(budget for _n, _w, budget in widened) / 3600
            print(f"--resume: {len(widened)} of them were killed under a budget narrower than "
                  f"this code's ({detail}{more}) — a retry re-runs each shard whole, so it "
                  f"costs up to {hours:.1f} h of wall clock and can only add ids that have no "
                  f"verdict at all; price those ids first with "
                  f"`score_audit.py --out-dir {out_dir}` (its kill-cost section says what the "
                  f"salvaged part of the same shards was worth)", flush=True)
    if hollow:
        print(f"--resume: {len(hollow)} shard(s) recorded as complete produced no verdicts at all "
              f"and will run again ({', '.join(hollow[:8])})", flush=True)
    if retried:
        print(f"--resume: {len(retried)} shard(s) will run again ({', '.join(retried[:8])})", flush=True)
    return states


def coverage_breakdown(manifest: dict, results: dict, empty_shards: list,
                       shard_states: list) -> dict:
    """Why the ids with no verdict have none, as part of the run's own summary.

    `never ran: N` is three unrelated things added together: types no executor
    is registered for (a runner gap, `WPT-RUN-8`, known in advance), ids inside
    a shard that visibly failed (recoverable — the shard is named in
    `state.json`), and ids inside a shard that reported **success** (a silent
    hole). The third is the Windows failure mode of slice 16: 479 of 479 shards
    recorded `ran` while 36 % of the corpus had no verdict, and the summary said
    only "never ran: 27117 — no executor, skipped, or lost shard", which is true
    of all three.

    The classification itself is `score_audit.accounting`, not a second copy of
    it: that module is the one that knows the causes apart (including the
    `shard-empty` case, which a naive "shard says ran, id has no verdict" rule
    misreads as a leak). Calling it here only moves the verdict from an audit
    someone has to remember to run into the line every run prints anyway. The
    import is local because `score_audit` imports this module at top level.
    """
    import score_audit  # noqa: PLC0415 — circular at module level, by design

    acc = score_audit.accounting(manifest, {"shards": shard_states}, results, empty_shards)
    cause = acc["cause"]
    lost = acc["lost_by_shard"]
    return {
        "by_type": acc["not_run_by_type"],
        "no_executor": sum(n for name, n in cause.items() if name.startswith("no-executor:")),
        "no_executor_by_type": {name.split(":", 1)[1]: n for name, n in cause.items()
                                if name.startswith("no-executor:")},
        "lost": sum(e["ids"] for e in lost),
        "lost_by_shard": lost,
        "lost_in_ran_shards": cause.get("lost-in-ran-shard", 0),
        "shard_empty": cause.get("shard-empty", 0),
        "leaked_examples": acc["leaked_examples"][:10],
    }


def score_reports(manifest: dict, out_dir: str, scope: set = None,
                  shard_states: list = None, prefixes: list = None,
                  exclude_prefixes: list = None) -> dict:
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
        if not id_selected(test_id, prefixes or [], exclude_prefixes or []):
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
            "empty_shards": empty_shards,
            "coverage": coverage_breakdown(manifest, results, empty_shards,
                                           shard_states or [])}


def _print_coverage(coverage: dict, not_run: int) -> None:
    """Print why the ids with no verdict have none — see `coverage_breakdown`."""
    if not coverage:
        return
    gaps = coverage.get("no_executor_by_type") or {}
    if gaps:
        detail = ", ".join(f"{t}={n}" for t, n in sorted(gaps.items(), key=lambda kv: -kv[1]))
        print(f"    no executor:  {coverage['no_executor']}  ({detail}) — a runner gap "
              f"(WPT-RUN-8), not an engine result")
    lost = coverage.get("lost_by_shard") or []
    if lost:
        head = ", ".join(f"{e['shard']}({e['cause']})={e['ids']}" for e in lost[:6])
        more = f", and {len(lost) - 6} more" if len(lost) > 6 else ""
        print(f"    lost:         {coverage['lost']}  in {len(lost)} shard(s): {head}{more}")
    hollow = coverage.get("shard_empty") or 0
    if hollow:
        named = ", ".join(f"{e['shard']}={e['ids']}" for e in lost
                          if e["cause"] == "shard-empty")
        print(f"    EMPTY REPORT: {hollow} of those are in shard(s) recorded as `ran` that "
              f"wrote no report at all: {named} — a checkpoint from before slice 16, where "
              f"that outcome is `empty-report` and gets retried")
    silent = coverage.get("lost_in_ran_shards") or 0
    if silent:
        named = ", ".join(f"{e['shard']}={e['ids']}" for e in lost
                          if e["cause"] == "lost-in-ran-shard")
        print(f"    SILENT HOLE:  {silent} ids scored 0 inside shard(s) that reported "
              f"success: {named}")
        print(f"                  the run says it covered them and no verdict exists — "
              f"the number is not publishable until this is explained "
              f"(tests/wpt/score_audit.py names the ids)")
    # Ids the classifier never saw: their shard is not in `state.json` at all,
    # so nothing ran them and nothing failed either. On a finished run this is
    # zero; on a live or resumed one it is the tail still to come, and saying
    # so is what keeps the three explained buckets from looking like the whole
    # of `never ran`.
    unseen = not_run - coverage.get("no_executor", 0) - coverage.get("lost", 0)
    if unseen > 0:
        print(f"    not attempted: {unseen}  no shard of theirs has run yet "
              f"(in-flight or partial run)")


def print_summary(scored: dict, shard_states: list) -> None:
    totals = scored["totals"]
    print()
    print("=" * 72)
    print(f"pass-rate: {totals['pass_rate'] * 100:.2f}%  "
          f"({totals['score']:.0f} of {totals['ids']} automatable manifest ids)")
    print(f"  ran:              {totals['ran']}")
    print(f"  never ran:        {totals['not_run']}  (scored 0)")
    _print_coverage(scored.get("coverage") or {}, totals["not_run"])
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
    nothing = [s for s in shard_states if s["outcome"] == "no-tests"]
    if nothing:
        print(f"  ran nothing:      {len(nothing)} shard(s) wptrunner had nothing to run in "
              f"(not a loss — no vendored files, or every test filtered out)")
    bad = [s for s in shard_states if s["outcome"] not in ("ran", "no-tests")]
    if bad:
        print(f"  PROBLEM SHARDS:   {len(bad)} — " + ", ".join(f"{s['name']}({s['outcome']})" for s in bad[:8]))
        if len(bad) > 8:
            print(f"                    ... and {len(bad) - 8} more, see state.json")
    print("=" * 72)


def _selftest() -> int:
    """Prove the coverage breakdown on a hand-made run, without running anything.

    The three buckets it separates only ever occur together on an 11-hour corpus
    run, and the one that matters most — ids lost inside a shard that reported
    success — is by construction the one a healthy run never produces. So it is
    checked here instead: a four-category manifest, four shards with hand-picked
    outcomes, and one verdict.
    """
    def leaf():
        return ["hash", [None, {}]]

    manifest = {"items": {
        "testharness": {
            "dom": {"ok.html": leaf(), "lost.html": leaf()},
            "css": {"a.html": leaf()},
            "encoding": {"e.html": leaf()},
        },
        "aamtest": {"svg": {"c.html": leaf()}},
    }}
    shard_states = [
        {"name": "dom", "outcome": "ran"},
        {"name": "css", "outcome": "timeout"},
        {"name": "encoding", "outcome": "ran"},
        {"name": "svg", "outcome": "ran"},
    ]
    results = {"/dom/ok.html": {"status": "OK", "subtests": []}}
    # `load_results` reports empty shards by on-disk report name, "/" → "__".
    empty_shards = ["encoding"]

    got = coverage_breakdown(manifest, results, empty_shards, shard_states)
    by_shard = {e["shard"]: (e["cause"], e["ids"]) for e in got["lost_by_shard"]}
    checks = [
        ("no-executor type counted apart", got["no_executor"] == 1
         and got["no_executor_by_type"] == {"aamtest": 1}),
        ("no-executor id not blamed on its shard", "svg" not in by_shard),
        ("killed shard named with its cause", by_shard.get("css") == ("shard-killed", 1)),
        ("silent hole counted", got["lost_in_ran_shards"] == 1),
        ("silent hole named", by_shard.get("dom") == ("lost-in-ran-shard", 1)),
        ("empty shard is not a silent hole", got["shard_empty"] == 1
         and by_shard.get("encoding") == ("shard-empty", 1)),
        ("lost excludes the no-executor id", got["lost"] == 3),
        ("id with a verdict is not counted", got["by_type"].get("testharness") == 3),
    ]
    checks.extend(_selftest_resume())
    checks.extend(_selftest_prefixes())
    checks.extend(_selftest_batches())
    checks.extend(_selftest_shared_queue())
    with contextlib.redirect_stdout(io.StringIO()):
        cap_status = browser_rss_cap._selftest()  # noqa: SLF001 — its own selftest
    checks.append(("browser rss cap kills only an oversized lumen of this run",
                   cap_status == 0))
    for label, ok in checks:
        print(f"  {'PASS' if ok else 'FAIL'}  {label}")
    failed = [label for label, ok in checks if not ok]
    print(f"selftest: {'PASS' if not failed else 'FAIL (' + ', '.join(failed) + ')'}")
    return 1 if failed else 0


def _selftest_prefixes() -> list:
    """Prove `--prefixes` / `--exclude-prefixes` on a hand-made manifest (WPT-RUN-14).

    The property that matters is an exact cover: the planned shards must run
    every selected id exactly once and nothing else. Both halves have a failure
    that looks like success — a shard that is too wide silently runs (and scores
    against) the rest of the category, and one that is too narrow loses ids that
    then score 0 as if the engine failed them — so the plan is expanded back to
    ids and compared as sets.
    """
    def leaf():
        return ["hash", [None, {}]]

    files = ["a/x/1.html", "a/x/2.html", "a/y/3.html", "a/y/z/4.html", "a/y/z/5.html",
             "a/6.html", "a/xx/7.html", "b/8.html"]
    tree = {}
    for f in files:
        node = tree
        parts = f.split("/")
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        node[parts[-1]] = leaf()
    manifest = {"items": {"testharness": tree}}

    def covered(shards):
        ids = []
        for s in shards:
            if s.get("test_ids"):
                ids.extend(s["test_ids"])
            else:
                ids.extend(f"/{f}" for f in files if f"/{f}".startswith(s["prefix"]))
        return sorted(ids)

    def plan(cats, inc, exc):
        global SHARD_THRESHOLD
        saved, SHARD_THRESHOLD = SHARD_THRESHOLD, 2   # force the category to split
        try:
            with contextlib.redirect_stderr(io.StringIO()):
                return plan_shards(manifest, cats, parse_prefixes(inc), parse_prefixes(exc))
        finally:
            SHARD_THRESHOLD = saved

    def ids_of(*names):
        return sorted(f"/{f}" for f in files if f in names)

    whole = plan(["a"], None, None)
    one = plan(["a"], "a/y", None)
    mixed = plan(["a"], "a/y/z", "a/y/z/5.html")
    out = plan(["a"], "a", "a/y,a/6.html")
    return [
        ("prefixes: no filter plans what it always did",
         covered(whole) == ids_of(*[f for f in files if f.startswith("a/")])),
        ("prefixes: a directory is covered exactly, `a/x` does not leak into `a/xx`",
         covered(plan(["a"], "a/x", None)) == ids_of("a/x/1.html", "a/x/2.html")),
        ("prefixes: a directory with a subdirectory is covered exactly",
         covered(one) == ids_of("a/y/3.html", "a/y/z/4.html", "a/y/z/5.html")),
        ("prefixes: an exclusion inside a selection carves one id out",
         covered(mixed) == ids_of("a/y/z/4.html")),
        ("prefixes: exclusions win over the whole category",
         covered(out) == ids_of("a/x/1.html", "a/x/2.html", "a/xx/7.html")),
        ("prefixes: the filter never selects another category",
         covered(plan(["a"], "b", None)) == []),
        ("prefixes: a single file is addressable",
         covered(plan(["a"], "a/y/3.html", None)) == ids_of("a/y/3.html")),
        ("prefixes: a variant id belongs to its file",
         id_selected("/a/y/3.html?x=1", ["a/y/3.html"], [])
         and not id_selected("/a/y/3.htmlx", ["a/y/3.html"], [])),
    ]


def _selftest_shared_queue() -> list:
    """`--shared-queue`: the flag this script passes is the one `run_smoke.py`
    understands, and its directory interleave spreads neighbours apart while
    keeping each directory's own order and every item exactly once."""
    import run_smoke  # noqa: PLC0415 — pulls in wptrunner; only the selftest needs it

    paths = ["/a/1.html", "/a/2.html", "/a/3.html", "/b/1.html", "/c/1.html", "/c/2.html"]
    got = run_smoke.interleave_by_directory(paths, lambda p: p)
    return [
        ("shared queue: run_corpus passes the flag run_smoke parses",
         SHARED_QUEUE_ARGS == (run_smoke.SHARED_QUEUE_FLAG,)),
        ("shared queue: interleave round-robins directories",
         got == ["/a/1.html", "/b/1.html", "/c/1.html", "/a/2.html", "/c/2.html", "/a/3.html"]),
        ("shared queue: interleave keeps every item once", sorted(got) == sorted(paths)),
    ]


def _selftest_batches() -> list:
    """Prove `--batch-small` and the lane configs of `--parallel-shards` (WPT-RUN-9).

    The property that matters is that batching is invisible downstream: every
    member ends up with the report and the state a solo run would have given
    it, nothing is attributed to the wrong member (`/a/x/` must not take
    `/a/xx/`), and a member with no verdict in a batch that ran is not passed
    off as `ran`. Lanes must differ in ports and only in ports.
    """
    out_dir = tempfile.mkdtemp(prefix="lumen-batch-selftest-")
    try:
        shards = [
            {"name": "big", "prefix": "/big/", "ids": 900, "auto_ids": 900, "long_ids": 0},
            {"name": "a/x", "prefix": "/a/x/", "ids": 3, "auto_ids": 3, "long_ids": 0},
            {"name": "a/xx", "prefix": "/a/xx/", "ids": 2, "auto_ids": 2, "long_ids": 1},
            {"name": "a (bare)", "prefix": None, "ids": 1, "auto_ids": 1, "long_ids": 0,
             "test_ids": ["/a/top.html"]},
            {"name": "empty", "prefix": "/empty/", "ids": 4, "auto_ids": 4, "long_ids": 0},
            {"name": "c", "prefix": "/c/", "ids": 5, "auto_ids": 5, "long_ids": 0},
        ]
        units = plan_units(shards, small=10, max_ids=10, out_dir=out_dir)
        off = plan_units(shards, small=0, max_ids=10, out_dir=out_dir)
        batch = units[1] if len(units) > 1 else {}
        members = [m["name"] for m in batch.get("members", [])]

        os.makedirs(os.path.dirname(shard_report_path(out_dir, batch)), exist_ok=True)
        results = [{"test": t, "status": "OK", "subtests": []}
                   for t in ("/a/x/1.html", "/a/xx/2.html", "/a/top.html")]
        with open(shard_report_path(out_dir, batch), "w", encoding="utf-8") as fh:
            json.dump({"results": results}, fh)
        states = {s["name"]: s for s in split_batch(
            batch, {"outcome": "ran", "returncode": 1, "seconds": 30.0}, out_dir)}
        loaded, _recovered, _empty = load_results(out_dir)

        def report_of(name):
            path = shard_report_path(out_dir, {"name": name})
            if not os.path.isfile(path):
                return None
            with open(path, encoding="utf-8") as fh:
                return sorted(r["test"] for r in json.load(fh)["results"])

        base = os.path.join(out_dir, "base-config.json")
        with open(base, "w", encoding="utf-8") as fh:
            json.dump({"ports": {"http": [18300, 18301], "dns": [None]}}, fh)
        lane1 = lane_server_config(1, out_dir, base)
        with open(lane1, encoding="utf-8") as fh:
            lane1_cfg = json.load(fh)

        return [
            ("batch: big shard stays alone, small ones are grouped up to the cap",
             [u["name"] for u in units][0] == "big"
             and members == ["a/x", "a/xx", "a (bare)", "empty"]
             and units[2]["name"] == "c"),
            ("batch: off means the plan is untouched", off == shards),
            ("batch: budget sums the members", batch.get("auto_ids") == 10
             and batch.get("long_ids") == 1),
            ("batch: wptrunner gets every member's filter",
             shard_targets(batch) == ["/a/x/", "/a/xx/", "/a/top.html", "/empty/"]),
            ("batch: its own report is out of load_results' reach",
             os.path.dirname(shard_report_path(out_dir, batch)) != out_dir
             and len(loaded) == 3),
            ("batch: `/a/x/` does not take `/a/xx/`",
             report_of("a/x") == ["/a/x/1.html"] and report_of("a/xx") == ["/a/xx/2.html"]),
            ("batch: an explicit-id member gets its id",
             report_of("a (bare)") == ["/a/top.html"]),
            ("batch: a member with no verdict is no-tests and has no report",
             states["empty"]["outcome"] == "no-tests" and report_of("empty") is None),
            ("batch: members keep their own names and carry the batch",
             set(states) == set(members)
             and all(s["batch"] == batch["name"] for s in states.values())),
            ("batch: wall clock is shared out, not multiplied",
             abs(sum(s["seconds"] for s in states.values()) - 30.0) < 0.2),
            ("lanes: lane 0 keeps config.json", lane_server_config(0, out_dir, base) is None),
            ("lanes: lane 1 shifts every port and writes nothing else",
             lane1_cfg == {"ports": {"http": [19300, 19301], "dns": [None]}}),
        ]
    finally:
        shutil.rmtree(out_dir, ignore_errors=True)


def _selftest_resume() -> list:
    """Prove that `--resume` prices a retry instead of merely offering one.

    The case only occurs on a resumed out-dir older than the code resuming it,
    which is exactly the case nobody can reproduce on demand — so it is built
    here: two killed shards with salvaged results, one of which this code would
    now budget wider than the wall it died at (WPT-RUN-5 slice 28).
    """
    out_dir = tempfile.mkdtemp(prefix="lumen-resume-selftest-")
    try:
        previous = [
            {"name": "widened", "prefix": "/widened/", "ids": 300, "outcome": "timeout",
             "seconds": 1000.0, "report": os.path.join(out_dir, "widened.json")},
            {"name": "same", "prefix": "/same/", "ids": 300, "outcome": "timeout",
             "seconds": 1000.0, "report": os.path.join(out_dir, "same.json")},
        ]
        for state in previous:
            # Non-empty report: what makes a killed shard worth protecting.
            with open(shard_report_path(out_dir, state), "w", encoding="utf-8") as fh:
                json.dump({"results": []}, fh)
        budgets = {"widened": 3000, "same": 1010}

        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            kept = resumable_states(previous, out_dir, retry_timeouts=False, budgets=budgets)
        said = buf.getvalue()

        buf_retry = io.StringIO()
        with contextlib.redirect_stdout(buf_retry):
            replayed = resumable_states(previous, out_dir, retry_timeouts=True, budgets=budgets)

        buf_blind = io.StringIO()
        with contextlib.redirect_stdout(buf_blind):
            resumable_states(previous, out_dir, retry_timeouts=False, budgets=None)

        return [
            ("killed shard with results is kept", len(kept) == 2),
            ("widened budget named with both walls", "widened 1000s -> 3000s" in said),
            ("budget within noise is not called widened", "same 1000s" not in said),
            ("retry is priced in hours", "0.8 h of wall clock" in said),
            ("--retry-timeouts still replays both", replayed == []),
            ("no budgets, no price claimed", "narrower than" not in buf_blind.getvalue()),
        ]
    finally:
        shutil.rmtree(out_dir, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=None, help="path to lumen.exe (default: target/$LUMEN_PROFILE/lumen.exe)")
    parser.add_argument("--all", action="store_true", help="run every category in the manifest")
    parser.add_argument("--pilot", action="store_true", help=f"run the pilot selection ({len(PILOT_CATEGORIES)} categories)")
    parser.add_argument("--categories", default=None, help="comma-separated category list")
    parser.add_argument("--processes", type=int, default=7,
                        help="wptrunner --processes per shard (default: 7; six concurrent "
                             "`lumen` instances plus a shard's orphans pushed a 7.6 GB "
                             "Linux box into OOM — see kill_tree — pass a lower value "
                             "on a machine that small)")
    parser.add_argument("--out-dir", default=DEFAULT_OUT_DIR)
    parser.add_argument("--parallel-shards", type=int, default=1,
                        help="run this many shards at once, each on its own copy of the wptserve "
                             "ports (+1000 per lane) — WPT-RUN-9; memory, not CPU, is the limit: "
                             "see --min-free-gb (default: 1, the old sequential run)")
    parser.add_argument("--batch-small", type=int, default=0,
                        help="run consecutive shards of at most this many automatable ids as one "
                             "wptrunner process, split back per shard afterwards — saves a wptserve "
                             f"boot per shard (suggested: {BATCH_SMALL_DEFAULT}; default: 0, off)")
    parser.add_argument("--batch-max-ids", type=int, default=600,
                        help="upper bound on automatable ids in one --batch-small batch (default: 600)")
    parser.add_argument("--shared-queue", action="store_true",
                        help="hand a shard's tests to its --processes browsers from one queue, "
                             "longest declared timeout first, instead of wptrunner's fixed "
                             "hash split — no process sits idle while another works off a "
                             "pile of TIMEOUTs (see SHARED_QUEUE_ARGS; default: off)")
    parser.add_argument("--min-free-gb", type=float, default=6.0,
                        help="with --parallel-shards, a lane waits to start a shard while less "
                             "physical memory than this is free and another lane is busy "
                             "(default: 6; 0 disables the gate)")
    parser.add_argument("--max-browser-gb", type=float, default=browser_rss_cap.DEFAULT_CAP_GB,
                        help="kill a lumen of this run whose resident memory passes this many GB; "
                             "wptrunner reports its test CRASH and restarts the browser "
                             "(tests/wpt/browser_rss_cap.py — healthy browsers peak at ~1 GB, "
                             "the few runaways at 15-25 GB and TIMEOUT with no subtests anyway; "
                             f"default: {browser_rss_cap.DEFAULT_CAP_GB}, 0 disables)")
    parser.add_argument("--shard-timeout-base", type=int, default=600, help="fixed part of a shard's time budget, seconds (default: 600)")
    parser.add_argument("--shard-timeout-per-id", type=float, default=None,
                        help="flat per-id time budget, seconds; default is to derive the "
                             "budget from the declared per-test timeouts in the manifest "
                             "(see shard_timeout)")
    parser.add_argument("--prefixes", default=None,
                        help="comma-separated WPT path prefixes (e.g. css/css-flexbox) — run and score "
                             "only the ids under them, within the selected categories; the "
                             "category is implied when no --categories/--all is given "
                             "(WPT-RUN-14). Use a dedicated --out-dir: the filter is recorded in "
                             "state.json and --resume refuses a different one")
    parser.add_argument("--exclude-prefixes", default=None,
                        help="comma-separated WPT path prefixes to leave out of the selection "
                             "(applies on top of --prefixes; an exclusion always wins)")
    parser.add_argument("--resume", action="store_true", help="skip shards that already produced a report")
    parser.add_argument("--retry-timeouts", action="store_true",
                        help="on --resume, run budget-killed shards again instead of keeping "
                             "the results salvaged from their raw stream (use together with a "
                             "raised --shard-timeout-per-id; the previous stream is rotated, "
                             "so a shorter retry cannot lose results)")
    parser.add_argument("--aggregate-only", action="store_true", help="score existing reports in --out-dir, run nothing")
    parser.add_argument("--selftest", action="store_true",
                        help="check the coverage breakdown on a synthetic run and exit")
    parser.add_argument("--skip-manifest-update", action="store_true", help="trust MANIFEST.json as-is")
    parser.add_argument("--run-json", default=None, help="write the scored run snapshot here (docs/wpt/runs/<date>.json)")
    parser.add_argument("--no-port-guard", action="store_true",
                        help="do not verify the run owns the wptserve ports before each shard "
                             "(tests/wpt/port_guard.py); an escape hatch, not a speed-up — "
                             "without it a stranded server answers for the run and the number "
                             "describes files nobody chose")
    parser.add_argument("--skip-https", action="store_true",
                        help="do not run .https. tests (BUG-785 fixed 2026-08-20 — this now just "
                             "trades a slower, complete run for a faster, partial one); "
                             "they stay in the denominator and score 0, the summary says how many")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

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
    prefixes = parse_prefixes(args.prefixes)
    exclude_prefixes = parse_prefixes(args.exclude_prefixes)
    if os.path.isfile(state_path):
        with open(state_path, encoding="utf-8") as fh:
            recorded = json.load(fh)
        recorded_filter = (recorded.get("prefixes") or [], recorded.get("exclude_prefixes") or [])
        if args.aggregate_only and not (prefixes or exclude_prefixes):
            # Scoring an existing run: the filter belongs to the run, as the
            # binary and the commit do.
            prefixes, exclude_prefixes = recorded_filter
        elif (args.resume or args.aggregate_only) and recorded_filter != (prefixes, exclude_prefixes):
            print(f"{state_path} was written with --prefixes={recorded_filter[0]} "
                  f"--exclude-prefixes={recorded_filter[1]}; this command asks for "
                  f"{prefixes} / {exclude_prefixes}. Shards of the same name would cover "
                  f"different ids — use a separate --out-dir", file=sys.stderr)
            return 1

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
        elif prefixes and not (args.pilot or args.categories):
            # `--prefixes css/css-flexbox` alone: the category is the first path
            # component, nobody should have to say `--categories css` as well.
            categories = sorted({p.split("/")[0] for p in prefixes})
        elif args.pilot:
            categories = list(PILOT_CATEGORIES)
        elif args.categories:
            categories = [c.strip() for c in args.categories.split(",") if c.strip()]
        else:
            print("pick a selection: --all, --pilot or --categories", file=sys.stderr)
            return 1

        shards = plan_shards(manifest, categories, prefixes, exclude_prefixes)
        print(f"{len(shards)} shards, {sum(s['ids'] for s in shards)} manifest ids "
              f"({sum(s['auto_ids'] for s in shards)} automatable — the scored denominator), "
              f"--processes={args.processes}", flush=True)

        exclude_file = None
        if args.skip_https:
            skipped = https_ids(manifest, set(categories), prefixes, exclude_prefixes)
            exclude_file = os.path.join(args.out_dir, "exclude-https.txt")
            with open(exclude_file, "w", encoding="utf-8") as fh:
                fh.write("\n".join(skipped) + "\n")
            print(f"--skip-https: {len(skipped)} ids excluded from the run "
                  f"(still in the denominator, scored 0)", flush=True)

        shard_states = []
        if args.resume and os.path.isfile(state_path):
            with open(state_path, encoding="utf-8") as fh:
                previous = json.load(fh)["shards"]
            budgets = {s["name"]: shard_timeout(s, args.shard_timeout_base,
                                                args.shard_timeout_per_id, args.processes)
                       for s in shards}
            shard_states = resumable_states(previous, args.out_dir, args.retry_timeouts,
                                            budgets)
        done = {s["name"] for s in shard_states}
        for index, shard in enumerate(shards, 1):
            if shard["name"] in done:
                print(f"[{index}/{len(shards)}] {shard['name']}: cached", flush=True)
        pending = [s for s in shards if s["name"] not in done]
        units = plan_units(pending, args.batch_small, args.batch_max_ids, args.out_dir)
        if len(units) != len(pending):
            batches = [u for u in units if u.get("members")]
            print(f"--batch-small {args.batch_small}: {sum(len(b['members']) for b in batches)} "
                  f"small shards run as {len(batches)} batches — {len(units)} wptrunner "
                  f"processes instead of {len(pending)}", flush=True)

        def write_checkpoint():
            # Checkpoint after every unit: a corpus run outlives the session
            # that started it, and must be resumable from wherever it stopped.
            with open(state_path, "w", encoding="utf-8") as fh:
                json.dump({"binary": binary, "commit": run_commit, "shards": shard_states,
                           "prefixes": prefixes, "exclude_prefixes": exclude_prefixes,
                           "skipped_https": len(skipped) if args.skip_https else 0}, fh, indent=2)

        def finish(unit, state):
            """Record a finished unit — a batch as its member shards."""
            states = split_batch(unit, state, args.out_dir) if unit.get("members") else [state]
            shard_states.extend(states)
            write_checkpoint()
            return states

        rss_cap = browser_rss_cap.BrowserRssCap(args.max_browser_gb, args.out_dir)
        if rss_cap.start():
            print(f"--max-browser-gb {args.max_browser_gb}: a lumen of this run above it is "
                  f"killed (test -> CRASH), kills logged to {browser_rss_cap.KILLS_FILE}",
                  flush=True)
        else:
            rss_cap = None
        try:
            if args.parallel_shards > 1:
                status = run_units_parallel(units, binary, args, exclude_file, finish, rss_cap)
                if status:
                    return status
            for index, shard in enumerate(units if args.parallel_shards <= 1 else [], 1):
                # A shard that cannot bind its own ports does not fail — it is
                # answered by whatever holds them, and scores against files and
                # route parameters that belong to a run nobody is watching
                # (slice 18). Checking here rather than once at startup is what
                # catches the leak this run leaves behind itself.
                if not args.no_port_guard:
                    try:
                        port_guard.ensure_free(own_pid=os.getpid())
                    except port_guard.PortsBusy as exc:
                        print(f"\n{exc}", file=sys.stderr)
                        print(f"stopped before shard {index}/{len(units)} "
                              f"({shard['name']}); {len(shard_states)} shards are "
                              f"checkpointed — rerun the same command with --resume",
                              file=sys.stderr, flush=True)
                        return 1
                    # `kill_tree` reaps a timed-out shard's own `lumen` orphans
                    # immediately; this is the fallback for the ones it can't
                    # reach — a *previous, external* SIGKILL of this very process
                    # (session teardown, another OOM kill) skips `kill_tree`
                    # entirely, since a caught-nothing SIGKILL runs no Python at
                    # all (BUG-1029).
                    port_guard.reap_lumen_orphans(own_pid=os.getpid())
                budget = shard_timeout(shard, args.shard_timeout_base, args.shard_timeout_per_id,
                                       args.processes)
                print(f"[{index}/{len(units)}] {shard['name']}: {shard['ids']} ids (budget {budget}s) ...", end="", flush=True)
                # BUG-1029 §3: held per shard, not for the whole (possibly
                # multi-day, --resume'd) run — a shard's browsers are the memory
                # spike, and releasing between shards lets a build waiting on
                # scripts/cargo-heavy.sh get in during the gap instead of being
                # starved for as long as this corpus run keeps going.
                with heavy_lock.heavy_lock(f"run_corpus.py pid={os.getpid()} shard={shard['name']}"):
                    state = run_shard(shard, binary, args.out_dir, args.processes, budget,
                                      exclude_file, rss_cap=rss_cap,
                                      shared_queue=args.shared_queue)
                finish(shard, state)
                print(f" {state['outcome']} in {state['seconds']}s", flush=True)
        finally:
            if rss_cap is not None:
                rss_cap.stop()

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
    if scope and scope >= all_categories and not (prefixes or exclude_prefixes):
        scope = None
    scored = score_reports(manifest, args.out_dir, scope, shard_states, prefixes,
                           exclude_prefixes)
    if scope:
        print(f"\nscope: {len(scope)} of {len(all_categories)} categories "
              f"(partial run — denominator covers only what was selected)")
    if prefixes or exclude_prefixes:
        print(f"filter: --prefixes {','.join(prefixes) or '(none)'}"
              + (f" --exclude-prefixes {','.join(exclude_prefixes)}" if exclude_prefixes else "")
              + " — the denominator is the ids under it, not the whole category")
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
            "parallel_shards": getattr(args, "parallel_shards", 1),
            "batch_small": getattr(args, "batch_small", 0),
            "shared_queue": getattr(args, "shared_queue", False),
            "max_browser_gb": getattr(args, "max_browser_gb", 0),
            "rss_cap_kills": len(browser_rss_cap.load_kills(args.out_dir)),
            "scope": sorted(scope) if scope else "full-corpus",
            "prefixes": prefixes,
            "exclude_prefixes": exclude_prefixes,
            "shards": shard_states,
            "scored": scored,
        }
        with open(args.run_json, "w", encoding="utf-8") as fh:
            json.dump(snapshot, fh, indent=2, sort_keys=True)
        print(f"run snapshot: {args.run_json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
