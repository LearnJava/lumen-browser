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

**Collateral damage.** A hung browser is not restarted between the tests of a
shard, so every test still queued for that process times out too — printing
nothing at all, because the process is already wedged. Those tests are not
defects of their own and counting them as one inflates whatever mechanism the
source-marker stage happens to find in them. A zero-output TIMEOUT that
follows another TIMEOUT on the same browser pid is therefore reported as
`hung-browser` and pinned to its culprit: the last test that process was still
able to say anything about. In the WPT-RUN-5 Linux snapshot that is 695 of
15 592 TIMEOUTs (4.5 %) produced by exactly 16 hung processes, every one of
them a page that hangs standalone under `--dump-layout` (WPT-RUN-6 slice 11).
The culprit list is the actionable output — `hung_browsers` in `--json`.

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

    def __init__(self, key, ref, patterns, note="", mode="any", predicate=None):
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
        #: Optional callable over the joined source text, used instead of
        #: `patterns` when a line-at-a-time regex cannot express the marker.
        #: BUG-804 needs it: `createElement('style')` alone is not evidence —
        #: `resources/check-layout-th.js` mints one to highlight failures and
        #: never waits on it — so the wait has to be tied to the *same*
        #: variable, which is a two-step match across lines.
        self.predicate = predicate

    def matches(self, lines):
        if self.predicate is not None:
            return self.predicate("\n".join(lines))
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
    # Last on purpose: a rejection is the *terminal* event of almost any
    # failure — a 404'd helper, a truncated https body and a missing DOM API
    # all end in one — so every mechanism with its own evidence must claim the
    # test first. Only produced by a run instrumented with
    # `tests/wpt/rejection_trace.py`; a normal run never prints this line,
    # because BUG-716 means nothing in the engine reports a rejection at all.
    Mechanism(
        "unhandled-rejection", "BUG-716",
        [r"LUMEN_UNHANDLED_REJECTION:"],
        "the test failed inside a promise chain and Lumen never dispatched "
        "`unhandledrejection`, so testharness.js never saw the failure and the "
        "verdict degraded from FAIL to TIMEOUT (instrumented runs only)",
    ),
]

#: Tests that printed nothing at all while running get their own bucket: the
#: browser produced no evidence, which is itself a finding (a hang before the
#: first log line, or a test whose whole body is a wait).
NO_OUTPUT = "no-output"

#: Tests with output that no mechanism claims. This is the WPT-RUN-6 residual —
#: the engine-level hangs still to be characterized.
UNCLASSIFIED = "unclassified"

#: Collateral: a silent TIMEOUT on a browser that had already timed out and was
#: never restarted. See "Collateral damage" in the module docstring — these are
#: victims of one culprit page, not 695 separate defects.
HUNG_BROWSER = "hung-browser"

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

#: An event-handler attribute on a parser-written `<style>`/`<link>`/`<script>`.
#: The attribute form is the marker precisely because it can only come from
#: markup: an element the parser wrote is exactly the one the engine's resource
#: machinery never reaches (BUG-804).
_RESOURCE_EVENT_ATTR_RE = re.compile(
    r"<(?:style|link|script)[^>]*\son(?:load|error)\s*=", re.IGNORECASE)

#: `x = document.createElement('style')`, capturing the variable so the wait
#: can be required on the same one.
_CREATE_STYLE_RE = re.compile(
    r"(?:var|let|const)?\s*(\w+)\s*=\s*document\.createElement\("
    r"['\"]style['\"]\)", re.IGNORECASE)


def _waits_on_var(text, var):
    """True if `text` hangs a `load`/`error` handler off `var` specifically."""
    name = re.escape(var)
    return re.search(
        r"\b%s\s*\.\s*(?:onload|onerror)\b|"
        r"\b%s\s*\.\s*addEventListener\(\s*['\"](?:load|error)['\"]" % (name, name),
        text) is not None


