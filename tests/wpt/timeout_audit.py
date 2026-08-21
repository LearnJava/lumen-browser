#!/usr/bin/env python3
"""WPT-RUN-6: what actually makes a corpus run TIMEOUT, read off the browser's
own output instead of guessed from the file name.

`WPT-RUN-6` slices 1-2 classified TIMEOUTs by the *suffix* of the test file
(`.worker.html`, `.https.*`, ...) with a throwaway script that was never
committed and got rewritten three times. A suffix is a proxy: it says which
family a test belongs to, not what went wrong inside it. This script uses the
direct evidence — every line the browser process printed while that specific
test was running — so a mechanism is attributed by its own error text
(`importScripts: cannot load script`, `peer closed connection without sending
TLS close_notify`, `subsetTest is not defined`, ...), and a test whose harness
loaded cleanly and still hung is separated from one that never got a harness at
all. The second group is the one `WPT-RUN-6` is actually hunting: an engine
defect rather than a hole in the runner's plumbing.

**How output is attributed to a test.** `wptrunner`'s structured log
(`.tmp/wpt-corpus/<shard>.raw.jsonl`, mozlog) carries `test_start`/`test_end`
on the `TestRunnerManager-N` thread that owns the test, but every
`process_output` line arrives on a single shared `ProcessReader` thread — so
the thread field cannot attribute browser output, and a run with
`--processes 6` interleaves six browsers into one log. Two fields make it
exact anyway: `process_output.process` is the browser pid, and
`test_end.extra.browser_pid` is the pid of the browser that ran the test. So a
line belongs to a test iff it came from that test's browser pid *and* its
timestamp falls inside `[test_start, test_end]`. Restarts (a new browser pid
mid-shard) fall out for free, because the pid changes with them.

**Mechanisms** are a precedence-ordered table (`MECHANISMS`): the first pattern
that matches any line of a test's output wins. Order encodes causality, not
importance — a test whose `testharness.js` was truncated by the TLS defect
(BUG-792) *also* prints `add_completion_callback is not defined` a moment
later, and reporting it as "missing harness global" would be reporting the
symptom. Network-layer causes therefore sort above JS-level ones.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/timeout_audit.py [--out-dir .tmp/wpt-corpus]
        [--category PREFIX] [--top N] [--examples K] [--json OUT]
    <venv>/python tests/wpt/timeout_audit.py --selftest

Safe to run against a live run's `--out-dir`: it only reads. A shard still
being written is read as far as it got (a truncated last line is skipped).
"""

import argparse
import ast
import bisect
import collections
import glob
import json
import os
import re
import sys
import tempfile

#: Normalization applied before a line is used as a signature: concrete URLs,
#: numbers and pids differ per test and would explode the histogram.
_URL_RE = re.compile(r"https?://[^\s\"']+")
_NUM_RE = re.compile(r"\d+")

#: How much of a normalized line is kept. Long lines here are stack-ish tails
#: of network errors; the head carries the mechanism.
_SIG_LEN = 160


class Mechanism:
    """One recognizable cause of a TIMEOUT.

    `patterns` are matched against the raw (un-normalized) output lines of a
    single test; the first mechanism in `MECHANISMS` with any match wins.
    `ref` names the bug or task that owns the fix, so the report doubles as a
    work list — an unowned mechanism is a bug nobody has filed yet.
    """

    def __init__(self, key, ref, patterns, note="", mode="any"):
        self.key = key
        self.ref = ref
        self.patterns = [re.compile(p) for p in patterns]
        self.note = note
        #: "any" — one matching pattern is enough (an error line names its own
        #: cause). "all" — every pattern must match somewhere, used by the
        #: source stage where a single marker is too weak on its own
        #: (`.focus(` is everywhere; `.focus(` *and* a `load` handler is the
        #: BUG-794 shape).
        self.mode = mode

    def matches(self, lines):
        hits = 0
        for pat in self.patterns:
            if any(pat.search(line) for line in lines):
                if self.mode == "any":
                    return True
                hits += 1
        return self.mode == "all" and hits == len(self.patterns)


#: Precedence-ordered; see the module docstring on why network causes sort
#: above JS-level symptoms.
MECHANISMS = [
    Mechanism(
        "https-body-truncated", "BUG-792",
        [r"close_notify"],
        "peer closed the TLS connection without close_notify — the response "
        "body is dropped, so an https-served harness/helper never arrives",
    ),
    Mechanism(
        "adblock-blocked", "BUG-800 (FIXED)",
        [r"blocked: easylist", r"blocked: [a-z0-9-]+list"],
        "a request the test needs was eaten by the built-in ad blocker",
    ),
    Mechanism(
        "foreign-host-unresolvable", "WPT-RUN-10",
        [r"failed to lookup address information",
         r"getaddrinfo", r"Name or service not known", r"os error 11001"],
        "a cross-origin WPT host (www1.*, not-web-platform.test) has no "
        "resolution on this machine",
    ),
    Mechanism(
        "worker-importscripts", "BUG-778/BUG-591",
        [r"importScripts: cannot load script", r"importScripts is not supported"],
        "a dedicated/shared/service worker cannot import its harness, and the "
        "failure never reaches the parent",
    ),
    Mechanism(
        "helper-404", "WPT-RUN-11",
        [r"Пропуск скрипта .* network error: HTTP \d",
         r"Ошибка загрузки .* network error: HTTP \d",
         r"script load failed",
         r"JS runtime error: Unexpected token '<'"],
        "a referenced helper is absent from the vendored checkout, so the "
        "harness crashes before registering a single test",
    ),
    Mechanism(
        "helper-global-missing", "WPT-RUN-11",
        [r"JS runtime error: (?:add_completion_callback|subsetTest|idl_test|"
         r"SanityChecker|fetch_tests_from_worker|RemoteContext|promise_test|"
         r"promise_setup|async_test|setup|test) is not defined"],
        "a harness global is undefined with no network error of its own — the "
        "defining helper was served but did not run, or is unvendored",
    ),
    Mechanism(
        "defaultview-test-driver", "BUG-622",
        [r"Cannot read properties of undefined \(reading 'test_driver'\)"],
        "`document.defaultView` is absent, so WPT helpers that reach through "
        "it (editing/editor-test-utils.js) throw before any test() runs",
    ),
    Mechanism(
        "mixed-content-blocked", "BUG-796",
        [r"mixed-content: blockable"],
        "the test's own subresource is blocked as mixed content because the "
        "page was reached over https",
    ),
    Mechanism(
        "worker-unsupported-api", "BUG-591",
        [r"\[worker-\d+\].*script error", r"\[shared-worker\].*script error",
         r"\[sw .*script eval error"],
        "worker script threw for a reason other than importScripts, and the "
        "exception is swallowed instead of failing the parent test",
    ),
    Mechanism(
        "unsupported-scheme", "BUG-651",
        [r"network error: unsupported scheme"],
        "navigation or fetch to a scheme the network stack does not handle "
        "(about:, data:) — reported as a network failure",
    ),
    Mechanism(
        "websocket", "BUG-799",
        [r"\[JS WebSocket\].*error"],
        "the page's WebSocket never connected",
    ),
]

#: Tests that printed nothing at all while running get their own bucket: the
#: browser produced no evidence, which is itself a finding (a hang before the
#: first log line, or a test whose whole body is a wait).
NO_OUTPUT = "no-output"

#: Tests with output that no mechanism claims. This is the WPT-RUN-6 residual —
#: the engine-level hangs still to be characterized.
UNCLASSIFIED = "unclassified"