def _resource_event_marker(text):
    """BUG-804 marker: the test waits for a `load`/`error` the engine never fires.

    Two shapes, both verified live (WPT-RUN-6 slice 12). The attribute shape is
    parser-inserted by construction. The `createElement('style')` shape is the
    one case where even the working `createElement` path stays silent, because
    `_lumen_resource_track` whitelists `script`/`link` only — but a created
    `<style>` is only evidence when something actually waits on *it*, hence
    `_waits_on_var`.

    Deliberately NOT matched: `createElement('script'|'link')`. That path fires
    `load` correctly (BUG-571/BUG-722), confirmed by the same A/B page.
    """
    if _RESOURCE_EVENT_ATTR_RE.search(text):
        return True
    return any(_waits_on_var(text, var) for var in _CREATE_STYLE_RE.findall(text))


#: `x = new Audio()` / `x = document.createElement('audio')`, capturing the
#: variable so the `src` assignment can be required on that same one.
_CREATE_AUDIO_RE = re.compile(
    r"(?:var|let|const)?\s*(\w+)\s*=\s*(?:new Audio\(\s*\)|"
    r"document\.createElement\(['\"]audio['\"]\))", re.IGNORECASE)


def _audio_src_marker(text):
    """BUG-799 marker: the page gives an `<audio>` element a `src`.

    That single act freezes the page — `AudioPlaybackProvider::load` deadlocks
    on its own `handles` mutex and never returns, so the assignment never
    completes and no later script, timer or harness callback runs (verified by
    `tests/wpt/verify_bug799_audio_timers.py`, WPT-RUN-6 slice 13). It is
    therefore *terminal and early*: whatever else the test would have waited
    for never gets the chance, which is why this marker sorts first.

    All three shapes have to be recognized, because the shim funnels them into
    the same `startLoad`: the markup attribute, `new Audio(url)` (the
    constructor assigns `el.src`), and a `src` assignment on an element built
    in script. A bare `new Audio()` is deliberately not enough — that shape is
    healthy, `audio-no-src` in the probe's own table.
    """
    if re.search(r"<audio[^>]*\ssrc\s*=", text, re.IGNORECASE):
        return True
    if re.search(r"new Audio\(\s*['\"]", text):
        return True
    return any(re.search(rf"\b{re.escape(var)}\s*\.\s*src\s*=", text)
               for var in _CREATE_AUDIO_RE.findall(text))