#: Second stage, applied only to tests the output-based table could not claim.
#:
#: The largest mechanisms WPT-RUN-6 has already found are *silent*: a missing
#: `document.fonts.ready` (BUG-564) or an `<iframe>` with no nested browsing
#: context (BUG-480) produces no error line at all — the page simply waits for
#: an event that never fires, and the browser has nothing to report. Those can
#: only be recognized from what the test asks for, so this stage greps the test
#: source. Precedence-ordered like `MECHANISMS`, and deliberately a *lower*
#: bound: a marker reached through a helper file this stage does not open is
#: missed, the same limitation `host_audit.py` carries.
SOURCE_MARKERS = [
    Mechanism(
        "fonts-ready", "BUG-564", [r"document\.fonts\.ready"],
        "`document.fonts.ready` is undefined, so the promise chain every test "
        "in the file hangs off never starts",
    ),
    Mechanism(
        "iframe-no-nested-context", "BUG-480",
        [r"<iframe", r"iframe\.(?:onload|contentWindow|contentDocument)|"
         r"\bsrcdoc\b|addEventListener\(['\"]load['\"]"],
        "URL-addressed subdocuments are never loaded, so `iframe.onload` and "
        "`contentWindow` access wait forever",
        mode="all",
    ),
    Mechanism(
        "img-no-load-event", "BUG-630",
        [r"new Image\(|<img", r"\.onload|\.complete\b|naturalWidth|"
         r"addEventListener\(['\"]load['\"]"],
        "`<img>` dispatches neither `load` nor `error` and exposes no "
        "`complete`/`naturalWidth`",
        mode="all",
    ),
    Mechanism(
        "window-open-stub", "BUG-797",
        [r"\bwindow\.open\(|\bRemoteContext\b|\bBroadcastChannel\b"],
        "`window.open()` returns a stub with a no-op `postMessage`, so any "
        "cross-window channel the test builds is dead",
    ),
    Mechanism(
        "track-element", "BUG-795", [r"<track|\.textTracks\b"],
        "`<track>` never dispatches `load`/`error` and has no `.track`",
    ),
    Mechanism(
        "media-element", "BUG-799 / video Phase 1",
        [r"<video|<audio|createElement\(['\"](?:video|audio)['\"]\)",
         r"loadeddata|loadedmetadata|canplaythrough|\.play\(\)"],
        "real media decoding is Phase 1 (`video_bindings.rs`) — a test waiting "
        "for a media event on an mp4/mp3 waits forever",
        mode="all",
    ),
    Mechanism(
        "focus-in-load", "BUG-794",
        [r"\.focus\(", r"addEventListener\(['\"]load['\"]|onload="],
        "`element.focus()` called synchronously from a `load` handler never "
        "returns",
        mode="all",
    ),
]



def normalize(line):
    """Collapse a raw output line into a comparable signature."""
    return _NUM_RE.sub("N", _URL_RE.sub("<URL>", line)).strip()[:_SIG_LEN]


def category_of(test_id):
    """Two-level category of a manifest id (`/html/canvas/foo.html` → `html/canvas`)."""
    parts = test_id.strip("/").split("/")
    return "/".join(parts[:2]) if len(parts) > 1 else parts[0]


#: The vendored WPT tree: this script's own directory. A manifest id is a
#: server path rooted here.
WPT_ROOT = os.path.dirname(os.path.abspath(__file__))

#: `.any.js`/`.window.js`/`.worker.js` sources are expanded by the manifest
#: into several generated ids; the marker stage has to read the source they
#: were generated from, not a file that does not exist on disk.
_GENERATED_SUFFIXES = [
    (".any.worker.html", ".any.js"),
    (".any.sharedworker.html", ".any.js"),
    (".any.serviceworker.html", ".any.js"),
    (".any.worker-module.html", ".any.js"),
    (".any.html", ".any.js"),
    (".window.html", ".window.js"),
    (".worker.html", ".worker.js"),
]


def source_path(test_id, root=WPT_ROOT):
    """On-disk source file a manifest id was generated from."""
    path = test_id.split("?")[0].strip("/")
    for generated, source in _GENERATED_SUFFIXES:
        if path.endswith(generated):
            path = path[: -len(generated)] + source
            break
    return os.path.join(root, path)


def _read_source(path, cache):
    if path not in cache:
        try:
            with open(path, encoding="utf-8", errors="replace") as handle:
                cache[path] = handle.read().splitlines()
        except OSError:
            cache[path] = None
    return cache[path]


def classify_source(test_id, root, cache):
    """Second-stage key for a test the output stage could not claim, or None.

    Reads only the test's own source file — a marker reached through a helper
    the test includes is missed, which keeps this a lower bound (see
    `SOURCE_MARKERS`).
    """
    lines = _read_source(source_path(test_id, root), cache)
    if not lines:
        return None
    for mech in SOURCE_MARKERS:
        if mech.matches(lines):
            return mech.key
    return None


def _parse_extra(extra):
    """`test_end.extra` is a dict in JSON logs and a repr string in some mozlog
    versions; accept both, never raise."""
    if isinstance(extra, dict):
        return extra
    if isinstance(extra, str):
        try:
            return ast.literal_eval(extra)
        except (ValueError, SyntaxError):
            return {}
    return {}


def read_shard(path):
    """Yield `(test_id, status, [output lines])` for every test in one shard log.

    See the module docstring for the pid+time-window attribution rule.
    """
    out_by_pid = collections.defaultdict(list)
    tests = []
    open_by_thread = {}
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            # `test_status` (one per subtest) is the bulk of the file and is
            # never needed here — skipping it before json.loads roughly halves
            # the parse cost of a 2.7 GB corpus run.
            if '"test_status"' in line:
                continue
            try:
                event = json.loads(line)
            except ValueError:
                continue
            action = event.get("action")
            if action == "process_output":
                try:
                    stamp = int(event["time"])
                except (KeyError, ValueError, TypeError):
                    continue
                out_by_pid[str(event.get("process"))].append((stamp, event.get("data") or ""))
            elif action == "test_start":
                open_by_thread[event.get("thread")] = (event.get("test"), int(event.get("time", 0)))
            elif action == "test_end":
                name, start = open_by_thread.pop(event.get("thread"),
                                                (event.get("test"), 0))
                extra = _parse_extra(event.get("extra"))
                tests.append((str(extra.get("browser_pid")), start,
                              int(event.get("time", 0)),
                              event.get("test") or name, event.get("status")))
    for pid in out_by_pid:
        out_by_pid[pid].sort()
    for pid, start, end, name, status in tests:
        events = out_by_pid.get(pid) or []
        stamps = [stamp for stamp, _ in events]
        left = bisect.bisect_left(stamps, start)
        right = bisect.bisect_right(stamps, end)
        yield name, status, [data for _, data in events[left:right] if data.strip()]


def classify(lines):
    """Return the key of the first mechanism claiming these output lines."""
    if not lines:
        return NO_OUTPUT
    for mech in MECHANISMS:
        if mech.matches(lines):
            return mech.key
    return UNCLASSIFIED


def audit(out_dir, category=None, root=WPT_ROOT, use_source=True):
    """Classify every TIMEOUT in a run directory.

    `category` filters by manifest-id prefix (`html`, `html/canvas`, ...);
    `use_source` enables the second, source-marker stage (`SOURCE_MARKERS`).
    """
    source_cache = {}
    counts = collections.Counter()
    by_cat = collections.defaultdict(collections.Counter)
    residual_sigs = collections.Counter()
    examples = collections.defaultdict(list)
    residual_examples = collections.defaultdict(list)
    residual_ids = []
    totals = collections.Counter()

    for path in sorted(glob.glob(os.path.join(out_dir, "*.raw.jsonl"))):
        for test, status, lines in read_shard(path):
            if not test:
                continue
            if category and not test.strip("/").startswith(category.strip("/")):
                continue
            totals[status] += 1
            if status != "TIMEOUT":
                continue
            key = classify(lines)
            if use_source and key in (NO_OUTPUT, UNCLASSIFIED):
                from_source = classify_source(test, root, source_cache)
                if from_source:
                    key = from_source
            counts[key] += 1
            by_cat[category_of(test)][key] += 1
            if len(examples[key]) < 200:
                examples[key].append(test)
            if key in (UNCLASSIFIED, NO_OUTPUT):
                # The full residual list, not a sample: the next slice picks
                # its target out of it, and 3k ids is small next to the run.
                residual_ids.append(test)
            if key == UNCLASSIFIED:
                for sig in sorted({normalize(line) for line in lines}):
                    residual_sigs[sig] += 1
                    if len(residual_examples[sig]) < 5:
                        residual_examples[sig].append(test)
    return {
        "out_dir": out_dir,
        "statuses": dict(totals),
        "timeouts": sum(counts.values()),
        "mechanisms": dict(counts),
        "by_category": {cat: dict(c) for cat, c in by_cat.items()},
        "residual_signatures": dict(residual_sigs),
        "residual_ids": residual_ids,
        "examples": {k: v for k, v in examples.items()},
        "residual_examples": {k: v for k, v in residual_examples.items()},
    }