SOURCE_MARKERS = [
    # First on purpose: see `_audio_src_marker` — the page stops dead at the
    # `src` assignment, before anything else it was going to do.
    Mechanism(
        "audio-src-deadlock", "BUG-799", [],
        "giving an `<audio>` a `src` never returns (deadlock in the audio "
        "provider), so the whole page freezes on the spot",
        predicate=_audio_src_marker,
    ),
    Mechanism(
        "fonts-ready", "BUG-564", [r"document\.fonts\.ready"],
        "`document.fonts.ready` is undefined, so the promise chain every test "
        "in the file hangs off never starts",
    ),
    Mechanism(
        "iframe-no-nested-context", "BUG-480",
        # `createElement("iframe")` counts as much as the literal tag: the
        # helpers that build the frame in script (WPT's own
        # `moving-between-documents-helper.js`, `speculative-parsing-util.js`)
        # never write one out (WPT-RUN-6 slice 10).
        # The wait is as often for the frame's *message* as for its `load`:
        # a sandboxed frame cannot be reached through `contentWindow` at all,
        # so `iframe_sandbox_*` posts its result back and the parent listens
        # (`onmessage = t.step_func_done(...)`). Requiring a `load`/DOM-access
        # wait missed all ten of those (WPT-RUN-6 slice 13).
        [r"<iframe|createElement\(['\"]iframe['\"]\)",
         r"iframe\.(?:onload|contentWindow|contentDocument)|"
         r"\.contentWindow\b|\.contentDocument\b|"
         r"\bsrcdoc\b|addEventListener\(['\"]load['\"]|"
         r"\bonmessage\s*=|addEventListener\(['\"]message['\"]|"
         r"\bframes\[|\bwindow\.frames\b"],
        "URL-addressed subdocuments are never loaded, so `iframe.onload` and "
        "`contentWindow` access wait forever",
        mode="all",
    ),
    Mechanism(
        "img-no-load-event", "BUG-630",
        [r"new Image\(|<img|createElement\(['\"]img['\"]\)",
         r"\.onload|\.complete\b|naturalWidth|"
         r"addEventListener\(['\"]load['\"]"],
        "`<img>` dispatches neither `load` nor `error` and exposes no "
        "`complete`/`naturalWidth`",
        mode="all",
    ),
    Mechanism(
        "embed-object-no-load", "BUG-798",
        [r"<embed|<object|createElement\(['\"](?:embed|object)['\"]\)",
         r"\.onload|\.onerror|addEventListener\(['\"](?:load|error)['\"]"],
        "`<embed>`/`<object>` have no resource-loading path at all, so neither "
        "`load` nor `error` ever fires on them",
        mode="all",
    ),
    Mechanism(
        "window-open-stub", "BUG-797",
        [r"\bwindow\.open\(|\bRemoteContext\b|\bBroadcastChannel\b"],
        "`window.open()` returns a stub with a no-op `postMessage`, so any "
        "cross-window channel the test builds is dead",
    ),
    Mechanism(
        # `createElement("track")` is the shape the whole `track-webvtt-*`
        # family uses: `track-helpers.js::check_cues_from_track` builds the
        # `<video>`/`<track>` pair in script and hangs the test off
        # `trackElement.onload` (WPT-RUN-6 slice 13).
        "track-element", "BUG-795",
        [r"<track|\.textTracks\b|createElement\(['\"]track['\"]\)"],
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
    # Last on purpose: this marker is narrow, so anything a broader mechanism
    # can explain (a test that also builds an `<iframe>`, say) should be
    # reported under that one instead.
    Mechanism(
        "resource-no-load-event", "BUG-804", [],
        "`<script>`/`<link>`/`<style>` written by the parser dispatch neither "
        "`load` nor `error` (and `<style>` never does, however inserted)",
        predicate=_resource_event_marker,
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


#: `<script src=...>` of a test file. Both shapes occur (quoted and bare).
_SCRIPT_SRC_RE = re.compile(r"""<script[^>]*\ssrc\s*=\s*["']?([^"'\s>]+)""",
                            re.IGNORECASE)

#: `// META: script=...` — how a *generated* test (`.any.js`, `.window.js`,
#: `.worker.js`) declares its helpers. There is no `<script src>` anywhere in
#: such a file: wptrunner's manifest step reads these comments and writes the
#: `<script>` tags into the HTML it synthesizes, so reading the `.js` source
#: alone finds no helper at all (WPT-RUN-6 slice 13).
_META_SCRIPT_RE = re.compile(r"^\s*//\s*META:\s*script=(\S+)", re.MULTILINE)

#: Never followed as a helper: the harness itself matches half the markers
#: (`testharness.js` contains `addEventListener('load'`, `<iframe`, ...), so
#: following it would claim every test in the corpus for whichever mechanism
#: sorts first.
_NOT_A_HELPER = ("/resources/testharness.js", "/resources/testharnessreport.js",
                 "/resources/testdriver.js", "/resources/testdriver-vendor.js")


def helper_paths(test_id, root):
    """Local helpers of a test (`<script src>` and `// META: script=`), as paths.

    A generated test is frequently a three-line stub whose whole body is one
    call into a shared helper — `moving-between-documents-helper.js` builds the
    `<iframe>` and waits for its `postMessage`, `speculative-parsing-util.js`
    hangs the assertion off `addEventListener('load')`. Reading only the stub
    finds no marker at all, so those land in the residual as if the browser had
    gone silent for an unknown reason (WPT-RUN-6 slice 10: 54 of the 107
    `scripting-1` residual ids were one such helper). One level is followed,
    not a full closure: helpers of helpers are rare and each level widens what
    a marker may be attributed to.

    Both include shapes have to be read, because they never coexist: a `.html`
    test writes `<script src>`, while a `.window.js`/`.any.js`/`.worker.js`
    source has no markup at all and declares the very same helpers as
    `// META: script=` comments. Ignoring the second shape made the stage blind
    to *every* generated test whose evidence lives in a helper — the ten
    `sandbox-top-navigation-*.window.js` ids of `the-iframe-element` build their
    `<iframe>` in `remote-context-helper.js` and matched nothing at all
    (WPT-RUN-6 slice 13).
    """
    src = source_path(test_id, root)
    try:
        with open(src, encoding="utf-8", errors="replace") as handle:
            text = handle.read()
    except OSError:
        return []
    out = []
    for ref in _SCRIPT_SRC_RE.findall(text) + _META_SCRIPT_RE.findall(text):
        ref = ref.split("#")[0].split("?")[0]
        if not ref or ref in _NOT_A_HELPER or not ref.endswith(".js"):
            continue
        if ref.startswith("/"):
            path = os.path.join(root, ref.lstrip("/"))
        elif "://" in ref:
            continue
        else:
            path = os.path.normpath(os.path.join(os.path.dirname(src), ref))
        if os.path.isfile(path):
            out.append(path)
    return out


def classify_source(test_id, root, cache, follow_helpers=True):
    """Second-stage key for a test the output stage could not claim, or None.

    Reads the test's own source plus, unless `follow_helpers` is off, the
    `<script src>` helpers it includes (`helper_paths`) — a marker in a helper
    is a marker of the test, and the pre-slice-10 test-file-only reading made
    the stage a strict lower bound (see `SOURCE_MARKERS`).
    """
    lines = _read_source(source_path(test_id, root), cache)
    if lines is None:
        return None
    lines = list(lines)
    if follow_helpers:
        for path in helper_paths(test_id, root):
            helper_lines = _read_source(path, cache)
            if helper_lines:
                lines += helper_lines
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
    """Yield `(test_id, status, [output lines], browser_pid)` per test in a shard.

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
        yield (name, status,
               [data for _, data in events[left:right] if data.strip()], pid)


def classify(lines):
    """Return the key of the first mechanism claiming these output lines."""
    if not lines:
        return NO_OUTPUT
    for mech in MECHANISMS:
        if mech.matches(lines):
            return mech.key
    return UNCLASSIFIED


def audit(out_dir, category=None, root=WPT_ROOT, use_source=True,
          follow_helpers=True):
    """Classify every TIMEOUT in a run directory.

    `category` filters by manifest-id prefix (`html`, `html/canvas`, ...);
    `use_source` enables the second, source-marker stage (`SOURCE_MARKERS`);
    `follow_helpers` lets that stage read the test's `<script src>` helpers as
    well as the test file itself (`helper_paths`).
    """
    source_cache = {}
    counts = collections.Counter()
    by_cat = collections.defaultdict(collections.Counter)
    residual_sigs = collections.Counter()
    examples = collections.defaultdict(list)
    residual_examples = collections.defaultdict(list)
    residual_ids = []
    totals = collections.Counter()
    hangs = {}

    for path in sorted(glob.glob(os.path.join(out_dir, "*.raw.jsonl"))):
        shard = os.path.basename(path)[: -len(".raw.jsonl")]
        # Per browser process: the last test it printed anything about (the
        # culprit candidate) and whether it is currently inside a run of
        # TIMEOUTs. A test that finishes with any other status proves the
        # process is still answering, so the run is broken there.
        culprit = {}
        wedged = set()
        for test, status, lines, pid in read_shard(path):
            if not test:
                continue
            if category and not test.strip("/").startswith(category.strip("/")):
                continue
            totals[status] += 1
            if status != "TIMEOUT":
                wedged.discard(pid)
                if lines:
                    culprit[pid] = test
                continue
            if not lines and pid in wedged:
                key = HUNG_BROWSER
                hang = hangs.setdefault((shard, pid), {
                    "shard": shard, "pid": pid,
                    "culprit": culprit.get(pid), "collateral": 0,
                    "first_victim": test, "last_victim": test,
                })
                hang["collateral"] += 1
                hang["last_victim"] = test
                counts[key] += 1
                by_cat[category_of(test)][key] += 1
                if len(examples[key]) < 200:
                    examples[key].append(test)
                wedged.add(pid)
                continue
            wedged.add(pid)
            if lines:
                culprit[pid] = test
            key = classify(lines)
            if use_source and key in (NO_OUTPUT, UNCLASSIFIED):
                from_source = classify_source(test, root, source_cache,
                                              follow_helpers=follow_helpers)
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
        "hung_browsers": sorted(hangs.values(),
                                key=lambda h: -h["collateral"]),
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
    refs[HUNG_BROWSER] = "collateral — see the culprit list below"
    print(f"{'mechanism':28} {'tests':>7} {'share':>7}  owner")
    for key, count in sorted(result["mechanisms"].items(), key=lambda kv: -kv[1]):
        share = 100.0 * count / total if total else 0.0
        print(f"{key:28} {count:7d} {share:6.1f}%  {refs.get(key, '—')}")
        for test in result["examples"].get(key, [])[:examples]:
            print(f"{'':28} {'':7}          {test}")
    hangs = result.get("hung_browsers") or []
    if hangs:
        collateral = sum(h["collateral"] for h in hangs)
        print()
        print(f"hung browsers: {len(hangs)} process(es), {collateral} collateral "
              f"TIMEOUT(s) — each culprit is one page to reproduce")
        for hang in hangs[:top]:
            print(f"  {hang['collateral']:6d}  {hang['culprit'] or '(no output on this pid)'}")
            print(f"{'':10}shard {hang['shard']} pid {hang['pid']}, "
                  f"first victim {hang['first_victim']}")
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

    # 4. no output at all while running, on a browser that already timed out
    #    (test 2, same pid 501) — collateral of a wedged process, not a defect
    #    of its own.
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

    # 7. silent, but the FIRST TIMEOUT of a fresh browser: nothing wedged it,
    #    so it keeps its own `no-output` bucket. The pair 4/7 is the whole
    #    difference the collateral rule makes.
    start("TestRunnerManager-2", 1100, "/c/first-silent.html")
    end("TestRunnerManager-2", 1200, "/c/first-silent.html", "TIMEOUT", 502)

    # 8. silent TIMEOUT on pid 500, which timed out earlier (test 3) but then
    #    finished test 5 with a real verdict — proof the process was still
    #    answering, so the run of TIMEOUTs is broken and this is not collateral.
    start("TestRunnerManager-0", 1300, "/a/silent-after-ok.html")
    end("TestRunnerManager-0", 1400, "/a/silent-after-ok.html", "TIMEOUT", 500)

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
        check(result["timeouts"] == 7,
              f"expected 7 TIMEOUTs (the OK test excluded), got {result['timeouts']}")
        check(result["statuses"].get("OK") == 1, "the OK test must still be counted")
        check(mech.get("https-body-truncated") == 1,
              f"TLS truncation must win over the missing global: {mech}")
        check(mech.get("helper-global-missing") is None,
              f"the downstream symptom must not get its own row: {mech}")
        check(mech.get("worker-importscripts") == 1,
              f"worker importScripts not attributed: {mech}")
        check(mech.get("foreign-host-unresolvable") == 1,
              f"unresolvable host not attributed: {mech}")
        check(mech.get(HUNG_BROWSER) == 1,
              f"silent test after a TIMEOUT on the same pid is collateral: {mech}")
        check(mech.get(NO_OUTPUT) == 2,
              f"a silent timeout on a browser never proven wedged must stay its "
              f"own finding: {mech}")
        hangs = result["hung_browsers"]
        check(len(hangs) == 1 and hangs[0]["collateral"] == 1,
              f"one wedged process with one victim expected: {hangs}")
        hang = hangs[0] if hangs else {}
        check(hang.get("culprit") == "/b/worker.worker.html",
              f"culprit must be the last test pid 501 still printed for: {hangs}")
        check(hang.get("first_victim") == "/b/silent.html",
              f"victim not recorded: {hangs}")
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

        # Stage 2, helper following: a stub test whose whole body is a call
        # into a helper. The marker lives in the helper, so reading the test
        # file alone finds nothing — that was the pre-slice-10 blind spot.
        with open(os.path.join(tmp, "a", "stub.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testharness.js"></script>\n'
                         '<script src="helper.js"></script>\n'
                         '<script>runTest();</script>')
        with open(os.path.join(tmp, "a", "helper.js"), "w", encoding="utf-8") as handle:
            handle.write("const iframe = document.createElement('iframe');\n"
                         "iframe.contentWindow.postMessage('x');")
        check(classify_source("/a/stub.html", tmp, {}) == "iframe-no-nested-context",
              "marker inside an included helper was not found")
        check(classify_source("/a/stub.html", tmp, {}, follow_helpers=False) is None,
              "--no-helpers must restore the test-file-only reading")
        # The harness itself is never followed: it matches several markers and
        # would claim every test in the corpus for whichever sorts first.
        os.makedirs(os.path.join(tmp, "resources"), exist_ok=True)
        with open(os.path.join(tmp, "resources", "testharness.js"), "w", encoding="utf-8") as handle:
            handle.write("document.fonts.ready.then(() => {});")
        check(classify_source("/a/stub.html", tmp, {}) == "iframe-no-nested-context",
              "testharness.js was followed as if it were a test helper")

        # Stage 2, BUG-804 (slice 12). An event-handler attribute on a
        # parser-written resource element is the marker on its own — that
        # element is by construction one the resource machinery never sees.
        with open(os.path.join(tmp, "a", "style-attr.html"), "w", encoding="utf-8") as handle:
            handle.write('<style onload="t.done()">#a{color:red}</style>')
        check(classify_source("/a/style-attr.html", tmp, {}) == "resource-no-load-event",
              "an onload attribute on a parser-written <style> was not claimed")
        # A created `<style>` counts only when the wait is on that same
        # variable. `resources/check-layout-th.js` mints one to highlight a
        # failure and never waits on it; matching that shape would have handed
        # this mechanism 40 unrelated `css-grid` tests.
        with open(os.path.join(tmp, "a", "style-var.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const st = document.createElement('style');\n"
                         "st.onload = () => t.done();</script>")
        check(classify_source("/a/style-var.html", tmp, {}) == "resource-no-load-event",
              "createElement('style') with a wait on the same var was not claimed")
        with open(os.path.join(tmp, "a", "style-nowait.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const st = document.createElement('style');\n"
                         "document.body.appendChild(st);\n"
                         "other.addEventListener('load', () => {});</script>")
        check(classify_source("/a/style-nowait.html", tmp, {}) is None,
              "a created <style> nobody waits on must not be claimed")
        # `createElement('script'|'link')` does fire `load` (BUG-571/BUG-722),
        # verified by the slice-12 A/B page — the marker must not take it.
        with open(os.path.join(tmp, "a", "script-var.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const s = document.createElement('script');\n"
                         "s.onload = () => t.done();</script>")
        check(classify_source("/a/script-var.html", tmp, {}) is None,
              "the working createElement script path must not be blamed on BUG-804")

        # Stage 2, slice 13. A generated test declares its helpers as
        # `// META: script=`, never as markup, so the `<script src>` reader saw
        # a helper-less file and every such id fell into the residual.
        with open(os.path.join(tmp, "a", "meta.window.js"), "w", encoding="utf-8") as handle:
            handle.write("// META: script=/a/meta-helper.js\n"
                         "// META: script=/resources/testdriver.js\n"
                         "promise_test(async () => { await setupTest(); });")
        with open(os.path.join(tmp, "a", "meta-helper.js"), "w", encoding="utf-8") as handle:
            handle.write("const f = document.createElement('iframe');\n"
                         "f.contentWindow.postMessage('x');")
        check(classify_source("/a/meta.window.html", tmp, {}) == "iframe-no-nested-context",
              "a helper declared with // META: script= was not followed")
        check(classify_source("/a/meta.window.html", tmp, {}, follow_helpers=False) is None,
              "--no-helpers must also switch off META-declared helpers")
        # `<track>` built in script: the shape of `track-helpers.js`, which
        # every `track-webvtt-*.html` calls. The literal-tag-only marker missed
        # all 21 of them.
        with open(os.path.join(tmp, "a", "track.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const tr = document.createElement('track');\n"
                         "tr.onload = () => t.done();</script>")
        check(classify_source("/a/track.html", tmp, {}) == "track-element",
              "createElement('track') with a load wait was not claimed")
        # `<embed>`/`<object>`: no loading path at all (BUG-798). The wait is
        # required — the tag alone appears in prose and in fallback markup.
        with open(os.path.join(tmp, "a", "object.html"), "w", encoding="utf-8") as handle:
            handle.write('<object data="x.svg"></object>\n'
                         "<script>o.onerror = () => t.done();</script>")
        check(classify_source("/a/object.html", tmp, {}) == "embed-object-no-load",
              "an <object> with an error wait was not claimed")
        with open(os.path.join(tmp, "a", "object-plain.html"), "w", encoding="utf-8") as handle:
            handle.write('<object data="x.svg">fallback</object>')
        check(classify_source("/a/object-plain.html", tmp, {}) is None,
              "an <object> nobody waits on must not be claimed")
        # A sandboxed frame cannot be reached through `contentWindow`, so the
        # parent waits for its message instead — the shape of every
        # `iframe_sandbox_*` test.
        with open(os.path.join(tmp, "a", "frame-msg.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>onmessage = t.step_func_done(e => {});</script>\n"
                         '<iframe sandbox="allow-scripts" src="child.html"></iframe>')
        check(classify_source("/a/frame-msg.html", tmp, {}) == "iframe-no-nested-context",
              "an <iframe> whose result arrives by postMessage was not claimed")
        # BUG-799: an audio `src` freezes the page, so it outranks whatever
        # else the test also does — here an `<iframe>` it never reaches.
        with open(os.path.join(tmp, "a", "audio-src.html"), "w", encoding="utf-8") as handle:
            handle.write('<audio src="/media/sine440.mp3"></audio>\n'
                         "<iframe src=x.html></iframe>\n"
                         "<script>onmessage = () => t.done();</script>")
        check(classify_source("/a/audio-src.html", tmp, {}) == "audio-src-deadlock",
              "an <audio src> must outrank every later wait in the same page")
        with open(os.path.join(tmp, "a", "audio-var.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const au = document.createElement('audio');\n"
                         "au.src = '/media/sine440.mp3';</script>")
        check(classify_source("/a/audio-var.html", tmp, {}) == "audio-src-deadlock",
              "a src assignment on a created <audio> was not claimed")
        # A bare `new Audio()` never touches the deadlocking path.
        with open(os.path.join(tmp, "a", "audio-bare.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const au = new Audio();\n"
                         "assert_equals(au.volume, 1);</script>")
        check(classify_source("/a/audio-bare.html", tmp, {}) is None,
              "a srcless <audio> is healthy and must not be claimed")

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
    parser.add_argument("--no-helpers", action="store_true",
                        help="in the source-marker stage read only the test "
                             "file, not the <script src> helpers it includes "
                             "(the pre-WPT-RUN-6-slice-10 behaviour)")
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
                   use_source=not args.no_source,
                   follow_helpers=not args.no_helpers)
    print_report(result, top=args.top, examples=args.examples)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, ensure_ascii=False, indent=1)
        print(f"\nwrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