def print_report(result, top=25, examples=3):
    """Human-readable form of `audit()`."""
    total = result["timeouts"]
    print(f"run: {result['out_dir']}")
    statuses = result["statuses"]
    ran = sum(statuses.values())
    print(f"tests with a verdict: {ran}   TIMEOUT: {total} "
          f"({100.0 * total / ran:.1f}%)" if ran else "no tests")
    print()
    refs = {m.key: m.ref for m in MECHANISMS}
    refs.update({m.key: m.ref + " (source marker)" for m in SOURCE_MARKERS})
    print(f"{'mechanism':28} {'tests':>7} {'share':>7}  owner")
    for key, count in sorted(result["mechanisms"].items(), key=lambda kv: -kv[1]):
        share = 100.0 * count / total if total else 0.0
        print(f"{key:28} {count:7d} {share:6.1f}%  {refs.get(key, '—')}")
        for test in result["examples"].get(key, [])[:examples]:
            print(f"{'':28} {'':7}          {test}")
    print()
    print(f"unclassified by category (top {top}):")
    rows = [(cat, c.get(UNCLASSIFIED, 0), sum(c.values()))
            for cat, c in result["by_category"].items()]
    rows.sort(key=lambda r: -r[1])
    for cat, unc, tot in rows[:top]:
        if not unc:
            continue
        print(f"  {unc:6d} of {tot:6d} TIMEOUT   {cat}")
    print()
    print(f"top signatures inside the residual (top {top}):")
    for sig, count in sorted(result["residual_signatures"].items(),
                             key=lambda kv: -kv[1])[:top]:
        print(f"  {count:6d}  {sig}")
        for test in result["residual_examples"].get(sig, [])[:1]:
            print(f"          e.g. {test}")


def _write_selftest_shard(path):
    """Synthetic mozlog shard exercising every attribution rule the audit makes.

    Six tests, one shard, two browser pids running concurrently — the shape a
    `--processes` run produces and the reason thread-based attribution fails.
    """
    events = []

    def out(pid, time, data):
        events.append({"action": "process_output", "time": str(time),
                       "thread": "ProcessReader", "process": pid, "data": data})

    def start(thread, time, test):
        events.append({"action": "test_start", "time": str(time),
                       "thread": thread, "test": test})

    def end(thread, time, test, status, pid):
        events.append({"action": "test_end", "time": str(time), "thread": thread,
                       "test": test, "status": status,
                       "extra": {"test_timeout": 10, "browser_pid": pid}})

    # 1. precedence: the TLS defect truncates testharness.js, and the missing
    #    global is the downstream symptom — must report BUG-792, not WPT-RUN-11.
    start("TestRunnerManager-0", 100, "/a/tls.https.html")
    out("500", 110, "Пропуск скрипта http://x/resources/testharness.js network error: "
                    "read body: peer closed connection without sending TLS close_notify: x")
    out("500", 120, "script error: JS runtime error: add_completion_callback is not defined")
    end("TestRunnerManager-0", 200, "/a/tls.https.html", "TIMEOUT", 500)

    # 2. interleaved second browser: its lines must not leak into test 1 or 3
    #    even though they share the ProcessReader thread and the time window.
    start("TestRunnerManager-1", 105, "/b/worker.worker.html")
    out("501", 115, '[worker-1] v1 script error: Runtime("importScripts: cannot '
                    'load script: /resources/testharness.js")')
    end("TestRunnerManager-1", 205, "/b/worker.worker.html", "TIMEOUT", 501)

    # 3. same pid as test 1, later window: output before its start belongs to
    #    test 1 only.
    start("TestRunnerManager-0", 300, "/a/clean.html")
    out("500", 310, "Распарсено: 42 DOM-узлов, 1 CSS-правил")
    end("TestRunnerManager-0", 400, "/a/clean.html", "TIMEOUT", 500)

    # 4. no output at all while running.
    start("TestRunnerManager-1", 500, "/b/silent.html")
    end("TestRunnerManager-1", 600, "/b/silent.html", "TIMEOUT", 501)

    # 5. not a TIMEOUT — must not be classified at all.
    start("TestRunnerManager-0", 700, "/a/ok.html")
    out("500", 710, "script error: JS runtime error: subsetTest is not defined")
    end("TestRunnerManager-0", 800, "/a/ok.html", "OK", 500)

    # 6. cross-origin host with no resolution.
    start("TestRunnerManager-1", 900, "/b/origin.html")
    out("501", 910, "✗ http://www1.127.0.0.1:18300/x (dns: resolve www1.127.0.0.1:18300: "
                    "network error: resolve www1.127.0.0.1: failed to lookup address information)")
    end("TestRunnerManager-1", 1000, "/b/origin.html", "TIMEOUT", 501)

    with open(path, "w", encoding="utf-8") as handle:
        for event in events:
            handle.write(json.dumps(event, ensure_ascii=False) + "\n")
        handle.write('{"action": "test_end", "time": "1', )  # truncated tail


def selftest():
    """Assertions over the synthetic shard. Every one has been checked to fail
    if the rule it guards is removed."""
    failures = []

    def check(cond, msg):
        if not cond:
            failures.append(msg)

    with tempfile.TemporaryDirectory() as tmp:
        _write_selftest_shard(os.path.join(tmp, "synthetic.raw.jsonl"))
        result = audit(tmp, use_source=False)

        mech = result["mechanisms"]
        check(result["timeouts"] == 5,
              f"expected 5 TIMEOUTs (the OK test excluded), got {result['timeouts']}")
        check(result["statuses"].get("OK") == 1, "the OK test must still be counted")
        check(mech.get("https-body-truncated") == 1,
              f"TLS truncation must win over the missing global: {mech}")
        check(mech.get("helper-global-missing") is None,
              f"the downstream symptom must not get its own row: {mech}")
        check(mech.get("worker-importscripts") == 1,
              f"worker importScripts not attributed: {mech}")
        check(mech.get("foreign-host-unresolvable") == 1,
              f"unresolvable host not attributed: {mech}")
        check(mech.get(NO_OUTPUT) == 1, f"silent test not bucketed: {mech}")
        check(mech.get(UNCLASSIFIED) == 1,
              f"the clean-but-hung test is the residual: {mech}")
        check(result["examples"].get(UNCLASSIFIED) == ["/a/clean.html"],
              "residual example must be the clean test, not a neighbour's")
        # Attribution, not just classification: the worker line came from pid
        # 501 inside test 1's time window, so a thread- or window-only rule
        # would have pulled it into /a/tls.https.html.
        check("/b/worker.worker.html" in result["examples"].get("worker-importscripts", []),
              "worker line attributed to the wrong test")
        sigs = result["residual_signatures"]
        check(any("Распарсено" in s for s in sigs),
              f"residual signature histogram is empty: {sigs}")
        check(all("importScripts" not in s for s in sigs),
              "another browser's line leaked into the residual histogram")

        # Stage 2: the same silent test, now with a source file on disk that
        # names a known-silent mechanism. `/a/clean.html` printed nothing an
        # error table could use; only the source says why it hung.
        os.makedirs(os.path.join(tmp, "a"), exist_ok=True)
        with open(os.path.join(tmp, "a", "clean.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>document.fonts.ready.then(() => done());</script>")
        with open(os.path.join(tmp, "a", "focus.window.js"), "w", encoding="utf-8") as handle:
            handle.write("addEventListener('load', () => { el.focus(); });")
        sourced = audit(tmp, root=tmp)
        check(sourced["mechanisms"].get("fonts-ready") == 1,
              f"source stage did not claim the silent test: {sourced['mechanisms']}")
        check(sourced["mechanisms"].get(UNCLASSIFIED) is None,
              f"source stage must consume the residual it explains: {sourced['mechanisms']}")
        check(result["mechanisms"].get(UNCLASSIFIED) == 1,
              "--no-source must leave the same test unclassified")
        # A marker that needs two conditions must not fire on one of them: the
        # silent test has no `.focus(` at all, so `mode="all"` must hold.
        check(sourced["mechanisms"].get("focus-in-load") is None,
              f"an unrelated all-mode marker fired: {sourced['mechanisms']}")
        check(classify_source("/a/focus.window.html", tmp, {}) == "focus-in-load",
              "generated id was not mapped back to its .window.js source")

    for msg in failures:
        print("FAIL:", msg)
    print("selftest:", "ok" if not failures else f"{len(failures)} failure(s)")
    return 1 if failures else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out-dir", default=".tmp/wpt-corpus",
                        help="run directory with <shard>.raw.jsonl files")
    parser.add_argument("--category", default=None,
                        help="restrict to a manifest-id prefix (html, html/canvas, ...)")
    parser.add_argument("--top", type=int, default=25, help="rows per ranked table")
    parser.add_argument("--examples", type=int, default=3,
                        help="example ids printed per mechanism")
    parser.add_argument("--json", dest="json_out", default=None,
                        help="write the full result here")
    parser.add_argument("--no-source", action="store_true",
                        help="skip the source-marker stage (output evidence only)")
    parser.add_argument("--selftest", action="store_true",
                        help="run the built-in assertions and exit")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if not glob.glob(os.path.join(args.out_dir, "*.raw.jsonl")):
        print(f"no *.raw.jsonl in {args.out_dir} — point --out-dir at a run "
              f"directory (run_corpus.py writes one)", file=sys.stderr)
        return 2

    result = audit(args.out_dir, category=args.category,
                   use_source=not args.no_source)
    print_report(result, top=args.top, examples=args.examples)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, ensure_ascii=False, indent=1)
        print(f"\nwrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
