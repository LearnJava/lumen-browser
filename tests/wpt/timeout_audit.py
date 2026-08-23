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

**Five stages, in order of how much the evidence says.** `MECHANISMS` reads
what the browser printed while the test ran and matches the error text that
names a cause. What survives that goes to `MEASURED_HANGS` — the ids that were
*run standalone* under `--dump-layout` and did not finish (`verify_layout_hangs.py`,
WPT-RUN-6 slice 23); executing the page is stronger evidence than reading it,
and for these mechanisms no regex over the source could decide the question
anyway (whether a grid item reaches past the last explicit line depends on the
resolved track count, which `repeat(auto-fill, ...)` makes a function of the
container width). Third is `SUBTEST_MARKERS` (WPT-RUN-6 slice 24), which reads
the run's *own* partial harness report — the subtests `wptrunner` collected
before the timeout — and attributes the file to the mechanism claiming the most
of its hung (`TIMEOUT`/`NOTRUN`) subtests. That is evidence the run produced,
and it reaches what a grep cannot: `query-encoding/resources/resolve-url.js`
builds `<frame>`, `<iframe>`, `<object>` and `<embed>` in one loop over
`createElement(tag)`, so no source pattern names the element that hung, while
the subtest name (`load nested browsing context <frame src>`) does. Then
`SOURCE_MARKERS`, which greps the test (and its helpers)
for a wait that cannot finish — the silent mechanisms, which by construction
print nothing. Only then does `WEAK_MECHANISMS` claim the rest of the *noisy*
ones: a page that threw an exception the engine printed but no listener ever
saw. That last stage sorts below the source markers on purpose — "something
threw" is true of a `document.fonts.ready` page too, and reporting it that way
would bury a mechanism that has a name (WPT-RUN-6 slice 14).

**The residual keeps its subtest evidence.** For every id still unexplained,
`residual_hung_subtests` in `--json` carries the names of the subtests that
never finished — the work list the next slice picks its target from, and the
only part of the record that says *where inside the file* the run stopped.

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
        [--no-measured] [--no-subtests] [--no-source]
    <venv>/python tests/wpt/timeout_audit.py --selftest

Safe to run against a live run's `--out-dir`: it only reads. A shard still
being written is read as far as it got (a truncated last line is skipped).
"""

import argparse
import ast
import bisect
import collections
import glob
import importlib.util
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
        #: Optional callable `(joined source text, test id)` used instead of
        #: `patterns` when a line-at-a-time regex cannot express the marker.
        #: The id is what tells an XML document from an HTML one (the file
        #: extension is what `wptserve` derives the content type from), and is
        #: `None` when the mechanism is matched against browser output.
        #: BUG-804 needs it: `createElement('style')` alone is not evidence —
        #: `resources/check-layout-th.js` mints one to highlight failures and
        #: never waits on it — so the wait has to be tied to the *same*
        #: variable, which is a two-step match across lines.
        self.predicate = predicate

    def matches(self, lines, test_id=None):
        if self.predicate is not None:
            return self.predicate("\n".join(lines), test_id)
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
        # The second text is `testdriver-extra.js::get_context` (line 118)
        # refusing to act because `element.ownerDocument.defaultView` is
        # falsy — the same missing property, thrown from wptrunner's own
        # injected code rather than from a test helper (WPT-RUN-6 slice 14).
        "defaultview-test-driver", "BUG-622",
        [r"Cannot read properties of undefined \(reading 'test_driver'\)",
         r"Browsing context for element was detached"],
        "`document.defaultView` is absent, so WPT helpers that reach through "
        "it (editing/editor-test-utils.js, testdriver-extra.js) throw before "
        "any test() runs",
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
        # Network layer, so it sorts with the other network causes: the request
        # was never made at all. `XMLHttpRequest.prototype.open` stores the URL
        # verbatim (`crates/js/src/xhr.rs:216`) and `send()` hands that string
        # straight to `_lumen_fetch_sync*`, so `open('GET', 'resources/x.py')`
        # reaches `lumen-network` unresolved. `fetch()` does not have the bug
        # (it resolves against `_lumen_document_base_url()`, BUG-347) — the
        # line therefore names XHR specifically (WPT-RUN-6 slice 18).
        # Filed as BUG-812, merged into BUG-780 as a duplicate on 2026-08-22
        # (same line, same fix; BUG-780 is the earlier of the two) — attribute
        # to the surviving id so the count lands on a live bug.
        "relative-url-unresolved", "BUG-780",
        [r"invalid url: missing scheme"],
        "a relative URL passed to XMLHttpRequest.open() was never resolved "
        "against the document base, so the request failed before leaving the "
        "engine and the test's load/readystatechange never arrives",
    ),
    Mechanism(
        "websocket", "BUG-799",
        [r"\[JS WebSocket\].*error"],
        "the page's WebSocket never connected",
    ),
    # --- The engine named the cause itself (WPT-RUN-6 slice 14) -----------
    # A page-script exception is printed by the shell (`script error:` /
    # `module error:`) and then goes nowhere: BUG-591 means the `error` event
    # is never dispatched on the window, so `testharness.js`'s own
    # `error_handler` — the code that would set the harness status to ERROR and
    # call `done()` — never runs, and a file that should have failed in a
    # second dies on the external timeout instead. The text of the line names
    # the API that is missing, so each of these is an ordinary evidence
    # mechanism owned by that API's bug; BUG-591 owns only the shared last
    # step, the degradation of the verdict from FAIL/ERROR to TIMEOUT.
    Mechanism(
        "inline-style-assign", "BUG-494 (+BUG-591)",
        [r"Cannot set property style of .* which has only a getter"],
        "`el.style = \"...\"` has no `[PutForwards=cssText]` setter: a no-op "
        "in sloppy mode, a TypeError in a strict-mode helper, which kills the "
        "script that called it",
    ),
    Mechanism(
        "cssom-stylesheets-missing", "BUG-471 (+BUG-591)",
        [r"reading 'cssRules'", r"reading 'rules'",
         r"styleSheets is not iterable",
         r"(?:insertRule|deleteRule) is not a function",
         r"CSSStyleSheet is not defined"],
        "the CSSOM stylesheet model is not wired to the shim at all — "
        "`document.styleSheets` is `undefined`, `<style>.sheet` absent",
    ),
    Mechanism(
        "typed-om-incomplete", "BUG-554 (+BUG-591)",
        [r"CSS\.[A-Za-z]+ is not a function",
         r"CSS(?:Math[A-Za-z]*|VariableReferenceValue|PositionValue|"
         r"TransformValue|ImageValue|UnparsedValue) is not defined"],
        "CSS Typed OM has only the value/unit/keyword core — the numeric "
        "factories (`CSS.px`, `CSS.deg`) and the math/transform classes are "
        "absent",
    ),
    Mechanism(
        "getclientrects-missing", "BUG-580 (+BUG-591)",
        [r"getClientRects is not a function"],
        "`Element.prototype.getClientRects` does not exist (only "
        "`getBoundingClientRect` does)",
    ),
    Mechanism(
        "font-loading-api", "BUG-467 (+BUG-591)",
        [r"document\.fonts\.[A-Za-z]+ is not a function",
         r"FontFace(?:Set|SetLoadEvent)? is not defined"],
        "CSS Font Loading is a stub: `document.fonts` has no `load`/`check`, "
        "and the `FontFace` constructors are missing",
    ),
    Mechanism(
        "elementfrompoint-missing", "BUG-464 (+BUG-591)",
        [r"elements?FromPoint is not a function"],
        "there is no point→node hit test on the JS side at all",
    ),
    Mechanism(
        # Not an engine gap in the API sense: the test ran and its assertion
        # said the engine is wrong. It is here because the verdict is wrong
        # too — a FAIL that BUG-591 turned into a TIMEOUT, so the run's own
        # numbers understate the failures and overstate the hangs.
        "assert-swallowed", "BUG-591",
        [r"(?:script|module) error:.*\bassert_[a-z_]+:"],
        "an assertion threw outside a `test()` body: a real FAIL, reported as "
        "a TIMEOUT because the window `error` event never reaches the harness",
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


#: `new LoadObserver(el)` / `test_render_blocking(el, ...)` — the two entry
#: points of `html/dom/render-blocking/support/test-render-blocking.js`, which
#: wraps `target.addEventListener('load', ...)` and hands the resulting promise
#: to a `promise_test`. The indirection is why the plain `_waits_on_var` shapes
#: miss the family: the listener is attached inside the helper, on a parameter,
#: so nothing in the test file itself mentions `load` at all.
_LOAD_OBSERVER_RE = re.compile(
    r"new LoadObserver\s*\(|\btest_render_blocking\s*\(")

#: A `<script src>` / `<link rel=stylesheet>` / `<style>` written by the parser
#: — the elements the shell loads on its own path without telling JS. Used only
#: together with `_LOAD_OBSERVER_RE`: on its own it is most of the corpus.
_PARSER_RESOURCE_TAG_RE = re.compile(
    r"<script\b[^>]*\ssrc\s*=|<link\b[^>]*\srel\s*=\s*['\"]?stylesheet|<style\b",
    re.IGNORECASE)

#: `createElement(tag)` where the tag can be `'style'` — either spelled out or
#: passed in as a string argument next to it, which is how
#: `remove-attr-unblocks-rendering.optional.html` mints its four elements
#: (`addRenderBlockingElement('style', ...)` → `document.createElement(tag)`).
_STYLE_ELEMENT_HINT_RE = re.compile(
    r"createElement\(\s*['\"]style['\"]\s*\)|['\"]style['\"]\s*,", re.IGNORECASE)


def _resource_event_marker(text, test_id=None):
    """BUG-804 marker: the test waits for a `load`/`error` the engine never fires.

    Three shapes, all verified live. The attribute shape is parser-inserted by
    construction (WPT-RUN-6 slice 12). The `createElement('style')` shape is the
    one case where even the working `createElement` path stays silent, because
    `_lumen_resource_track` whitelists `script`/`link` only — but a created
    `<style>` is only evidence when something actually waits on *it*, hence
    `_waits_on_var`.

    The third shape is `LoadObserver` (WPT-RUN-6 slice 20). The whole
    `html/dom/render-blocking` family waits for an element `load` through that
    helper rather than in its own text, and slice 20 re-measured both halves it
    needs: a parser-inserted `<script src>` runs but dispatches `load` neither
    to `addEventListener` nor to `onload` (the resource is held for a second by
    the probe server, so the listener is certainly attached first), the same
    for a parser-inserted `<link rel=stylesheet>`, and a `<style>` fires
    nothing however it was inserted — while the script-created `<script>`/
    `<link>` controls on the same page do fire. So the helper plus one silent
    element under observation is the marker; `test_render_blocking(window)`
    with no such element would not match either half.

    Deliberately NOT matched: `createElement('script'|'link')`. That path fires
    `load` correctly (BUG-571/BUG-722), confirmed by the same A/B page.
    """
    if _RESOURCE_EVENT_ATTR_RE.search(text):
        return True
    if _LOAD_OBSERVER_RE.search(text) and (
            _PARSER_RESOURCE_TAG_RE.search(text)
            or _STYLE_ELEMENT_HINT_RE.search(text)):
        return True
    return any(_waits_on_var(text, var) for var in _CREATE_STYLE_RE.findall(text))


#: `x = new Audio()` / `x = document.createElement('audio')`, capturing the
#: variable so the `src` assignment can be required on that same one.
_CREATE_AUDIO_RE = re.compile(
    r"(?:var|let|const)?\s*(\w+)\s*=\s*(?:new Audio\(\s*\)|"
    r"document\.createElement\(['\"]audio['\"]\))", re.IGNORECASE)


def _audio_src_marker(text, test_id=None):
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


#: Extensions `wptserve` hands out as a real XML content type
#: (`application/xhtml+xml`, `image/svg+xml`, `text/xml`). Everything else is
#: parsed as HTML by a conforming browser too, so the same markup in a `.html`
#: file is not evidence of anything.
_XML_EXTENSIONS = (".xhtml", ".xht", ".svg", ".xml")

#: `<h:script src="...">` — a namespace-prefixed element. Only an XML parser
#: resolves the prefix; the HTML tree builder takes `h:script` for an unknown
#: element name, so the script is never a script.
_PREFIXED_SCRIPT_RE = re.compile(r"<\w+:script\b", re.IGNORECASE)

#: `<script src="..."/>` — self-closing, which XML honours and HTML does not:
#: everything up to the next `</script>` becomes this element's text, so the
#: rest of the document (including the remaining `<script src>` tags) is
#: swallowed.
_SELFCLOSED_SCRIPT_RE = re.compile(r"<script\b[^>]*/\s*>", re.IGNORECASE)

#: `<script><![CDATA[ ... ]]></script>` — in HTML the marker is not markup but
#: the first characters of the script text, i.e. a syntax error.
_CDATA_SCRIPT_RE = re.compile(r"<script\b[^>]*>\s*(?://[^\n]*\n\s*)?<!\[CDATA\[",
                              re.IGNORECASE)


def _xml_document_script_marker(text, test_id=None):
    """BUG-786 marker: an XML document whose scripts HTML parsing loses.

    Navigation has no XML path at all — `crates/shell/src/main.rs:5365` runs
    `lumen_html_parser::parse` on every response and only stamps
    `document.contentType` from the header afterwards. BUG-786 measured the
    `<style><![CDATA[` half of the damage; the script half is what makes the
    test TIMEOUT rather than merely render wrong, and it comes in three shapes,
    each measured separately by `tests/wpt/verify_document_and_record_gaps.py`
    (WPT-RUN-6 slice 16):

    * prefixed `<h:script src>` — never becomes a script element, so the file's
      `testharness.js` is not even *requested* (the corpus log of every
      `dom/nodes/Element-*-svg.svg` shows the document parsed and painted with
      no GET for the harness);
    * self-closing `<script src="..."/>` — the first one swallows the rest of
      the document as its own text (`css/cssom/MediaList2.xhtml` loads
      `testharness.js` and then neither `testharnessreport.js` nor the test
      body exists);
    * `<![CDATA[` at the head of an inline script — `script error: JS runtime
      error: Unexpected token '<'`, which BUG-591 then swallows.

    The same markup inside a `.html` file is not evidence: there a conforming
    browser parses as HTML too, so the test would not be written this way.
    Hence the extension check — it is what `wptserve` derives the content type
    from — and hence `None` (an output-stage call) never matches.
    """
    if not test_id:
        return False
    if not test_id.split("?")[0].lower().endswith(_XML_EXTENSIONS):
        return False
    return bool(_PREFIXED_SCRIPT_RE.search(text)
                or _SELFCLOSED_SCRIPT_RE.search(text)
                or _CDATA_SCRIPT_RE.search(text))


#: A window-level `error` / `unhandledrejection` wait: `window.onerror = fn`,
#: `window.addEventListener('error', ...)`, or either rejection event. Element
#: handlers (`script.onerror`, `img.onerror`) are deliberately not here — those
#: are BUG-630/BUG-804 and are claimed by markers above.
_WINDOW_ERROR_WAIT_RE = re.compile(
    r"window\.onerror\s*=|window\.onunhandledrejection\s*="
    r"|(?:window|self)\.addEventListener\(\s*['\"](?:error|unhandledrejection"
    r"|rejectionhandled)['\"]"
    r"|\bunhandledrejection\b|\brejectionhandled\b")

#: `window.onerror = t.unreached_func(...)` — the same syntax used as an
#: *assertion* rather than as the test's completion condition.
_UNREACHED_RE = re.compile(r"unreached_func|assert_unreached")


def _window_error_wait_marker(text, test_id=None):
    """True iff the test's completion depends on a window-level error event.

    BUG-716/BUG-591: neither a top-level `throw` nor a rejected promise reaches
    a `window` listener — measured again in slice 19
    (`verify_stream_scroll_message_gaps.py --variant window-error-events`: all
    four registrations, both handler shapes, print nothing at all). A test
    whose `done()` hangs off one of those events can therefore only TIMEOUT.

    The `unreached_func` guard is why this is a predicate rather than two
    patterns: `html/semantics/scripting-1/the-script-element/fetch-src/
    empty.html` sets `window.onerror = this.unreached_func(...)` as an
    *assertion* and finishes from `script.onerror`, so matching the line would
    attribute it to the one mechanism it is explicitly asserting against.
    """
    for line in text if isinstance(text, list) else text.splitlines():
        if not _WINDOW_ERROR_WAIT_RE.search(line):
            continue
        if _UNREACHED_RE.search(line):
            continue
        return True
    return False


#: `setTimeout("code", 0)` / `setInterval(`code`, 0)` — the string-handler
#: form of the timer API.
_STRING_TIMER_RE = re.compile(r"""set(?:Timeout|Interval)\s*\(\s*(?:`|"|')""")

#: The `string-compilation-*` family's evaluator table: a map/list of the ways
#: a string can be compiled, `setTimeout` among them, applied to one and the
#: same import expression. The string never appears next to the call, so
#: `_STRING_TIMER_RE` cannot see it.
_EVALUATOR_TABLE_RE = re.compile(r"\bevaluators\b")


def _timer_string_handler_marker(text, test_id=None):
    """True iff the test hands a *string* to `setTimeout`/`setInterval`.

    BUG-831: `setTimeout` and `setInterval` (`crates/js/src/dom.rs:6809`,
    `:6827`) open with `if (typeof fn !== 'function') return 0;` — a string
    handler is dropped on the floor and a plausible timer id is returned, so
    nothing throws and nothing runs. Measured live in slice 21
    (`verify_navigation_form_import_gaps.py --variant settimeout-string`: the
    function-handler control fires, neither string handler ever does).

    Two shapes, because the family that hangs on this hardest never writes the
    string next to the call: `string-compilation-*` builds a table of
    evaluators (`eval`, `setTimeout`, `the Function constructor`, ...) and
    feeds each the same `import()` source, so the evidence is the table plus
    the name. `promise_test`s run sequentially, which is why one dead
    evaluator takes the whole file with it.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    if _STRING_TIMER_RE.search(body):
        return True
    return bool(_EVALUATOR_TABLE_RE.search(body) and "setTimeout" in body)


#: `location.hash = "x"` — the assignment that performs the same-document
#: fragment navigation.
_HASH_ASSIGN_RE = re.compile(r"location\.hash\s*=")

#: `addEventListener("hashchange", ...)` — the only registration form the
#: affected tests use (`window.onhashchange = ...` is assigned, not added, and
#: would be in place before the assignment anyway).
_HASHCHANGE_LISTEN_RE = re.compile(r"""addEventListener\(\s*["']hashchange["']""")


def _hashchange_after_assignment_marker(text, test_id=None):
    """True iff the test attaches its `hashchange` listener after the assignment.

    BUG-832: `_lumen_set_location_hash` (`crates/js/src/dom.rs:6323`) calls
    `_lumen_fire_hashchange` inline, so the event is delivered *during* the
    assignment instead of from a queued task. Measured live in slice 21 as a
    pair: `--variant nav-hashchange` (listener first) sees the event,
    `--variant nav-hashchange-late` (listener attached on the next line) hears
    nothing at all, on an otherwise identical page.

    Order is the whole marker, which is why this is a predicate: the same two
    lines in the other order are a *working* page, and matching them would
    claim every fragment test in the corpus.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    assign = _HASH_ASSIGN_RE.search(body)
    listen = _HASHCHANGE_LISTEN_RE.search(body)
    return bool(assign and listen and listen.start() > assign.start())


#: `postMessage(msg, {targetOrigin: ...})` — the `WindowPostMessageOptions`
#: overload.
_PM_OPTIONS_RE = re.compile(
    r"postMessage\s*\([^;{]*?,\s*\{\s*(?:targetOrigin|transfer)\b", re.S)

#: `postMessage("msg")` — the one-argument form, valid since the same spec
#: change and equally unsupported. A bare call or an explicitly window-targeted
#: one only: `port.postMessage(x)` / `worker.postMessage(x)` take exactly one
#: argument by spec and are a different (working) API.
_PM_ONE_ARG_RE = re.compile(
    r"""(?<![.\w])(?:(?:window|parent|top)\.)?postMessage\s*\(\s*['"`][^'"`]*['"`]\s*\)""")

#: A `postMessage` that crosses into a worker is a different mechanism (the
#: worker stage owns it), so a file that starts one is left alone.
_PM_WORKER_RE = re.compile(r"new\s+(?:Shared|Service)?Worker\s*\(|\.worker\.|worker\.postMessage")

#: The wait: without it, a `postMessage` in the file is not what the test hangs
#: on (`broken-origin.html` asserts a synchronous throw and is a FAIL, not a
#: hang).
_PM_WAIT_RE = re.compile(r"""onmessage\s*=|addEventListener\(\s*["']message["']""")


def _postmessage_options_marker(text, test_id=None):
    """True iff the test waits for a message it posted through the modern overload.

    BUG-717: `window.postMessage` implements only the legacy string
    `targetOrigin`; the one-argument form and the `WindowPostMessageOptions`
    dictionary drop the message with no error, and `transfer` never produces
    `e.ports`. Measured live in slice 19 (`verify_stream_scroll_message_gaps.py`
    `--variant postmessage-options`), which is why slice 21 adds the marker
    without a new measurement — the mechanism was already established, only
    its ids were never attributed.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    if _PM_WORKER_RE.search(body):
        return False
    if not _PM_WAIT_RE.search(body):
        return False
    return bool(_PM_OPTIONS_RE.search(body) or _PM_ONE_ARG_RE.search(body))


#: Tree `classify_source` is currently reading, so `_own_source` re-reads the
#: test from the same place (the selftest runs against a temporary tree, and
#: reading the real WPT checkout there would silently fall back to the joined
#: text). Set on every `classify_source` call; single-threaded by construction.
_SOURCE_ROOT = None


def _own_source(text, test_id):
    """The test file's own source, without the helpers `classify_source` joins in.

    Needed by the IndexedDB rules and by nothing else so far: WPT's
    `IndexedDB/resources/support.js` *defines* `keep_alive` — and, inside it, a
    self-rearming `spin()` — so matching the joined text claims every test that
    merely includes the helper (16 ids instead of 5, measured while writing
    slice 22). Falls back to the joined text when the file cannot be read, so
    the rule degrades to its old, wider self rather than to silence.
    """
    joined = "\n".join(text) if isinstance(text, list) else text
    if not test_id:
        return joined
    try:
        own = _decode_source(source_path(test_id, _SOURCE_ROOT or WPT_ROOT))
    except OSError:
        return joined
    return own if own else joined


#: WPT's own `IndexedDB/resources/support.js` helpers: both hold a transaction
#: open by re-arming a request from that request's own `onsuccess`.
_IDB_KEEP_ALIVE_RE = re.compile(r"\bkeep_alive\s*\(|\bis_transaction_active\s*\(")

#: A function that issues an IndexedDB request and calls *itself* from the
#: handler — the same spin written out by hand
#: (`transaction-scheduling-*.any.js::doTransaction1Get`).
_IDB_FN_RE = re.compile(r"function\s+(\w+)\s*\(")

#: Cheap gate so the two rules above cannot claim a non-IndexedDB test that
#: happens to define a recursive function.
_IDB_TEST_RE = re.compile(r"\bindexedDB\b|\bindexeddb_test\s*\(|\bcreatedb\s*\(")


def _idb_spin_marker(text, test_id=None):
    """True iff the test holds a transaction open with a self-rearming request.

    BUG-842: `_idb_flush_txn` (`crates/js/src/dom.rs:12474`) drains the
    transaction's queue in a `while` loop inside one microtask, and a handler
    that enqueues the next request refills that queue from inside the loop —
    so the spin never yields. Measured in slice 22: 16.3M iterations in 6 s,
    with no timer, no rendering and no later script of the document ever
    running (`verify_perf_idb_sse_gaps.py --variant idb-spin-unbounded`), and
    the page dead from the outside (`--variant idb-keep-alive`: zero output).

    Two shapes count. The helper (`keep_alive`/`is_transaction_active`) is the
    one four residual ids use; the hand-written one is a function that both
    issues a request and re-calls itself, which is how the
    `transaction-scheduling-*` pair keeps two transactions alive at once.
    """
    body = _own_source(text, test_id)
    if not _IDB_TEST_RE.search(body):
        return False
    if _IDB_KEEP_ALIVE_RE.search(body):
        return True
    for match in _IDB_FN_RE.finditer(body):
        name = match.group(1)
        window = body[match.end():match.end() + 800]
        if "objectStore(" not in window:
            continue
        # One rule for both ways of writing the loop: the handler *is* the
        # function (`rq.onsuccess = spin;`) or calls it from a wrapper
        # (`request.onsuccess = t.step_func(() => { doGet(); })`). Two
        # separate regexes were tried first and turned out to be equivalent
        # mutants — each matched both shapes.
        if re.search(r"onsuccess\s*=.{0,400}?\b" + re.escape(name) + r"\s*[(;,)]",
                     window, re.S):
            return True
    return False


#: The two events a connection queue produces. `'versionchange'` as a bare
#: string is deliberately NOT here: it is also the *mode* of an upgrade
#: transaction (`assert_equals(tx.mode, 'versionchange')`), which half the
#: IndexedDB suite writes without waiting for anything.
_IDB_QUEUE_RE = re.compile(
    r"\bonblocked\b|\bonversionchange\b|"
    r"addEventListener\s*\(\s*['\"](?:blocked|versionchange)['\"]")


def _idb_connection_queue_marker(text, test_id=None):
    """True iff the test waits for `versionchange` / `blocked`.

    BUG-843: `indexedDB.open` (`crates/js/src/dom.rs:13023`) looks only at the
    stored database record, never at live connections; `onversionchange`
    (`:12509`) and `onblocked` (`:12351`) are declared and dispatched by
    nobody. Measured in slice 22 (`--variant idb-versionchange-blocked`: the
    second `open()` upgrades at once, neither event arrives).
    """
    body = _own_source(text, test_id)
    if not _IDB_TEST_RE.search(body):
        return False
    return bool(_IDB_QUEUE_RE.search(body))


#: `const tx = db.transaction(...)` — the transaction's own variable name is
#: what the rest of the rule is written against.
_IDB_TXN_ASSIGN_RE = re.compile(r"(?:var|let|const)?\s*(\w+)\s*=\s*[\w.]*\bdb\w*\.transaction\s*\(")

#: Any request method on an object store — the thing an "empty" transaction
#: does not have.
_IDB_REQUEST_RE = re.compile(
    r"\.(?:put|get|add|delete|count|clear|openCursor|openKeyCursor|getAll|getAllKeys)\s*\(")


def _idb_empty_transaction_marker(text, test_id=None):
    """True iff the test awaits `complete` on a transaction with no requests.

    BUG-841: a transaction reaches `_idb_flush_txn` only through
    `_idb_schedule_txn`, which is called by the *first request inside it*
    (`crates/js/src/dom.rs:12494`); `IDBDatabase.prototype.transaction`
    (`:12548`) queues nothing, so a transaction that never gets a request
    never commits. Measured in slice 22 (`--variant idb-tx-empty`: only
    `idb-empty-armed`, no `complete`), against the control `--variant
    idb-tx-complete`, where a transaction *with* a request completes in the
    right order.

    The rule reads one transaction at a time: a file whose other transactions
    do have requests (`transaction-lifetime-empty.any.js`) still matches on
    the empty one, which is the transaction its last subtest waits for.
    """
    body = _own_source(text, test_id)
    if not _IDB_TEST_RE.search(body):
        return False
    for match in _IDB_TXN_ASSIGN_RE.finditer(body):
        name = match.group(1)
        window = body[match.end():match.end() + 900]
        if not re.search(re.escape(name) + r"\.oncomplete\s*=|"
                         + re.escape(name) + r"\.addEventListener\s*\(\s*['\"]complete",
                         window):
            continue
        # Requests are attributed by *variable*, not by proximity: the tests
        # this rule is for open three transactions in a row and then issue a
        # request on the first one's store, below all three
        # (`transaction-lifetime-empty.any.js`), so "is there a request within
        # N characters" reads that one as belonging to the empty transactions.
        if re.search(re.escape(name) + r"\.objectStore\s*\([^)]*\)\s*" + _IDB_REQUEST_RE.pattern,
                     body):
            continue
        # A store variable's requests are looked for near *its own*
        # assignment, not across the whole file: `store` is reused by a dozen
        # unrelated subtests in `idbobjectstore_createIndex.any.js`, and a
        # whole-file search reads one of those as this transaction's request.
        busy = False
        for store_match in re.finditer(r"(\w+)\s*=\s*" + re.escape(name)
                                       + r"\.objectStore\s*\(", body):
            store = store_match.group(1)
            near = body[store_match.end():store_match.end() + 900]
            if re.search(r"\b" + re.escape(store) + r"\s*" + _IDB_REQUEST_RE.pattern, near):
                busy = True
                break
        if busy:
            continue
        return True
    return False


#: The three ways a test asks for a `resource` PerformanceEntry: through
#: `observe()`, through the buffer, or through the registry's own table.
_RT_OBSERVE_RE = re.compile(
    r"entryTypes\s*:\s*\[[^\]]*['\"]resource['\"]|"
    r"type\s*:\s*['\"]resource['\"]|"
    r"getEntriesByType\s*\(\s*['\"]resource['\"]|"
    r"\[\s*['\"]resource['\"]\s*,\s*['\"]PerformanceResourceTiming['\"]|"
    r"\bdroppedEntriesCount\b")


def _resource_timing_marker(text, test_id=None):
    """True iff the test waits for a `resource` entry (or the callback's options).

    BUG-839: `_lumen_record_resource_timing` (`crates/js/src/dom.rs:11600`)
    has no caller outside the shim's own unit tests, so no `resource` entry is
    ever produced on a live page — while `resource` *is* in
    `supportedEntryTypes` and `observe()` accepts it, which is exactly what
    turns these tests into TIMEOUTs instead of FAILs. The same bug's second
    facet is the missing third callback argument, so a test reading
    `droppedEntriesCount` counts too. Measured in slice 22 (`--variant
    po-resource-fetch`, `--variant po-resource-subresource`,
    `--variant po-callback-options`) against the control `--variant
    po-mark-measure`, where `mark`/`measure` entries are delivered normally.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    if "PerformanceObserver" not in body and "getEntriesByType" not in body:
        return False
    return bool(_RT_OBSERVE_RE.search(body))


#: A `DecompressionStream`/`CompressionStream` read.
_CS_CTOR_RE = re.compile(r"new\s+(?:De)?[Cc]ompressionStream\s*\(")
_CS_READ_RE = re.compile(r"\.read\s*\(\s*\)|getReader\s*\(")
_CS_CLOSE_RE = re.compile(r"\.close\s*\(\s*\)|pipeThrough\s*\(|pipeTo\s*\(")


def _compression_read_marker(text, test_id=None):
    """True iff the test reads a compression stream without closing the writer.

    BUG-846: the shim is a buffer-then-flush model by its own header comment
    (`crates/js/src/dom.rs:8197`) — `transform` only accumulates and the single
    output chunk is enqueued from `flush`, i.e. from `writable.close()`. A test
    that writes a chunk and reads before closing therefore waits forever.
    Measured in slice 22: `--variant decompression-basic` produces nothing,
    while the control `--variant decompression-after-close` — the same page
    plus `writer.close()` — decompresses correctly.

    The `close`/`pipeThrough`/`pipeTo` exclusion is what keeps the rule honest:
    those tests do reach the flush and fail (or pass) for other reasons.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    if not _CS_CTOR_RE.search(body) or not _CS_READ_RE.search(body):
        return False
    return not _CS_CLOSE_RE.search(body)


#: The `fetch/metadata` templates all funnel through one helper name, and the
#: element is what decides whether a request happens at all.
_INDUCE_RE = re.compile(r"function\s+induceRequest\s*\(|\binduceRequest\s*\(")
_SUBRESOURCE_ELEMENT_RE = re.compile(
    r"createElement\s*\(\s*['\"](?:video|audio|link|input|img)['\"]|"
    r"createElementNS\s*\([^)]*['\"](?:image|video|audio)['\"]\s*\)|"
    r"<\s*(?:video|audio|input|image)\b|"
    r"rel\s*=\s*['\"]icon['\"]|\bposter\b|type\s*=\s*['\"]image['\"]")


def _element_subresource_marker(text, test_id=None):
    """True iff the test induces a subresource request through an element.

    BUG-848: `collect_requests_inner`
    (`crates/engine/layout/src/box_tree.rs:2262`) matches `name.local == "img"`
    and nothing else, so a `<video poster>`, an `<input type=image>` and an SVG
    `<image>` never produce a request; `<link rel=icon>` reaches only the hint
    scanner, whose single consumer prints a line to stderr (BUG-826). For
    `<video src>`/`<audio src>` the cause is the missing resource selection
    algorithm (BUG-825/BUG-799) — a different defect with the same observable
    shape, which is why the ref names both.

    Measured in slice 22 with the probe's own request-recording server, so
    "no request was made" does not depend on the page or on the browser log:
    `--variant req-video-poster`, `--variant req-input-image-src`,
    `--variant req-svg-image`, `--variant req-link-icon` — none of the four
    reaches the server, while the `<img>`/`<link rel=stylesheet>` controls on
    the same pages do.
    """
    body = "\n".join(text) if isinstance(text, list) else text
    if not _INDUCE_RE.search(body):
        return False
    return bool(_SUBRESOURCE_ELEMENT_RE.search(body))


def _single_test_load_handler_marker(text, test_id=None):
    """`setup({single_test})` + a body that runs entirely from `load`.

    The shape of `css/css-shapes/spec-examples/*`: the file declares itself a
    single test, does its asserting inside a function called from
    `<body onload>` and calls `done()` at the end. An assertion that fails
    there *throws*, `done()` is never reached, and the exception is swallowed
    by `_lumen_apply_ready_state`'s bare `catch (e) {}` around the window
    `load` listeners and `window.onload` (`dom.rs:13816`/`:13819`) — measured
    2026-08-22, `verify_frame_load_media_gaps.py --variant onload-throw`: no
    `error` event, no `window.onerror`, not even a line on stderr. So the file
    reports NOTRUN and times out where a spec-compliant engine reports FAIL.

    Requires all three markers: `single_test` alone is common, and a page that
    merely has an `onload` attribute usually reports through `async_test`.
    """
    if "single_test" not in text:
        return False
    if not re.search(r"<body[^>]*\bonload\s*=", text, re.I):
        return False
    return "done()" in text


SOURCE_MARKERS = [

    # First on purpose: see `_audio_src_marker` — the page stops dead at the
    # `src` assignment, before anything else it was going to do. It sorts above
    # the XML marker too: the media load is driven by the parser, so it happens
    # whether or not the document's scripts survived parsing.
    Mechanism(
        "audio-src-deadlock", "BUG-799", [],
        "giving an `<audio>` a `src` never returns (deadlock in the audio "
        "provider), so the whole page freezes on the spot",
        predicate=_audio_src_marker,
    ),
    # Second: when the harness itself never arrives, nothing else the file
    # contains can be what the test is waiting for.
    Mechanism(
        "xml-document-scripts-lost", "BUG-786", [],
        "an XML document is parsed by the HTML tree builder, which loses "
        "prefixed / self-closed / CDATA-wrapped scripts — the harness or the "
        "test body never runs",
        predicate=_xml_document_script_marker,
    ),
    # Third: a page frozen by an IndexedDB spin cannot be waiting for anything
    # else either — measured as a dead page, not a slow one (slice 22).
    Mechanism(
        "idb-keep-alive-spin", "BUG-842", [],
        "the test holds an IndexedDB transaction open with a self-rearming "
        "request, and the shim drains that queue in one microtask — the page "
        "spins forever and no other task source ever runs",
        predicate=_idb_spin_marker,
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
        # Measured live in slice 20 (`verify_preload_script_audio_gaps.py`),
        # with the probe's own http server as the witness: a `rel=preload`,
        # `rel=modulepreload` or `rel=prefetch` link — created from script or
        # written by the parser — produces **no request at all**, and fires
        # neither `load` nor `error`. The `rel=stylesheet` control on the same
        # page loads and fires. Root cause is one hop: the scanner's hint ends
        # up in `Event::SubresourceHintFound`, whose only consumer in the
        # workspace is the stderr logger at `crates/shell/src/main.rs:285` —
        # so `⤷ preload js [medium] <URL>` appears in the log for a request
        # that was never made (BUG-826). Sorted with the element-resource
        # family above and below `iframe`/`img`, which are older causes.
        "preload-hint-never-fetched", "BUG-826",
        [r"<link\b[^>]*\brel\s*=\s*['\"]?(?:[^'\">]*\s)?"
         r"(?:preload|modulepreload|prefetch)\b"
         r"|\.rel\s*=\s*['\"](?:[^'\"]*\s)?(?:preload|modulepreload|prefetch)\b"
         r"|rel:\s*['\"](?:preload|modulepreload|prefetch)['\"]"
         r"|nextValueFromServer"],
        "a `rel=preload`/`modulepreload`/`prefetch` link is never fetched and "
        "never fires `load`/`error` — the hint is logged and dropped, so a "
        "test awaiting the preload (or polling the server for it) waits "
        "forever",
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
    Mechanism(
        # Probed live in slice 15 (`verify_event_delivery_gaps.py`): a page
        # holding listeners for all four animation and all four transition
        # types hears none of them, while a WAAPI `finish` on the same page
        # arrives — so this is the CSS-driven side specifically, not events in
        # general. `grep -rn animationstart crates/` finds the interface and
        # the `on*` attribute name and no dispatch site at all.
        "css-animation-events", "BUG-503/BUG-536",
        [r"animationstart|animationend|animationiteration|animationcancel|"
         r"transitionrun|transitionstart|transitionend|transitioncancel"],
        "a CSS animation/transition is invisible to JS — no event of either "
        "family is ever dispatched, and neither `getComputedStyle` nor "
        "`getBoundingClientRect` moves while one runs",
    ),
    Mechanism(
        # `<animate>`/`<set>` in markup are `HTMLUnknownElement` (BUG-685), and
        # even the `createElementNS` path only reaches `svg.rs`'s four
        # explicitly-stubbed classes whose `beginElement()` bodies are empty.
        "smil-animation", "BUG-806",
        [r"beginEvent|endEvent|repeatEvent|\.beginElement\(|\.endElement\(|"
         r"<animate\b|<animateTransform|<animateMotion|<set\b"],
        "SMIL is absent — no timing model, no attribute animation, and the "
        "`beginEvent`/`endEvent`/`repeatEvent` a test waits for never fire",
    ),
    Mechanism(
        # Probed live in slice 17 (`verify_layout_shift_and_peer_gaps.py`): the
        # API is *advertised* — `supportedEntryTypes` lists `layout-shift` and
        # `observe()` accepts it — and never delivers, because the Rust trigger
        # `deliver_layout_shift` (`crates/shell/src/main.rs:2925`) is
        # `#[allow(dead_code)]` with no call site in layout/reflow at all. The
        # advertisement is what turns these into TIMEOUTs rather than FAILs:
        # WPT's own `ScoreWatcher` (`layout-instability/resources/util.js`)
        # throws unless the type is listed, so an honest "unsupported" would
        # end the test at once instead of hanging it on `watcher.promise`.
        "layout-shift-never-delivered", "BUG-809",
        [r"['\"]layout-shift['\"]|\bScoreWatcher\b|\bLayoutShift\b"],
        "no `layout-shift` entry is ever delivered (the shell-side trigger has "
        "no call site), while `supportedEntryTypes` advertises the type — so a "
        "CLS test observes, shifts and waits forever",
    ),
    Mechanism(
        # Slice 17, same probe: offer/answer complete locally, but the two
        # `RTCPeerConnection`s are never connected to each other —
        # `ondatachannel` never fires on the remote side, `connectionState`
        # is a getter hardcoded to `new`, and a data channel stays
        # `connecting`. Every canonical WPT shape (`RTCPeerConnection-helper.js`
        # `exchangeOfferAnswer` + wait on the remote peer) therefore hangs
        # before its own assertions start.
        "webrtc-no-remote-peer", "BUG-727",
        [r"new RTCPeerConnection\(|createDataChannel\(|\bRTCDataChannel\b|"
         r"RTCPeerConnection-helper"],
        "the `RTCPeerConnection` stub never connects two peers — no "
        "`ondatachannel`, no connection-state change, a data channel never "
        "opens",
    ),
    Mechanism(
        # Slice 17, confirmed live by instrumenting `_handle_action`: the
        # action really does reach the executor, which answers
        # `failure: action 'action_sequence' not implemented by Lumen's
        # minimal WPT executor`; the page-side rejection is then swallowed
        # (BUG-716) and the test hangs instead of failing. Element-targeted
        # actions do not even get that far — `testdriver-extra.js`'s
        # `get_context` throws on the missing `document.defaultView`
        # (BUG-622) — but the observable outcome is the same silent TIMEOUT.
        # `click`/`generate_test_report` are excluded: those two are
        # implemented.
        "testdriver-action-unimplemented", "BUG-810",
        [r"test_driver\.Actions\(|test_driver\.action_sequence\(|"
         r"test_driver\.send_keys\(|test_driver\.bless\(|"
         r"test_driver\.set_permission\(|test_driver\.delete_all_cookies\(|"
         r"test_driver\.get_computed_(?:role|label)\(|"
         r"test_driver\.(?:add|remove)_virtual_authenticator\(|"
         r"test_driver\.set_window_rect\(|test_driver\.minimize_window\(|"
         r"test_driver\.freeze\(|test_driver\.send_report\("],
        "every `test_driver.*` action but `click`/`generate_test_report` is "
        "rejected by `executorlumen.py::_handle_action`, and the rejection is "
        "invisible to the page — so the test waits on an action that will "
        "never complete",
    ),
    Mechanism(
        # Deliberately after the two above and after every wait-shaped marker:
        # plenty of tests build an observer *and* an iframe, and the iframe is
        # the older, better-understood cause.
        "intersection-observer-initial", "BUG-807",
        [r"new IntersectionObserver\("],
        "the observation queued by `observe()` is never delivered — entries "
        "arrive only as a side effect of a later relayout, so a test that "
        "observes and waits hears nothing",
    ),
    Mechanism(
        # Above the BUG-804 marker on purpose, and only for the *call* shape:
        # `test-render-blocking.js` defines `nodeInserted()` and is included by
        # all nine residual `render-blocking` ids, but only three await it. In
        # those three the wait comes first — `promise_setup` cannot finish, so
        # no test in the file ever starts, whatever else the file would have
        # waited for afterwards. Measured in slice 20: a `MutationObserver`
        # armed on `document.documentElement` with `{childList, subtree}` hears
        # nothing about the `<div>`/`<script>` the parser writes right below
        # it, while the same observer reports a node appended from script
        # (BUG-827).
        "mutation-record-parser-insert", "BUG-827",
        [r"await\s+nodeInserted\s*\(|=\s*nodeInserted\s*\("],
        "the test awaits a `MutationObserver` record about an element the "
        "parser inserts, and parser insertions produce no records at all",
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
    # Below the whole table on purpose, both of them: a CSP test routinely
    # builds an `<iframe>` or waits on an `<img>` as well, and those causes are
    # older and better understood, so they must claim the test first. Placed
    # last, these two can only take what nothing else explains.
    Mechanism(
        # CSP is parsed (`crates/network/src/csp.rs`, `crates/storage/src/
        # csp_policies.rs`) and never enforced: `crates/js/src/csp.rs` says so
        # in its own header, and the hook it names — `_lumen_dispatch_csp_
        # violation` — has no caller outside that file's unit test. So no
        # directive blocks anything and, decisively for the verdict, the
        # `securitypolicyviolation` event is never dispatched. Verified live by
        # `tests/wpt/verify_csp_url_worker_gaps.py` (WPT-RUN-6 slice 18): with
        # `img-src 'none'; script-src 'self'` the inline script still runs and
        # neither `window` nor `document` ever hears the event.
        "csp-no-violation-event", "BUG-811",
        [r"securitypolicyviolation", r"SecurityPolicyViolationEvent"],
        "the test waits for a `securitypolicyviolation` that cannot come — "
        "CSP is parsed but never enforced, so no violation is ever reported",
    ),
    Mechanism(
        # `workers/Worker_ErrorEvent_*.htm` and kin: make the worker throw,
        # then wait on `worker.onerror`. The shim fires `error` from exactly
        # one place — the script-fetch-failure branch of the constructor
        # (`crates/js/src/worker.rs`, BUG-364) — so an exception inside a
        # *started* worker propagates nowhere. Both halves are required: `new
        # Worker(` alone is every worker test, and an `error` listener alone is
        # most of the corpus.
        "worker-no-error-event", "BUG-813",
        [r"new\s+(?:Shared)?Worker\s*\(",
         r"\.onerror\s*=|addEventListener\(\s*['\"]error['\"]|ErrorEvent"],
        "an uncaught exception inside a started worker never reaches the "
        "page: `error` is dispatched at the `Worker` object only when the "
        "script fetch itself failed",
        mode="all",
    ),
    Mechanism(
        # Slice 20, same probe: `OfflineAudioContext.startRendering()` resolves
        # at once with a buffer of pure silence (`nonzero=0/4410` for a graph
        # whose oscillator runs the whole render), a source node never fires
        # `ended` under either registration, and `AudioParam` automation
        # changes neither `.value` nor the output. Decisively for the verdict,
        # the shim calls `oncomplete` inside `try { … } catch (e) {}`
        # (`crates/js/src/web_audio.rs`), so the comparison every
        # `audioparam-*` test runs from that handler dies without a word and
        # its audit task never finishes — a TIMEOUT where the same failure on
        # a real engine would be a FAIL (BUG-828).
        "offline-audio-silent", "BUG-828",
        [r"new OfflineAudioContext\(|new AudioContext\(|createOscillator\(|"
         r"AudioBufferSourceNode|createBufferSource\(|\.startRendering\("],
        "Web Audio renders silence: `startRendering()` hands back an empty "
        "buffer, `ended` never fires, automation is inert, and a throw inside "
        "`oncomplete` is swallowed by the shim",
    ),
    # The three scroll/stream markers of slice 19 sort here, below everything
    # older, for the same reason the two above do: a scroll test that also
    # builds an `<iframe>` or waits on a font is claimed by that cause first.
    Mechanism(
        # `scrollend` is not dispatched anywhere in the workspace — there is no
        # `_lumen_fire_scrollend` to match the `_lumen_fire_window_scroll_event`
        # / `_lumen_fire_scroll_on_element` pair, and `'onscrollend' in window`
        # is false. Measured live by `verify_stream_scroll_message_gaps.py`
        # (WPT-RUN-6 slice 19): a page and an element that both *do* scroll and
        # both *do* fire `scroll` never hear `scrollend`. Placed above the
        # page-scroll marker on purpose — an element scroll fires `scroll`
        # correctly, so for a test that waits on `scrollend` the missing event
        # is the whole cause whichever scroller it used.
        "scrollend-never-fired", "BUG-822",
        [r"scrollend"],
        "the test waits for a `scrollend` event, which no scroll path in the "
        "engine dispatches (and `onscrollend` is not on `window` either)",
    ),
    Mechanism(
        # A programmatic page scroll works — `window.scrollTo(0, 300)` really
        # moves the page and `window.scrollY` reports 300 afterwards (measured;
        # the earlier reading of 0 was a probe page whose spacer painted
        # nothing, so `content_height` was 0) — but it dispatches no `scroll`
        # event: `fire_window_scroll` (`crates/shell/src/main.rs:3505`) has
        # exactly one caller, the mouse-wheel branch at `main.rs:16085`. The
        # element path is fine (`fire_element_scroll` runs for `scrollTo` and
        # `scrollTop=` alike), which is why both halves are required below:
        # the wait must be on a *page-level* `scroll`.
        "page-scroll-no-scroll-event", "BUG-821",
        [r"window\.scroll(?:To|By)\s*\(|document\.scrollingElement"
         r"|\bscrollIntoView\s*\(",
         r"window\.addEventListener\(\s*['\"]scroll['\"]"
         r"|document\.addEventListener\(\s*['\"]scroll['\"]"
         r"|window\.onscroll\s*=|(?<![.\w])onscroll\s*="],
        "the test scrolls the page from script and waits for the `scroll` "
        "event, which only a mouse-wheel scroll dispatches",
        mode="all",
    ),
    Mechanism(
        # The Streams shim (`crates/js/src/dom.rs:7303-7600`) settles promises
        # on the happy path only: `writer.closed` never settles at all,
        # `controller.error()` reaches no pending promise, a `start()` that
        # returns a rejected promise or a thenable is ignored, the sink's
        # `abort()` is never called, `tee()` closes its source and drops every
        # chunk enqueued after the call, `getReader({mode:'byob'})` hands back
        # a default reader, and there is no `Symbol.asyncIterator`. Measured
        # two ways in slice 19: the probe variants of
        # `verify_stream_scroll_message_gaps.py`, and a per-file sweep that ran
        # each residual `streams/*` test with `add_result_callback` logging —
        # 35 of the 36 runnable ones stop mid-file, always on a subtest of one
        # of those shapes (BUG-823 for the unsettled promises, BUG-824 for the
        # missing surfaces). Both halves are required so that a test merely
        # *mentioning* a stream is not claimed.
        "streams-promise-unsettled", "BUG-823/BUG-824",
        [r"new\s+(?:Readable|Writable|Transform)Stream\s*\("
         r"|TextDecoderStream|TextEncoderStream",
         r"\.closed\b|\.abort\s*\(|\.tee\s*\("
         r"|mode:\s*['\"]byob['\"]|controller\.error\s*\("
         r"|\.pipeTo\s*\(|\.getWriter\s*\("],
        "the test drives a stream through an error/close/cancel path, where "
        "the shim leaves the promise pending forever",
        mode="all",
    ),
    Mechanism(
        # Slice 20 re-measured what BUG-568 still covers after BUG-701 added
        # `write`/`writeln`: writing plain markup during parsing works and the
        # node is in the tree, but a `<script>` written that way never runs,
        # and `document.open` is still `not a function` (`document.open is not
        # a function` from the probe, verbatim). Both shapes below are that
        # residue — a test that writes a script and waits for it, or one that
        # reopens the stream — and both are silent: nothing is thrown at the
        # page, the written script simply never executes.
        "document-write-script-inert", "BUG-568",
        [r"document\.open\s*\(|document\.write\s*\(\s*[\"'`]\s*<\s*(?:scr|/scr)"],
        "a `<script>` handed to `document.write()` is never executed and "
        "`document.open()` does not exist, so a test that writes its own "
        "script and waits for it hears nothing",
    ),
    Mechanism(
        # Slice 21, measured before the marker was written. Sits below the
        # `document.write` marker because a written script is the older and
        # more specific cause, and above the window-error marker for the usual
        # reason: this names a mechanism, that one only says "something threw".
        "timer-string-handler", "BUG-831", [],
        "the test schedules a *string* through `setTimeout`/`setInterval`, "
        "which the shim drops without compiling it (and returns a timer id "
        "anyway, so nothing looks wrong)",
        predicate=_timer_string_handler_marker,
    ),
    Mechanism(
        # Slice 21. Order-independent in practice — no other marker matches a
        # bare `location.hash` assignment — but kept next to its neighbours in
        # the same-document-navigation group rather than at the end.
        "hashchange-listener-too-late", "BUG-832", [],
        "the test sets `location.hash` and only then attaches its "
        "`hashchange` listener; the engine dispatches the event synchronously "
        "from the setter, so the listener is registered a line too late",
        predicate=_hashchange_after_assignment_marker,
    ),
    Mechanism(
        # Slice 21, from slice 19's measurement (see `_postmessage_options_marker`).
        "postmessage-options-dropped", "BUG-717", [],
        "the test posts a message through the one-argument or "
        "`WindowPostMessageOptions` overload, which the shim drops silently, "
        "and then waits for it to arrive",
        predicate=_postmessage_options_marker,
    ),
    Mechanism(
        # Slice 22. Below the spin marker on purpose: a file can do both, and
        # a frozen page is the earlier cause.
        "idb-no-connection-queue", "BUG-843", [],
        "the test waits for `versionchange` / `blocked`, which the shim never "
        "dispatches — a second `open()` upgrades under a live connection",
        predicate=_idb_connection_queue_marker,
    ),
    Mechanism(
        # Slice 22.
        "idb-empty-transaction", "BUG-841", [],
        "the test awaits `complete` on a transaction that never gets a "
        "request, and only a request queues a transaction for commit",
        predicate=_idb_empty_transaction_marker,
    ),
    Mechanism(
        # Slice 22. Sorts below `layout-shift-never-delivered` in effect (that
        # marker is far above) — the two name the same shape of defect for
        # different entry types and cannot both match: their entry-type
        # literals differ.
        "resource-timing-entry-never-delivered", "BUG-839", [],
        "the test waits for a `resource` PerformanceEntry (or for the "
        "observer callback's `droppedEntriesCount`), neither of which the "
        "engine ever produces although `resource` is advertised",
        predicate=_resource_timing_marker,
    ),
    Mechanism(
        # Slice 22. Every WPT `eventsource` stream is a `.py` handler answering
        # with a Content-Length, and that shape never ends for the engine — so
        # any test needing more than the first response's messages hangs.
        "eventsource-no-reconnect", "BUG-844",
        [r"new\s+EventSource\s*\(",
         r"\.onmessage\s*=|\.onopen\s*=|addEventListener\s*\(\s*['\"](?:message|open)"],
        "the test waits for a message or an `open` that only a reconnect can "
        "deliver; a stream ended by the response body is never treated as "
        "ended, and the reconnects that do happen fire no `open`",
        mode="all",
    ),
    Mechanism(
        # Slice 22.
        "compression-stream-read-before-close", "BUG-846", [],
        "the test reads a Compression Streams reader before closing the "
        "writable side, and the shim emits its only chunk from `flush`",
        predicate=_compression_read_marker,
    ),
    Mechanism(
        # Slice 22.
        "timer-overflow-delay", "BUG-847",
        [r"set(?:Timeout|Interval)\s*\([^,]+,\s*(?:Math\.pow\(\s*2\s*,\s*3[12]\s*\)|2\s*\*\*\s*3[12]|\d{10,})"],
        "the test schedules a timer with a delay above 2^31-1 and waits for "
        "it to fire immediately (WebIDL `long` conversion), which the shim "
        "does not apply",
    ),
    Mechanism(
        # Slice 22. Below `preload-hint-never-fetched` and `media-element`:
        # both name a narrower cause for the same silence.
        "element-subresource-never-requested", "BUG-848/BUG-825", [],
        "the test induces a subresource request through an element the "
        "request collector does not know (`poster`, `<input type=image>`, SVG "
        "`<image>`, `rel=icon`, media `src`) and waits for its `load`/`error`",
        predicate=_element_subresource_marker,
    ),
    # Truly last: "the test listens for an error" is the weakest of these
    # claims, so anything with a named cause above must take the test first.
    # Late on purpose: this marker says why a failure surfaced as a TIMEOUT
    # rather than what failed, so every marker naming an actual engine gap
    # must be tried first — `pointerevent_setpointercapture_*` is a
    # `<body onload>` single-test page *and* a testdriver-action page, and
    # BUG-810 is the finding worth having.
    Mechanism(
        "single-test-load-handler-throw", "BUG-591", [],
        "the whole test body runs from `<body onload>` under "
        "`setup({single_test: true})`: a failing assertion throws, `done()` is "
        "never reached and the exception is swallowed by the window-load "
        "dispatch path, so a FAIL surfaces as NOTRUN + TIMEOUT",
        predicate=_single_test_load_handler_marker,
    ),
    Mechanism(
        "window-error-event-never-fired", "BUG-716/BUG-591", [],
        "the test completes from a window-level `error` / `unhandledrejection` "
        "event, and the engine dispatches neither",
        predicate=_window_error_wait_marker,
    ),
]

#: Fourth stage, applied only after `SOURCE_MARKERS` has failed, and matched
#: against the *worker script* rather than the test file.
#:
#: A worker test's own source says nothing about why it hangs — the wait is one
#: `worker.onmessage`, and what never happens happens on the other side of
#: `new Worker(url)`. The script at that url is not a `<script src>` helper, so
#: `helper_paths` does not reach it and every one of these ids stayed in the
#: residual. Kept a separate table rather than folded into `SOURCE_MARKERS`
#: because the two must not see the same text: `navigator` appears in the *page*
#: half of `WorkerNavigator_appName.htm` too (`assert_equals(e.data.appName,
#: navigator.appName)`), and matching that would claim any test that merely
#: mentions the object (WPT-RUN-6 slice 18).
WORKER_SOURCE_MARKERS = [
    Mechanism(
        # `navigator` is never defined in the worker global scope: the shim
        # (`crates/js/src/worker.rs:274-390`) defines `self`, `name`,
        # `postMessage`, `onmessage`, `addEventListener`, `console`,
        # `importScripts`, the timer stubs and `queueMicrotask`, and nothing
        # else — so `navigator.platform` throws, and the throw is itself
        # invisible (BUG-813), leaving the page's `onmessage` to wait forever.
        # Measured live (`verify_csp_url_worker_gaps.py --variant
        # worker-navigator`): `typeof navigator=undefined self=object
        # location=undefined setTimeout=function`.
        # Filed as BUG-814, merged into BUG-776 as a duplicate on 2026-08-22
        # (BUG-776 is earlier and wider — it also covers the service worker,
        # whose scope has no `navigator` either).
        "worker-navigator-missing", "BUG-776",
        [r"\bnavigator\s*\.|\blocation\s*\."],
        "the worker reads `navigator`/`location`, which the worker global "
        "scope does not define at all — it throws before its `postMessage` "
        "and the page waits forever",
    ),
    Mechanism(
        # The timer stubs only run from `_lumen_flush_timers`, which the Rust
        # side calls between *message dispatches*, and `setInterval` is an
        # alias of `setTimeout` (`worker.rs:377`), so it never repeats. A
        # worker that is never sent a message never flushes at all. Measured
        # live (`--variant worker-timers` / `worker-timers-poked`): a worker
        # arming both posts the microtask immediately and the timeout only
        # after the page pokes it, and `interval:2` never comes.
        "worker-timers-not-driven", "BUG-815",
        [r"\bset(?:Timeout|Interval)\s*\("],
        "the worker arms a timer, but worker timers are flushed only when a "
        "message is dispatched to that worker and `setInterval` never repeats",
    ),
]


#: `new Worker('url')` / `new SharedWorker('url')` with a literal URL — the
#: only form this stage can follow. A constructed or `blob:` URL is skipped
#: rather than guessed at.
_WORKER_URL_RE = re.compile(r"new\s+(?:Shared)?Worker\s*\(\s*['\"]([^'\"]+)['\"]")

#: Generated ids whose *source file is itself the worker script*: `.any.js`
#: and `.worker.js` are expanded by the manifest into one id per scope, and
#: only the worker-scoped ones run in a worker. The window-scoped siblings
#: (`.any.html`, `.window.html`) must not be matched against this table — they
#: run where `navigator` exists and timers are driven normally.
_WORKER_SCOPE_SUFFIXES = (".worker.html", ".any.worker.html",
                          ".any.sharedworker.html", ".any.worker-module.html",
                          ".any.serviceworker.html")


def worker_scripts(test_id, root, lines):
    """Paths of the worker scripts a test starts, `new Worker(url)` by `url`.

    Resolved like `helper_paths` (root-relative or relative to the test file),
    and, like it, one level deep: a worker that itself spawns a worker is rare
    and each level widens what a marker may be attributed to.
    """
    src = source_path(test_id, root)
    out = []
    for ref in _WORKER_URL_RE.findall("\n".join(lines)):
        ref = ref.split("#")[0].split("?")[0]
        if not ref or "://" in ref or ref.startswith(("data:", "blob:")):
            continue
        if ref.startswith("/"):
            path = os.path.join(root, ref.lstrip("/"))
        else:
            path = os.path.normpath(os.path.join(os.path.dirname(src), ref))
        if os.path.isfile(path) and path not in out:
            out.append(path)
    return out


#: Fifth stage, matched against the *subframe document* a test embeds.
#:
#: The BUG-480 marker in `SOURCE_MARKERS` requires the wait to be visible in
#: the test file — `iframe.onload`, `contentWindow`, an `onmessage` handler.
#: A large family waits in the opposite direction: the parent defines a global
#: (`window.t`, `do_test`, `parent.success`) and the *child* calls it once it
#: has loaded, so the test file contains an `<iframe>`, a callback nobody in
#: that file ever invokes, and no wait at all to match. Reading the child is
#: what makes the wait visible, exactly as the worker stage reads the worker
#: script: all of `unloading-documents/unload/*`, `the-location-interface/
#: assign_*`, `opening-the-input-stream/01*`, `webstorage/event_*` and
#: `xhr/open-url-multi-window*` are that shape (WPT-RUN-6 slice 21, 47 ids).
#:
#: The cause is still BUG-480 and nothing new: measured again in slice 21
#: (`verify_navigation_form_import_gaps.py --variant iframe-src-child-runs`),
#: a URL-addressed subframe never runs a line of script and `contentWindow` is
#: null, so the callback the parent is waiting for cannot happen.
SUBFRAME_SOURCE_MARKERS = [
    Mechanism(
        "iframe-child-callback-never-runs", "BUG-480",
        [r"\bparent\s*\.|\btop\s*\.|\bwindow\.parent\b|postMessage\s*\("],
        "the test waits for its subframe to call back into the page, and a "
        "URL-addressed subframe never loads or runs any script at all",
    ),
]


#: `<iframe src=...>`, `frame.src = '...'` and `setAttribute('src', '...')` —
#: the three shapes a WPT test uses to point a subframe at a document. All
#: three occur in the residual, and `setAttribute` is not decoration: it is how
#: `the-history-interface/009.html` and `010.html` load their child.
_IFRAME_SRC_RE = re.compile(
    r"""<iframe[^>]*?\ssrc\s*=\s*["']([^"'>]+)["']"""
    r"""|\.src\s*=\s*["']([^"']+)["']"""
    r"""|setAttribute\(\s*['"]src['"]\s*,\s*['"]([^'"]+)['"]""",
    re.IGNORECASE | re.DOTALL)


def subframe_documents(test_id, root, lines):
    """Paths of the local documents a test points a subframe at.

    Resolved like `helper_paths`/`worker_scripts` and, like them, one level
    deep. `javascript:`/`data:`/`about:` and cross-origin URLs are skipped
    rather than guessed at — a `javascript:` frame is a different mechanism
    (the URL is the script) and has to be measured on its own before it can be
    claimed.
    """
    src = source_path(test_id, root)
    out = []
    for match in _IFRAME_SRC_RE.finditer("\n".join(lines)):
        ref = next(group for group in match.groups() if group)
        ref = ref.split("#")[0].split("?")[0]
        if not ref or "://" in ref or ref.startswith(("data:", "javascript:",
                                                      "blob:", "about:")):
            continue
        if ref.startswith("/"):
            path = os.path.join(root, ref.lstrip("/"))
        else:
            path = os.path.normpath(os.path.join(os.path.dirname(src), ref))
        if os.path.isfile(path) and path not in out:
            out.append(path)
    return out


#: Third stage, applied after both the evidence table and the source markers.
#:
#: "The page threw something" is real evidence — the browser said so — but it
#: names no cause, so it must not outrank a source marker that does: a page
#: whose `document.fonts.ready` is undefined *also* prints a `script error:`
#: line, and reporting that one as "some exception" would bury the mechanism
#: (WPT-RUN-6 slice 14). Whatever this stage claims is still a work list, not
#: an answer — `swallowed_errors` in `--json` keeps the error texts so the
#: next slice can pick the next API out of it.
WEAK_MECHANISMS = [
    Mechanism(
        "script-error-swallowed", "BUG-591",
        [r"^(?:script|module) error:", r"\[JS error\] Uncaught"],
        "the page threw and the engine printed it, but no `error` event was "
        "dispatched, so the harness never saw the failure",
    ),
]

#: Lines that carry a page-script exception, for the `swallowed_errors`
#: histogram of the third stage.
_ERROR_LINE_RE = re.compile(r"^(?:script|module) error:|\[JS error\] Uncaught")


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


def _decode_source(path):
    """Text of a test/helper file, or None if it cannot be read.

    The BOM sniff is not cosmetic: `html/infrastructure/urls/resolving-urls/
    query-encoding/utf-16{le,be}.html` and their kin are stored as UTF-16, and
    decoding those bytes as UTF-8 leaves every character separated by a
    replacement byte, so not one marker regex can ever match. All 20 UTF-16
    ids of the WPT-RUN-5 residual were invisible to this stage for that reason
    alone (WPT-RUN-6 slice 16).
    """
    try:
        with open(path, "rb") as handle:
            raw = handle.read()
    except OSError:
        return None
    if raw[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return raw.decode("utf-16", "replace")
    return raw.decode("utf-8", "replace")


def _read_source(path, cache):
    if path not in cache:
        text = _decode_source(path)
        cache[path] = None if text is None else text.splitlines()
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

#: `import {runTests} from "./support/x.js"` / `import "./x.js"` — how an
#: inline `<script type="module">` (and a module helper) pulls in its code.
#: Neither include shape above sees it, so a test whose whole body is one
#: `import` plus one call was invisible to this stage (WPT-RUN-6 slice 14).
_ES_IMPORT_RE = re.compile(
    r"""\bimport\s+(?:[\w*{}\s,$]+\s+from\s+)?['"]([^'"]+)['"]""")

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

    A third shape is the ES-module `import`: a modern CSS test is often an
    inline `<script type="module">` whose entire body is
    `import {runTests} from "./support/x.js"` plus one call, and the helper it
    names is where the marker lives. All 32 `css-grid/abspos` residual ids of
    slice 13 were that shape — their `document.fonts.ready` (BUG-564) sits in
    `positioned-grid-descendants.js` and nothing pointed at it from markup
    (WPT-RUN-6 slice 14).

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
    text = _decode_source(src)
    if text is None:
        return []
    out = []
    for ref in (_SCRIPT_SRC_RE.findall(text) + _META_SCRIPT_RE.findall(text)
                + _ES_IMPORT_RE.findall(text)):
        ref = ref.split("#")[0].split("?")[0]
        # `.mjs` as well as `.js`: an ES-module helper is how the whole
        # `browsing-the-web` family builds its `<iframe>` (`helpers.mjs`), and
        # dropping the extension made 22 of its residual ids unattributable
        # (WPT-RUN-6 slice 16).
        if not ref or ref in _NOT_A_HELPER or not ref.endswith((".js", ".mjs")):
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


#: `subsetTestByKey('history', async_test, ...)` — `common/subset-tests-by-key.js`.
#: A test file using it is really N test files: the manifest expands it into one
#: id per `?include=<key>` variant, and a variant runs only its own blocks.
_SUBSET_KEY_RE = re.compile(r"subsetTestByKey\(\s*['\"]([^'\"]+)['\"]")


def _restrict_to_subset(lines, key):
    """Drop the blocks belonging to keys other than `key`.

    Without this, all ten `?include=` variants of `query-encoding/utf-16le.html`
    read the same 600-line helper and all ten got the first marker any block of
    it matched — `iframe-no-nested-context`, from the `submit` block, even for
    `?include=xhr`, whose whole body is one `XMLHttpRequest` (WPT-RUN-6 slice
    16). Blocks are sequential in every user of this helper, so "skip from a
    foreign key's call until the next call" is enough; the preamble before the
    first call is always kept, because it is what the selected block runs
    inside.
    """
    out, skipping = [], False
    for line in lines:
        match = _SUBSET_KEY_RE.search(line)
        if match:
            skipping = match.group(1) != key
        if not skipping:
            out.append(line)
    return out


#: Bug owning each mechanism the standalone probe measured. Kept here and not
#: in the probe so the audit still names an owner when the probe file is gone.
MEASURED_REFS = {
    "grid-implicit-track-loop": "BUG-801",
    "svg-transform-loop": "BUG-803",
    "nested-flex-exponential": "BUG-802",
    "dom-wrapper-oom": "BUG-849",
    "unclamped-blur": "BUG-850",
}


def _load_measured_hangs():
    """`test id -> mechanism key` for the pages measured to hang standalone.

    The table lives in `verify_layout_hangs.py` (the probe that produced it, by
    running every residual id under `--dump-layout` with a timeout) and is read
    from there rather than copied, so a re-measurement updates both users at
    once. Loaded by path, not by `import`, because the audit is run from the
    repo root as often as from `tests/wpt`. A missing or broken probe file
    disables the stage instead of failing the run — the audit's other three
    stages do not depend on it.
    """
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        "verify_layout_hangs.py")
    try:
        spec = importlib.util.spec_from_file_location("verify_layout_hangs", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        measured = module.MEASURED
    except (OSError, AttributeError, ImportError, SyntaxError):
        return {}
    return {test_id: mech for mech, ids in measured.items() for test_id in ids}


#: Filled at import; see `_load_measured_hangs`.
MEASURED_HANGS = _load_measured_hangs()


def classify_measured(test_id):
    """Key for a test measured to hang/die standalone, or None.

    The strongest evidence the audit has, and therefore the first stage to run
    on what the output stage could not claim: the page was executed on its own,
    outside wptrunner and outside `testharness.js`, and did not finish. A regex over the source
    cannot replace it — whether a grid item reaches past the last explicit line
    depends on the *resolved* track count, which `repeat(auto-fill, ...)` makes
    a function of the container width (BUG-801), and a cost curve (BUG-802) has
    no textual marker at all.
    """
    if not test_id:
        return None
    return (MEASURED_HANGS.get(test_id)
            or MEASURED_HANGS.get(test_id.split("?")[0]))


class SubtestMarker:
    """One mechanism recognized from the harness's *own* partial report.

    `wptrunner` writes every subtest it managed to collect before the timeout
    into the shard's `wptreport` json, so a file that hung inside one
    `async_test` names that test — and the subtests that PASSed next to it
    prove the page was alive and the harness loaded. That is evidence produced
    by the run, not a guess read off the file, which is why this stage sorts
    above `SOURCE_MARKERS`.

    `name` is matched against the names of the *hung* subtests (`TIMEOUT` /
    `NOTRUN`) only; `test` optionally restricts the marker to ids matching a
    pattern. The winner is the marker matching the most hung subtests, not the
    first one in the table: a file whose 24 subtests split 18/6 between two
    mechanisms is attributed to the mechanism that actually stopped it, and
    ties fall back to table order.
    """

    def __init__(self, key, ref, name=None, test=None, note=""):
        self.key = key
        self.ref = ref
        self.name = re.compile(name) if name else None
        self.test = re.compile(test) if test else None
        self.note = note

    def score(self, test_id, hung_names):
        """How many hung subtests of this test the marker claims (0 = none)."""
        if self.test is not None and not self.test.search(test_id or ""):
            return 0
        if self.name is None:
            return len(hung_names)
        return sum(1 for name in hung_names if self.name.search(name))


#: Third stage, between `MEASURED_HANGS` and `SOURCE_MARKERS`. Every entry was
#: measured against a live browser by `verify_frame_load_media_gaps.py`
#: (WPT-RUN-6 slice 24) — the subtest name only says *which* wait hung, the
#: probe says why it can never finish.
SUBTEST_MARKERS = [
    # `<frame src>`, `<object data>` and `<embed src>` request nothing at all
    # (proved on the probe's own server) and fire no `load`; `<frame>` is not
    # even an `HTMLFrameElement`. The `<iframe>` subtests of the same files are
    # BUG-480 — the majority rule decides which of the two owns the file.
    SubtestMarker(
        "nbc-element-never-loads", "BUG-854 / BUG-798 (+BUG-480 для iframe)",
        name=r"nested browsing context must be navigated|"
             r"load nested browsing context <(?:frame|object|embed)\b|"
             r"^(?:same|cross)-origin <(?:frame|object|embed)[ >]",
        note="`<frame>`/`<object data>`/`<embed src>` never request their "
             "resource and never fire `load`, so a test waiting for the "
             "nested browsing context to appear cannot finish",
    ),
    SubtestMarker(
        "iframe-no-nested-context", "BUG-480",
        name=r"load nested browsing context <iframe\b|"
             r"^(?:same|cross)-origin <iframe[ >]",
        note="an `<iframe src>`'s sub-document is never fetched (Phase 0, "
             "`main.rs:5408`), so its `load` never fires — measured with the "
             "probe's server, which is never asked for the child",
    ),
    # `<details>`: the shim fires `toggle` from the summary-click path only,
    # and that path toggles `open` twice (listener + activation behaviour), so
    # every "adding open should fire a toggle event" subtest hangs.
    SubtestMarker(
        "details-toggle-not-fired", "BUG-851",
        name=r"toggle event at the '?details|Setting open from the parser",
        note="a script-driven `open` change fires no `toggle` at all — the "
             "shim's only dispatch site is the summary-click handler",
    ),
    # `ResizeObserver` never delivers the initial observation, so a callback
    # armed on a static element is never entered (BUG-661, re-measured).
    SubtestMarker(
        "resize-observer-no-initial", "BUG-661",
        name=r"^contain-intrinsic-size: auto$",
        test=r"/css/css-sizing/contain-intrinsic-size/",
        note="the test's whole body runs from a `ResizeObserver` callback, "
             "and no initial observation is ever delivered",
    ),
    SubtestMarker(
        "content-visibility-state-event", "BUG-852",
        name=r"ContentVisibilityAutoStateChange|content-visibility",
        test=r"/css/css-contain/content-visibility/",
        note="`contentvisibilityautostatechange` is never dispatched and "
             "`content-visibility` is absent from computed style",
    ),
    # An incubating element (WICG/PEPC): `<usermedia>`/`<geolocation>`/
    # `<install>` are `HTMLUnknownElement`, so `onvalidationstatuschange`
    # never fires. Not an engine defect — an unimplemented proposal.
    SubtestMarker(
        "permission-element-unimplemented", "нет (PEPC, incubation)",
        test=r"/html/semantics/permission-element/",
        note="the `<permission>` family is not implemented; the tests wait "
             "for `onvalidationstatuschange` on an `HTMLUnknownElement`",
    ),
    SubtestMarker(
        "media-resource-selection", "BUG-825",
        name=r"volumechange|playbackRate|resource selection|candidate|"
             r"error event with load\(\)|currentSrc",
        test=r"/html/semantics/embedded-content/(?:media-elements|the-video-element)/",
        note="the media resource selection algorithm never runs: a `<source>` "
             "candidate is never requested, `networkState`/`currentSrc`/"
             "`playbackRate` are `undefined` and `load()` fires nothing",
    ),
    SubtestMarker(
        "script-empty-src", "BUG-853",
        name=r"Script src with an empty URL",
        note="`<script src=\"\">` fires neither `load` nor `error`, so a "
             "test waiting for the error event hangs",
    ),
    # The clicked node's own activation behaviour is looked up instead of the
    # nearest activatable ancestor (BUG-837), so a click on an inline element
    # inside a `<label>`/`<a>` does nothing.
    SubtestMarker(
        "click-on-inner-element", "BUG-837",
        name=r"inline element inside a label|anchor with embedded inline element|"
             r"child of a button with \.click\(\)",
        note="activation behaviour is resolved on the clicked node rather "
             "than on the nearest activatable ancestor",
    ),
]


def read_subtest_report(out_dir):
    """`test id -> [(subtest name, status)]` from a run's wptreport files.

    `run_corpus.py` writes both a mozlog `<shard>.raw.jsonl` and a wptreport
    `<shard>.json` per shard; the audit reads the first for browser output and
    this one for what the harness itself managed to report. A shard whose json
    is missing or truncated (a run killed mid-write) contributes nothing rather
    than failing the audit.
    """
    report = {}
    for path in sorted(glob.glob(os.path.join(out_dir, "*.json"))):
        try:
            with open(path, encoding="utf-8", errors="replace") as handle:
                data = json.load(handle)
        except (OSError, ValueError):
            continue
        for record in data.get("results") or ():
            test = record.get("test")
            if not test:
                continue
            report[test] = [(sub.get("name") or "", sub.get("status") or "")
                            for sub in record.get("subtests") or ()]
    return report


def hung_subtests(subtests):
    """The names of the subtests that never finished."""
    return [name for name, status in subtests if status in ("TIMEOUT", "NOTRUN")]


def classify_subtests(test_id, subtests):
    """Key for a test whose partial harness report names the hung wait, or None.

    Runs after `MEASURED_HANGS` (a page proven to hang standalone outranks
    everything) and before `SOURCE_MARKERS` (a grep of the file is weaker than
    the run's own record of which test hung).
    """
    hung = hung_subtests(subtests)
    if not hung:
        return None
    best, best_score = None, 0
    for marker in SUBTEST_MARKERS:
        score = marker.score(test_id, hung)
        if score > best_score:
            best, best_score = marker, score
    return best.key if best else None


def classify_source(test_id, root, cache, follow_helpers=True):
    """Second-stage key for a test the output stage could not claim, or None.

    Reads the test's own source plus, unless `follow_helpers` is off, the
    `<script src>` helpers it includes (`helper_paths`) — a marker in a helper
    is a marker of the test, and the pre-slice-10 test-file-only reading made
    the stage a strict lower bound (see `SOURCE_MARKERS`).
    """
    global _SOURCE_ROOT
    _SOURCE_ROOT = root
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
    subset = re.search(r"[?&]include=([^&]+)", test_id or "")
    if subset and any(_SUBSET_KEY_RE.search(line) for line in lines):
        lines = _restrict_to_subset(lines, subset.group(1))
    for mech in SOURCE_MARKERS:
        if mech.matches(lines, test_id):
            return mech.key
    # Fourth stage: what the *worker* runs. Deliberately last — a worker test
    # that also builds an `<iframe>` or waits on a font is claimed by the older
    # and better-understood cause above.
    worker_lines = []
    if (test_id or "").split("?")[0].endswith(_WORKER_SCOPE_SUFFIXES):
        # The test source is the worker script; it has already been read.
        worker_lines += lines
    for path in worker_scripts(test_id, root, lines):
        script_lines = _read_source(path, cache)
        if script_lines:
            worker_lines += script_lines
    if worker_lines:
        for mech in WORKER_SOURCE_MARKERS:
            if mech.matches(worker_lines, test_id):
                return mech.key
    # Fifth stage: what the *subframe* runs. Last for the same reason the
    # worker stage is next-to-last — a test that also waits on a font, a
    # stream or a `window.open` channel is claimed by that older cause first.
    # Measured for overlap when the stage was added (slice 21): of the 47 ids
    # it claims, none is matched by the worker table, so the two orders are
    # equivalent on today's snapshot; last is the conservative choice anyway.
    frame_lines = []
    for path in subframe_documents(test_id, root, lines):
        child_lines = _read_source(path, cache)
        if child_lines:
            frame_lines += child_lines
    if frame_lines:
        for mech in SUBFRAME_SOURCE_MARKERS:
            if mech.matches(frame_lines, test_id):
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


def classify(lines, mechanisms=None):
    """Return the key of the first mechanism claiming these output lines."""
    if not lines:
        return NO_OUTPUT
    for mech in (MECHANISMS if mechanisms is None else mechanisms):
        if mech.matches(lines):
            return mech.key
    return UNCLASSIFIED


def audit(out_dir, category=None, root=WPT_ROOT, use_source=True,
          follow_helpers=True, use_measured=True, use_subtests=True):
    """Classify every TIMEOUT in a run directory.

    `category` filters by manifest-id prefix (`html`, `html/canvas`, ...);
    `use_measured` enables the measured-hang stage (`MEASURED_HANGS`);
    `use_subtests` enables the partial-harness-report stage
    (`SUBTEST_MARKERS`); `use_source` enables the source-marker stage
    (`SOURCE_MARKERS`); `follow_helpers` lets that stage read the test's
    `<script src>` helpers as well as the test file itself (`helper_paths`).
    """
    source_cache = {}
    subtest_report = read_subtest_report(out_dir) if use_subtests else {}
    subtest_evidence = {}
    counts = collections.Counter()
    by_cat = collections.defaultdict(collections.Counter)
    residual_sigs = collections.Counter()
    examples = collections.defaultdict(list)
    residual_examples = collections.defaultdict(list)
    residual_ids = []
    totals = collections.Counter()
    hangs = {}
    swallowed = collections.Counter()
    swallowed_examples = collections.defaultdict(list)

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
            if key in (NO_OUTPUT, UNCLASSIFIED):
                # Measured first: a page proven to hang standalone outranks
                # anything its source merely suggests.
                measured = classify_measured(test) if use_measured else None
                from_subtests = (classify_subtests(test, subtest_report.get(test, []))
                                 if use_subtests else None)
                if measured:
                    key = measured
                elif from_subtests:
                    # The harness's own partial report: it names the subtest
                    # that never finished, which a grep of the file cannot —
                    # `resolve-url.js` builds all four nested-context elements
                    # in one loop over `createElement(tag)`.
                    key = from_subtests
                elif use_source:
                    from_source = classify_source(test, root, source_cache,
                                                  follow_helpers=follow_helpers)
                    if from_source:
                        key = from_source
            if key == UNCLASSIFIED:
                weak = classify(lines, WEAK_MECHANISMS)
                if weak != UNCLASSIFIED:
                    key = weak
                    for sig in sorted({normalize(line) for line in lines
                                       if _ERROR_LINE_RE.search(line)}):
                        swallowed[sig] += 1
                        if len(swallowed_examples[sig]) < 5:
                            swallowed_examples[sig].append(test)
            counts[key] += 1
            by_cat[category_of(test)][key] += 1
            if len(examples[key]) < 200:
                examples[key].append(test)
            if key in (UNCLASSIFIED, NO_OUTPUT):
                # The full residual list, not a sample: the next slice picks
                # its target out of it, and 3k ids is small next to the run.
                residual_ids.append(test)
                # ...and, where the harness got that far, the names of the
                # subtests that never finished. This is the work list the
                # next slice reads: a name is a locator for the wait, which
                # neither the browser output nor the file name gives.
                hung = hung_subtests(subtest_report.get(test, []))
                if hung:
                    subtest_evidence[test] = hung
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
        "residual_hung_subtests": subtest_evidence,
        "swallowed_errors": dict(swallowed),
        "swallowed_examples": {k: v for k, v in swallowed_examples.items()},
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
    refs.update({m.key: m.ref + " (subtest marker)" for m in SUBTEST_MARKERS})
    refs.update({m.key: m.ref + " (source marker)" for m in SOURCE_MARKERS})
    refs.update({m.key: m.ref + " (worker-source marker)"
                 for m in WORKER_SOURCE_MARKERS})
    refs.update({m.key: m.ref for m in WEAK_MECHANISMS})
    refs.update({key: ref + " (measured standalone)"
                 for key, ref in MEASURED_REFS.items()})
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
    swallowed = result.get("swallowed_errors") or {}
    if swallowed:
        print()
        print(f"exceptions nobody saw, by text (top {top}) — each is an engine "
              f"gap or a real FAIL that BUG-591 turned into a TIMEOUT:")
        for sig, count in sorted(swallowed.items(), key=lambda kv: -kv[1])[:top]:
            print(f"  {count:6d}  {sig}")
            for test in result.get("swallowed_examples", {}).get(sig, [])[:1]:
                print(f"          e.g. {test}")
    hung_names = collections.Counter()
    for names in result.get("residual_hung_subtests", {}).values():
        hung_names.update(names)
    if hung_names:
        print()
        covered = len(result.get("residual_hung_subtests", {}))
        print(f"residual ids whose harness still reported: {covered} of "
              f"{len(result['residual_ids'])} — the subtests that never "
              f"finished (top {top}):")
        for name, count in hung_names.most_common(top):
            print(f"  {count:6d}  {name[:110]}")
            for test, names in result["residual_hung_subtests"].items():
                if name in names:
                    print(f"          e.g. {test}")
                    break
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

    # 8b. wptrunner's injected testdriver refusing to act: the element's
    #     `ownerDocument.defaultView` is falsy, which is BUG-622 seen from the
    #     harness side rather than from a test helper.
    start("TestRunnerManager-2", 1420, "/a/testdriver.html")
    out("503", 1430, "script error: JS runtime error: Error: Browsing context "
                     "for element was detached")
    end("TestRunnerManager-2", 1450, "/a/testdriver.html", "TIMEOUT", 503)

    # 9. the engine named the cause itself: a strict-mode helper assigning to
    #    `el.style`. Evidence beats everything the source could say.
    start("TestRunnerManager-2", 1500, "/a/style-assign.html")
    out("503", 1510, "Загружен скрипт: http://x/css/support/numeric-testcommon.js")
    out("503", 1520, "script error: JS runtime error: Cannot set property style "
                     "of #<_ctor> which has only a getter")
    end("TestRunnerManager-2", 1600, "/a/style-assign.html", "TIMEOUT", 503)

    # 10. an assertion that threw outside a test() body — a FAIL wearing a
    #     TIMEOUT, which is a different finding from an engine gap.
    start("TestRunnerManager-2", 1700, "/a/assert.html")
    out("503", 1710, "script error: JS runtime error: assert_equals: expected 1 but got 2")
    end("TestRunnerManager-2", 1800, "/a/assert.html", "TIMEOUT", 503)

    # 11. an exception no table knows: the weak stage keeps it and files the
    #     text in `swallowed_errors` for the next slice to pick up.
    start("TestRunnerManager-2", 1900, "/a/unknown-throw.html")
    out("503", 1910, "script error: JS runtime error: Frobnicate is not defined")
    end("TestRunnerManager-2", 2000, "/a/unknown-throw.html", "TIMEOUT", 503)

    # 12. an exception a *source marker* can explain (BUG-564 makes
    #     `document.fonts.ready` undefined, and the page prints the resulting
    #     TypeError). The named mechanism must win over "something threw".
    start("TestRunnerManager-2", 2100, "/a/fonts-throw.html")
    out("503", 2110, "script error: JS runtime error: Cannot read properties "
                     "of undefined (reading 'then')")
    end("TestRunnerManager-2", 2200, "/a/fonts-throw.html", "TIMEOUT", 503)

    # 13. a page measured to hang standalone (BUG-803's `2d-rotate-notref`).
    #     It prints an ordinary parse line and nothing an error table can use,
    #     so only the measured stage can name it.
    start("TestRunnerManager-2", 2300, "/css/css-transforms/2d-rotate-notref.html")
    out("504", 2310, "Распарсено: 12 DOM-узлов, 3 CSS-правил")
    end("TestRunnerManager-2", 2400, "/css/css-transforms/2d-rotate-notref.html",
        "TIMEOUT", 504)

    # 14. two silent pages whose *harness* still reported: the run recorded
    #     which subtests never finished. Neither prints anything an error
    #     table can use and neither has a source file on disk, so only the
    #     subtest stage can name them. `/d/nbc.html` splits 3/1 between two
    #     markers and is the majority rule's guard.
    start("TestRunnerManager-2", 2500, "/d/nbc.html")
    out("504", 2510, "Распарсено: 12 DOM-узлов, 3 CSS-правил")
    end("TestRunnerManager-2", 2600, "/d/nbc.html", "TIMEOUT", 504)

    start("TestRunnerManager-2", 2700, "/html/semantics/permission-element/pepc.tentative.html")
    out("504", 2710, "Распарсено: 5 DOM-узлов, 1 CSS-правил")
    end("TestRunnerManager-2", 2800, "/html/semantics/permission-element/pepc.tentative.html", "TIMEOUT", 504)

    with open(path, "w", encoding="utf-8") as handle:
        for event in events:
            handle.write(json.dumps(event, ensure_ascii=False) + "\n")
        handle.write('{"action": "test_end", "time": "1', )  # truncated tail


def _write_selftest_report(path):
    """Synthetic wptreport json — the partial subtest record of the same run.

    Only the fields the audit reads are written. `/d/nbc.html` is the shape
    that makes the majority rule matter: three of its four hung subtests are
    `<frame>`/`<object>`/`<embed>` and one is `<iframe>`, so the file belongs
    to the first marker even though the second also matches.
    """
    report = {"results": [
        {"test": "/d/nbc.html", "status": "TIMEOUT", "subtests": [
            {"name": "load nested browsing context <frame src>", "status": "TIMEOUT"},
            {"name": "load nested browsing context <object data>", "status": "TIMEOUT"},
            {"name": "load nested browsing context <embed src>", "status": "NOTRUN"},
            {"name": "load nested browsing context <iframe src>", "status": "TIMEOUT"},
            {"name": "resolving a relative url", "status": "PASS"},
        ]},
        {"test": "/html/semantics/permission-element/pepc.tentative.html", "status": "TIMEOUT", "subtests": [
            {"name": "Usermedia element display style validation", "status": "TIMEOUT"},
        ]},
        # A test that finished: its subtests must never reach the stage, which
        # only runs on TIMEOUTs the earlier stages could not claim.
        {"test": "/a/ok.html", "status": "OK", "subtests": [
            {"name": "load nested browsing context <frame src>", "status": "PASS"},
        ]},
        # The residual test, with a hung subtest no marker claims: it must stay
        # unclassified and its subtest name must reach `residual_hung_subtests`.
        {"test": "/a/clean.html", "status": "TIMEOUT", "subtests": [
            {"name": "an unattributed wait", "status": "TIMEOUT"},
        ]},
    ]}
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, ensure_ascii=False)


def selftest():
    """Assertions over the synthetic shard. Every one has been checked to fail
    if the rule it guards is removed."""
    failures = []

    def check(cond, msg):
        if not cond:
            failures.append(msg)

    with tempfile.TemporaryDirectory() as tmp:
        _write_selftest_shard(os.path.join(tmp, "synthetic.raw.jsonl"))
        _write_selftest_report(os.path.join(tmp, "synthetic.json"))
        result = audit(tmp, use_source=False)

        mech = result["mechanisms"]
        check(result["timeouts"] == 15,
              f"expected 15 TIMEOUTs (the OK test excluded), got {result['timeouts']}")
        check(result["statuses"].get("OK") == 1, "the OK test must still be counted")
        check(mech.get("https-body-truncated") == 1,
              f"TLS truncation must win over the missing global: {mech}")
        check(mech.get("helper-global-missing") is None,
              f"the downstream symptom must not get its own row: {mech}")
        check(mech.get("worker-importscripts") == 1,
              f"worker importScripts not attributed: {mech}")
        check(mech.get("defaultview-test-driver") == 1,
              f"testdriver's own detached-context error not attributed: {mech}")
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
        # Stage 1, slice 14: the error line names the API, so the mechanism is
        # owned by that API's bug and not by the generic "it threw" bucket.
        check(mech.get("inline-style-assign") == 1,
              f"`el.style = ...` TypeError not attributed: {mech}")
        check(mech.get("assert-swallowed") == 1,
              f"a thrown assertion is its own finding, not an engine gap: {mech}")
        # Stage 3: two noisy tests no table can name (`Frobnicate`, and the
        # fonts one as long as no source is read).
        check(mech.get("script-error-swallowed") == 2,
              f"unnamed exceptions must land in the weak bucket: {mech}")
        check(result["swallowed_errors"],
              "the weak bucket must keep the error texts as a work list")
        check(any("Frobnicate" in s for s in result["swallowed_errors"]),
              f"unknown error text not recorded: {result['swallowed_errors']}")
        check(all("assert_equals" not in s for s in result["swallowed_errors"]),
              "a text a mechanism already claims must not be re-listed as unknown")
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
        with open(os.path.join(tmp, "a", "fonts-throw.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>document.fonts.ready.then(() => done());</script>")
        with open(os.path.join(tmp, "a", "focus.window.js"), "w", encoding="utf-8") as handle:
            handle.write("addEventListener('load', () => { el.focus(); });")
        sourced = audit(tmp, root=tmp)
        check(sourced["mechanisms"].get("fonts-ready") == 2,
              f"source stage did not claim the silent test: {sourced['mechanisms']}")
        # The ordering rule of slice 14: the noisy fonts test printed a
        # TypeError, so the weak stage *could* have taken it — the named
        # mechanism must get it first.
        check("/a/fonts-throw.html" in sourced["examples"].get("fonts-ready", []),
              f"a source marker must outrank the generic exception bucket: "
              f"{sourced['examples'].get('script-error-swallowed')}")
        check(sourced["mechanisms"].get("script-error-swallowed") == 1,
              f"only the unnameable exception may stay in the weak bucket: "
              f"{sourced['mechanisms']}")
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
        # Stage 2, slice 14: an inline `<script type="module">` whose body is
        # one `import` plus one call. Neither `<script src>` nor `// META:`
        # points at the helper, so the marker inside it was unreachable.
        with open(os.path.join(tmp, "a", "module.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testharness.js"></script>\n'
                         '<script type="module">\n'
                         'import {runTests} from "./support/mod-helper.js";\n'
                         'runTests({});\n</script>')
        os.makedirs(os.path.join(tmp, "a", "support"), exist_ok=True)
        with open(os.path.join(tmp, "a", "support", "mod-helper.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("export function runTests() {\n"
                         "  document.fonts.ready.then(() => done());\n}")
        check(classify_source("/a/module.html", tmp, {}) == "fonts-ready",
              "a helper reached only through an ES import was not followed")
        check(classify_source("/a/module.html", tmp, {}, follow_helpers=False) is None,
              "--no-helpers must also switch off ES-import helpers")

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

        # Stage 2, slice 15. Three event families the engine never dispatches;
        # each was measured live by `verify_event_delivery_gaps.py` before the
        # marker was written.
        with open(os.path.join(tmp, "a", "transition.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const w = new EventWatcher(t, div, ['transitionend']);\n"
                         "return w.wait_for('transitionend');</script>")
        check(classify_source("/a/transition.html", tmp, {}) == "css-animation-events",
              "a transitionend wait was not claimed")
        with open(os.path.join(tmp, "a", "animation.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>el.addEventListener('animationstart', t.step_func_done());"
                         "</script>")
        check(classify_source("/a/animation.html", tmp, {}) == "css-animation-events",
              "an animationstart wait was not claimed")
        with open(os.path.join(tmp, "a", "smil.svg"), "w", encoding="utf-8") as handle:
            handle.write('<svg><rect><animate id="a" begin="0s" dur="5ms"/></rect>\n'
                         "<script>a.addEventListener('endEvent', () => t.done());</script>"
                         "</svg>")
        check(classify_source("/a/smil.svg", tmp, {}) == "smil-animation",
              "a SMIL endEvent wait was not claimed")
        with open(os.path.join(tmp, "a", "io.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>new IntersectionObserver(es => t.done())"
                         ".observe(document.body);</script>")
        check(classify_source("/a/io.html", tmp, {}) == "intersection-observer-initial",
              "an observe-and-wait IntersectionObserver test was not claimed")
        # Ordering: the IO marker sorts last of the three because an observer
        # is routine boilerplate in tests whose real wait is something else —
        # here a frame that is never loaded at all (BUG-480).
        with open(os.path.join(tmp, "a", "io-frame.html"), "w", encoding="utf-8") as handle:
            handle.write("<iframe src=child.html></iframe>\n"
                         "<script>iframe.onload = () => {};\n"
                         "new IntersectionObserver(es => t.done()).observe(el);</script>")
        check(classify_source("/a/io-frame.html", tmp, {}) == "iframe-no-nested-context",
              "an IntersectionObserver must not outrank the frame the test waits on")
        # A page that merely constructs the event type is not waiting for one:
        # `new TransitionEvent(...)` works today (the interface exists), so the
        # marker must key on a listener, not on the word.
        with open(os.path.join(tmp, "a", "anim-plain.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>el.style.animation = 'fade 1s';\n"
                         "assert_equals(getComputedStyle(el).animationName, 'fade');</script>")
        check(classify_source("/a/anim-plain.html", tmp, {}) is None,
              "a test that only reads animation style must not be claimed")

        # Stage 2, slice 16. An XML document loses three kinds of script to the
        # HTML tree builder; each shape was measured on its own page by
        # `verify_document_and_record_gaps.py` before the marker was written.
        with open(os.path.join(tmp, "a", "prefixed.svg"), "w", encoding="utf-8") as handle:
            handle.write('<?xml version="1.0"?>\n'
                         '<svg xmlns="http://www.w3.org/2000/svg" '
                         'xmlns:h="http://www.w3.org/1999/xhtml">\n'
                         '<h:script src="/resources/testharness.js"/></svg>')
        check(classify_source("/a/prefixed.svg", tmp, {}) == "xml-document-scripts-lost",
              "a prefixed <h:script> in an SVG document was not claimed")
        with open(os.path.join(tmp, "a", "selfclosed.xhtml"), "w", encoding="utf-8") as handle:
            handle.write('<html xmlns="http://www.w3.org/1999/xhtml"><head>\n'
                         '<script src="/resources/testharness.js"/>\n'
                         "</head><body><script>test(() => {});</script></body></html>")
        check(classify_source("/a/selfclosed.xhtml", tmp, {}) == "xml-document-scripts-lost",
              "a self-closing <script src/> in an XHTML document was not claimed")
        with open(os.path.join(tmp, "a", "cdata.xht"), "w", encoding="utf-8") as handle:
            handle.write('<html xmlns="http://www.w3.org/1999/xhtml"><body>\n'
                         "<script><![CDATA[\ntest(() => {});\n]]></script>\n"
                         "</body></html>")
        check(classify_source("/a/cdata.xht", tmp, {}) == "xml-document-scripts-lost",
              "a CDATA-wrapped inline script was not claimed")
        # The extension is the evidence: `wptserve` types a `.html` file as
        # `text/html`, where a conforming browser parses exactly the way Lumen
        # does — the same bytes are then not a defect at all.
        with open(os.path.join(tmp, "a", "cdata.html"), "w", encoding="utf-8") as handle:
            handle.write("<body>\n<script><![CDATA[\ntest(() => {});\n]]></script>\n</body>")
        check(classify_source("/a/cdata.html", tmp, {}) is None,
              "the same markup in an HTML file must not be claimed")
        # An XML document whose scripts survive HTML parsing is not this
        # mechanism — `/a/smil.svg` above is exactly that shape and must keep
        # its own marker.
        check(classify_source("/a/smil.svg", tmp, {}) == "smil-animation",
              "a plain <script> in an SVG document must not be claimed as lost")
        # Ordering: the harness never arriving outranks whatever the file also
        # waits for — but not the audio deadlock, which the parser triggers
        # whether or not any script survived.
        with open(os.path.join(tmp, "a", "xml-frame.svg"), "w", encoding="utf-8") as handle:
            handle.write('<svg xmlns="http://www.w3.org/2000/svg" '
                         'xmlns:h="http://www.w3.org/1999/xhtml">\n'
                         '<h:script src="/resources/testharness.js"/>\n'
                         "<h:script>document.fonts.ready.then(() => t.done());"
                         "</h:script></svg>")
        check(classify_source("/a/xml-frame.svg", tmp, {}) == "xml-document-scripts-lost",
              "a lost harness must outrank the wait the file never reaches")
        with open(os.path.join(tmp, "a", "xml-audio.xhtml"), "w", encoding="utf-8") as handle:
            handle.write('<html xmlns="http://www.w3.org/1999/xhtml"><body>\n'
                         '<audio src="/media/sine440.mp3"/>\n'
                         '<script src="/resources/testharness.js"/>\n'
                         "</body></html>")
        check(classify_source("/a/xml-audio.xhtml", tmp, {}) == "audio-src-deadlock",
              "the parser-driven audio deadlock must still sort first")

        # Stage 2, slice 16: a UTF-16 test file. Decoded as UTF-8 its markers
        # are separated by replacement bytes and nothing can ever match.
        with open(os.path.join(tmp, "a", "utf16.html"), "wb") as handle:
            handle.write("<script>document.fonts.ready.then(() => done());</script>"
                         .encode("utf-16"))
        check(classify_source("/a/utf16.html", tmp, {}) == "fonts-ready",
              "a UTF-16 source was not decoded")

        # Stage 2, slice 16: an ES-module helper with the `.mjs` extension.
        with open(os.path.join(tmp, "a", "mjs.html"), "w", encoding="utf-8") as handle:
            handle.write('<script type="module">\n'
                         'import {createIframe} from "./helpers.mjs";\n'
                         "promise_test(async t => { await createIframe(t); });\n"
                         "</script>")
        with open(os.path.join(tmp, "a", "helpers.mjs"), "w", encoding="utf-8") as handle:
            handle.write("export function createIframe(t) {\n"
                         "  const iframe = document.createElement('iframe');\n"
                         "  iframe.onload = () => {};\n}")
        check(classify_source("/a/mjs.html", tmp, {}) == "iframe-no-nested-context",
              "an .mjs helper was not followed")

        # Stage 2, slice 16: `?include=` selects the blocks that actually run,
        # so a marker from a sibling block must not be attributed to it.
        with open(os.path.join(tmp, "a", "subset.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="subset-helper.js"></script>')
        with open(os.path.join(tmp, "a", "subset-helper.js"), "w", encoding="utf-8") as handle:
            handle.write("subsetTestByKey('frames', async_test, function() {\n"
                         "  const iframe = document.createElement('iframe');\n"
                         "  iframe.onload = this.step_func_done(function() {});\n"
                         "});\n"
                         "subsetTestByKey('fonts', async_test, function() {\n"
                         "  document.fonts.ready.then(() => this.done());\n"
                         "});")
        check(classify_source("/a/subset.html?include=fonts", tmp, {}) == "fonts-ready",
              "the variant's own block was not the one read")
        # The load-bearing one: `fonts-ready` outranks the frame marker, so
        # reading the whole file would answer `fonts-ready` here too.
        check(classify_source("/a/subset.html?include=frames", tmp, {})
              == "iframe-no-nested-context",
              "a sibling block's marker was attributed to this variant")
        check(classify_source("/a/subset.html", tmp, {}) == "fonts-ready",
              "a file with no ?include= must still be read whole")

        # Stage 2, slice 17. Three more silent waits, each measured live
        # before the marker was written (`verify_layout_shift_and_peer_gaps.py`
        # for the first two, an instrumented `_handle_action` for the third).
        with open(os.path.join(tmp, "a", "cls.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>new PerformanceObserver(l => t.done())\n"
                         "  .observe({entryTypes: ['layout-shift']});\n"
                         "shifter.style.top = '160px';</script>")
        check(classify_source("/a/cls.html", tmp, {}) == "layout-shift-never-delivered",
              "an observe-and-shift layout-instability test was not claimed")
        # The category's own helper is the marker for the tests that never
        # name the entry type themselves.
        with open(os.path.join(tmp, "a", "cls-watcher.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="util.js"></script>\n'
                         "<script>const watcher = new ScoreWatcher;\n"
                         "promise_test(async () => { await watcher.promise; });</script>")
        check(classify_source("/a/cls-watcher.html", tmp, {})
              == "layout-shift-never-delivered",
              "a ScoreWatcher test was not claimed")
        # A `PerformanceObserver` on a type the engine *does* deliver is not
        # this mechanism — the marker must key on the entry type, not on the
        # observer.
        with open(os.path.join(tmp, "a", "perf-paint.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>new PerformanceObserver(l => t.done())\n"
                         "  .observe({entryTypes: ['paint']});</script>")
        check(classify_source("/a/perf-paint.html", tmp, {}) is None,
              "an observer on a delivered entry type must not be claimed")
        with open(os.path.join(tmp, "a", "rtc.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const pc1 = new RTCPeerConnection();\n"
                         "pc2.ondatachannel = t.step_func_done(() => {});\n"
                         "pc1.createDataChannel('x');</script>")
        check(classify_source("/a/rtc.html", tmp, {}) == "webrtc-no-remote-peer",
              "a two-peer RTCPeerConnection test was not claimed")
        with open(os.path.join(tmp, "a", "actions.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testdriver.js"></script>\n'
                         "<script>promise_test(async () => {\n"
                         "  await new test_driver.Actions().scroll(0, 0, 0, 50).send();\n"
                         "});</script>")
        check(classify_source("/a/actions.html", tmp, {})
              == "testdriver-action-unimplemented",
              "a test_driver.Actions() wait was not claimed")
        # `click()` is implemented by the executor, so a test using only that
        # one is not blocked by this gap.
        with open(os.path.join(tmp, "a", "click.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testdriver.js"></script>\n'
                         "<script>promise_test(async () => {\n"
                         "  await test_driver.click(document.getElementById('b'));\n"
                         "});</script>")
        check(classify_source("/a/click.html", tmp, {}) is None,
              "the implemented click action must not be blamed on BUG-810")
        # Ordering: the frame that is never loaded is the older, better
        # understood cause and keeps its tests (the shape of every
        # `pointerevents/*-in-iframe.html`).
        with open(os.path.join(tmp, "a", "actions-frame.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testdriver.js"></script>\n'
                         "<iframe src=child.html></iframe>\n"
                         "<script>iframe.onload = () => {};\n"
                         "new test_driver.Actions().scroll(0, 0, 0, 50).send();</script>")
        check(classify_source("/a/actions-frame.html", tmp, {})
              == "iframe-no-nested-context",
              "an unimplemented action must not outrank the frame the test waits on")

        # ── slice 18 ───────────────────────────────────────────────────────
        # The output half: the engine names the unresolved URL itself, and it
        # is the whole line the network layer prints for a relative XHR.
        check(classify(['fetch error: invalid url: invalid url: missing '
                        'scheme: "resources/status.py"'])
              == "relative-url-unresolved",
              "an unresolved relative URL was not claimed from the output")
        # ...but only for that shape. `Error::InvalidUrl`
        # (`crates/core/src/error.rs:33`) prints the same `invalid url:` head
        # for five different failures (`crates/core/src/url.rs:47,146,256,275,
        # 287`), and only the `missing scheme` one means "a relative URL
        # arrived unresolved" — an absolute URL the parser rejects is a
        # different finding and must not be filed under BUG-780.
        check(classify(['fetch error: invalid url: invalid url: empty host '
                        'in http://']) != "relative-url-unresolved",
              "a malformed absolute URL must not be read as an unresolved one")
        check(classify(['fetch error: invalid url: invalid url: invalid port: '
                        '"80x"']) != "relative-url-unresolved",
              "a bad port must not be read as an unresolved relative URL")
        # A scheme the engine does not speak is a different, older mechanism
        # and must keep its tests.
        check(classify(["network error: unsupported scheme: ftp"])
              == "unsupported-scheme",
              "an unsupported scheme must keep its own mechanism")
        # CSP: the wait is the marker, and it sits at the bottom of the source
        # table, so a test that merely mentions a policy is not claimed.
        with open(os.path.join(tmp, "a", "csp.sub.html"), "w", encoding="utf-8") as handle:
            handle.write('<meta http-equiv="Content-Security-Policy" '
                         'content="img-src \'none\'">\n'
                         "<script>async_test(t => {\n"
                         "  document.addEventListener('securitypolicyviolation',\n"
                         "    t.step_func_done(e => {}));\n"
                         "});</script>")
        check(classify_source("/a/csp.sub.html", tmp, {})
              == "csp-no-violation-event",
              "a securitypolicyviolation wait was not claimed")
        with open(os.path.join(tmp, "a", "csp-load.sub.html"), "w", encoding="utf-8") as handle:
            handle.write('<meta http-equiv="Content-Security-Policy" '
                         'content="img-src \'none\'">\n'
                         "<script>async_test(t => { window.onload = "
                         "t.step_func_done(() => {}); });</script>")
        check(classify_source("/a/csp-load.sub.html", tmp, {})
              != "csp-no-violation-event",
              "a CSP test that never waits for the event must not be claimed")
        # The worker table is matched against the *worker script*, not the page.
        os.makedirs(os.path.join(tmp, "workers", "support"), exist_ok=True)
        with open(os.path.join(tmp, "workers", "support", "nav.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("postMessage(navigator.platform);\n")
        with open(os.path.join(tmp, "workers", "nav.htm"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  const w = new Worker('support/nav.js');\n"
                         "  w.onmessage = t.step_func_done(e => {});\n"
                         "});</script>")
        check(classify_source("/workers/nav.htm", tmp, {})
              == "worker-navigator-missing",
              "a worker reading navigator was not claimed")
        # The page's own `navigator` is not evidence about the worker: this is
        # the `WorkerNavigator_appName.htm` shape, whose page half compares the
        # worker's answer against its own `navigator.appName`.
        with open(os.path.join(tmp, "workers", "support", "plain.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("onmessage = function (e) { postMessage(e.data); };\n")
        with open(os.path.join(tmp, "workers", "page-nav.htm"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  const w = new Worker('support/plain.js');\n"
                         "  w.onmessage = t.step_func_done(e => {\n"
                         "    assert_equals(e.data, navigator.appName); });\n"
                         "  w.postMessage(1);\n"
                         "});</script>")
        check(classify_source("/workers/page-nav.htm", tmp, {}) is None,
              "the page's own navigator must not be read as the worker's")
        # A worker-scoped generated id is its own worker script.
        with open(os.path.join(tmp, "workers", "timers.worker.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("setTimeout(function () { postMessage('late'); }, 10);\n")
        check(classify_source("/workers/timers.worker.html", tmp, {})
              == "worker-timers-not-driven",
              "a .worker.js test's own timers were not claimed")
        # ...and its window-scoped sibling is not: `.any.html` runs where
        # timers are driven normally.
        with open(os.path.join(tmp, "workers", "timers.any.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("setTimeout(function () { done(); }, 10);\n")
        check(classify_source("/workers/timers.any.html", tmp, {}) is None,
              "the window-scoped variant must not be blamed on worker timers")
        # Precedence inside the worker table: a missing global stops the script
        # before any timer it also arms.
        with open(os.path.join(tmp, "workers", "both.worker.js"), "w",
                  encoding="utf-8") as handle:
            handle.write("setTimeout(function () { postMessage(navigator.platform); }, 10);\n")
        check(classify_source("/workers/both.worker.html", tmp, {})
              == "worker-navigator-missing",
              "a missing global must outrank the timer it is read from")
        # The `error`-at-the-Worker gap is a page-source marker (both halves
        # required), and one half alone is not enough.
        with open(os.path.join(tmp, "workers", "err.htm"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  const w = new Worker('support/throw.js');\n"
                         "  w.onerror = t.step_func_done(e => {});\n"
                         "});</script>")
        check(classify_source("/workers/err.htm", tmp, {})
              == "worker-no-error-event",
              "a wait on Worker.onerror was not claimed")
        with open(os.path.join(tmp, "workers", "err-only.htm"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  window.addEventListener('error', t.step_func_done(e => {}));\n"
                         "});</script>")
        check(classify_source("/workers/err-only.htm", tmp, {})
              != "worker-no-error-event",
              "an error listener without a Worker must not be claimed")

        # ── slice 19 ───────────────────────────────────────────────────────
        # A programmatic page scroll moves the page but dispatches no `scroll`
        # event: both halves are required, because the wait alone is every
        # scroll test and the scroll alone is half of `css-scroll-*`.
        os.makedirs(os.path.join(tmp, "css", "css-scroll-anchoring"), exist_ok=True)
        anchoring = os.path.join(tmp, "css", "css-scroll-anchoring")
        with open(os.path.join(anchoring, "page.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async t => {\n"
                         "  window.scrollTo(0, 300);\n"
                         "  await new Promise(r => window.addEventListener('scroll', r));\n"
                         "});</script>")
        check(classify_source("/css/css-scroll-anchoring/page.html", tmp, {})
              == "page-scroll-no-scroll-event",
              "a page scroll followed by a scroll-event wait was not claimed")
        with open(os.path.join(anchoring, "bare-onscroll.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const t = async_test('x');\n"
                         "onscroll = t.step_func_done(() => {});\n"
                         "window.scrollBy(0, -200);</script>")
        check(classify_source("/css/css-scroll-anchoring/bare-onscroll.html", tmp, {})
              == "page-scroll-no-scroll-event",
              "the bare `onscroll =` shape was not claimed")
        # An element scroll DOES fire `scroll` (`fire_element_scroll` runs for
        # `scrollTo` and `scrollTop=` alike), so an element-only test must not
        # be claimed by the page marker.
        with open(os.path.join(anchoring, "element.html"), "w", encoding="utf-8") as handle:
            handle.write("<div id=s></div><script>promise_test(async t => {\n"
                         "  s.addEventListener('scroll', () => {});\n"
                         "  s.scrollTo(0, 250);\n"
                         "});</script>")
        check(classify_source("/css/css-scroll-anchoring/element.html", tmp, {})
              != "page-scroll-no-scroll-event",
              "an element scroll must not be blamed on the page-scroll gap")
        # ...and a page scroll with no wait at all is not this mechanism.
        with open(os.path.join(anchoring, "no-wait.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async t => {\n"
                         "  window.scrollTo(0, 150);\n"
                         "  assert_equals(window.scrollY, 150);\n"
                         "});</script>")
        check(classify_source("/css/css-scroll-anchoring/no-wait.html", tmp, {}) is None,
              "a page scroll without an event wait must stay unclassified")
        # `scrollend` is dispatched by nothing at all, so the mention alone is
        # the marker — and it outranks the page-scroll one, because an element
        # scroll that fires `scroll` correctly still never fires `scrollend`.
        with open(os.path.join(anchoring, "scrollend.html"), "w", encoding="utf-8") as handle:
            handle.write("<div id=s></div><script>promise_test(async t => {\n"
                         "  const p = new Promise(r => s.addEventListener('scrollend', r));\n"
                         "  s.scrollTo(0, 250);\n"
                         "  await p;\n"
                         "});</script>")
        check(classify_source("/css/css-scroll-anchoring/scrollend.html", tmp, {})
              == "scrollend-never-fired",
              "a scrollend wait was not claimed")
        with open(os.path.join(anchoring, "both.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async t => {\n"
                         "  window.addEventListener('scroll', () => {});\n"
                         "  const p = new Promise(r => window.addEventListener('scrollend', r));\n"
                         "  window.scrollTo(0, 300);\n"
                         "  await p;\n"
                         "});</script>")
        check(classify_source("/css/css-scroll-anchoring/both.html", tmp, {})
              == "scrollend-never-fired",
              "scrollend must outrank the page-scroll marker")
        # Streams: both halves again — a construction plus one of the measured
        # error/close/cancel paths.
        os.makedirs(os.path.join(tmp, "streams"), exist_ok=True)
        with open(os.path.join(tmp, "streams", "closed.any.js"), "w", encoding="utf-8") as handle:
            handle.write("promise_test(t => {\n"
                         "  const ws = new WritableStream({ close() { throw 1; } });\n"
                         "  const writer = ws.getWriter();\n"
                         "  return promise_rejects_exactly(t, 1, writer.closed);\n"
                         "});\n")
        check(classify_source("/streams/closed.any.html", tmp, {})
              == "streams-promise-unsettled",
              "a writer.closed wait was not claimed")
        # A test that merely *builds* a stream and reads it on the happy path
        # is not this mechanism — the happy path works (measured).
        with open(os.path.join(tmp, "streams", "happy.any.js"), "w", encoding="utf-8") as handle:
            handle.write("promise_test(async () => {\n"
                         "  const rs = new ReadableStream({ start(c) { c.enqueue(1); c.close(); } });\n"
                         "  const reader = rs.getReader();\n"
                         "  assert_equals((await reader.read()).value, 1);\n"
                         "});\n")
        check(classify_source("/streams/happy.any.html", tmp, {}) is None,
              "a happy-path stream test must stay unclassified")
        # A window-level error/rejection wait, and the `unreached_func` guard
        # that must not be read as one.
        os.makedirs(os.path.join(tmp, "webappapis"), exist_ok=True)
        with open(os.path.join(tmp, "webappapis", "rejection.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  window.addEventListener('unhandledrejection', t.step_func_done(e => {}));\n"
                         "  Promise.reject(new Error('x'));\n"
                         "});</script>")
        check(classify_source("/webappapis/rejection.html", tmp, {})
              == "window-error-event-never-fired",
              "an unhandledrejection wait was not claimed")
        with open(os.path.join(tmp, "webappapis", "guard.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function () {\n"
                         "  window.onerror = this.unreached_func('no error expected');\n"
                         "  const s = document.createElement('script');\n"
                         "  s.onerror = this.step_func_done(() => {});\n"
                         "  document.head.appendChild(s);\n"
                         "});</script>")
        check(classify_source("/webappapis/guard.html", tmp, {})
              != "window-error-event-never-fired",
              "an unreached_func guard must not be read as the test's wait")
        # Ordering: the window-error marker is last, so a named cause above it
        # keeps the test even when the page also listens for `error`.
        with open(os.path.join(tmp, "webappapis", "with-font.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(t => {\n"
                         "  window.addEventListener('error', t.step_func(e => {}));\n"
                         "  document.fonts.ready.then(t.step_func_done(() => {}));\n"
                         "});</script>")
        check(classify_source("/webappapis/with-font.html", tmp, {}) == "fonts-ready",
              "an older named cause must outrank the window-error marker")

        # ── slice 20 ──────────────────────────────────────────────────────
        # BUG-826: a link hint is never fetched, so both the `load` wait and
        # the "poll the server until the preload shows up" shape hang.
        os.makedirs(os.path.join(tmp, "preload"), exist_ok=True)
        with open(os.path.join(tmp, "preload", "created.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async () => {\n"
                         "  const link = document.createElement('link');\n"
                         "  link.rel = 'preload';\n"
                         "  link.as = 'script';\n"
                         "  await new Promise(r => { link.onload = r; });\n"
                         "});</script>")
        check(classify_source("/preload/created.html", tmp, {})
              == "preload-hint-never-fetched",
              "a script-created rel=preload link was not claimed")
        with open(os.path.join(tmp, "preload", "markup.html"), "w", encoding="utf-8") as handle:
            handle.write('<link rel="modulepreload" href="m.js" onload="t.done()">')
        check(classify_source("/preload/markup.html", tmp, {})
              == "preload-hint-never-fetched",
              "a parser-written rel=modulepreload link was not claimed")
        # The `connection-allowlist` shape: nothing in the test mentions
        # `load` at all, it polls the server in a `while (true)` until the
        # preloaded URL arrives there.
        with open(os.path.join(tmp, "preload", "poll.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async () => {\n"
                         "  const result = await nextValueFromServer(key);\n"
                         "});</script>")
        check(classify_source("/preload/poll.html", tmp, {})
              == "preload-hint-never-fetched",
              "a server-poll wait for a preload was not claimed")
        # The control that separates the hint from the element: a
        # `rel=stylesheet` link created from script does load and does fire
        # (BUG-722), so it must not be claimed by this marker.
        with open(os.path.join(tmp, "preload", "stylesheet.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async () => {\n"
                         "  const link = document.createElement('link');\n"
                         "  link.rel = 'stylesheet';\n"
                         "  await new Promise(r => { link.onload = r; });\n"
                         "});</script>")
        check(classify_source("/preload/stylesheet.html", tmp, {}) is None,
              "a script-created rel=stylesheet link must stay unclassified")

        # BUG-827: the wait is for a mutation record about a parser insertion.
        # Only the *call* counts — `test-render-blocking.js` defines
        # `nodeInserted()` for all nine files of that directory and only three
        # await it.
        os.makedirs(os.path.join(tmp, "renderblocking"), exist_ok=True)
        with open(os.path.join(tmp, "renderblocking", "await.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_setup(async () => {\n"
                         "  let script = await nodeInserted(document.head, n => n.id === 'script');\n"
                         "});</script>\n"
                         '<script id="script" src="dummy.js"></script>')
        check(classify_source("/renderblocking/await.html", tmp, {})
              == "mutation-record-parser-insert",
              "an await on a parser-insertion mutation record was not claimed")
        with open(os.path.join(tmp, "renderblocking", "define.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>function nodeInserted(parentNode, predicate) {\n"
                         "  return new Promise(resolve => {\n"
                         "    new MutationObserver(() => {}).observe(parentNode, {childList: true});\n"
                         "  });\n"
                         "}</script>")
        check(classify_source("/renderblocking/define.html", tmp, {}) is None,
              "merely defining the helper must not be read as awaiting it")

        # BUG-804, third shape: the `load` wait goes through `LoadObserver`,
        # so nothing in the test itself mentions `load`.
        with open(os.path.join(tmp, "renderblocking", "observer.html"), "w", encoding="utf-8") as handle:
            handle.write('<script id="s" src="dummy.js" blocking="render"></script>\n'
                         "<script>const el = document.getElementById('s');\n"
                         "test_render_blocking(el, () => assert_true(true), 'x');</script>")
        check(classify_source("/renderblocking/observer.html", tmp, {})
              == "resource-no-load-event",
              "a LoadObserver wait on a parser-inserted script was not claimed")
        # …but the helper alone is not the marker: with nothing silent under
        # observation there is no reason to claim the file.
        with open(os.path.join(tmp, "renderblocking", "window.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>test_render_blocking(() => assert_true(true), 'x');</script>")
        check(classify_source("/renderblocking/window.html", tmp, {}) is None,
              "test_render_blocking with no silent element must stay unclassified")

        # BUG-828: Web Audio renders silence and never reports `ended`.
        os.makedirs(os.path.join(tmp, "webaudio"), exist_ok=True)
        with open(os.path.join(tmp, "webaudio", "render.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async () => {\n"
                         "  const ctx = new OfflineAudioContext(1, 44100, 44100);\n"
                         "  const buf = await ctx.startRendering();\n"
                         "});</script>")
        check(classify_source("/webaudio/render.html", tmp, {}) == "offline-audio-silent",
              "an OfflineAudioContext render was not claimed")
        # Ordering: a WebRTC test that also builds an `AudioContext` belongs to
        # the older, better-understood peer-connection cause.
        with open(os.path.join(tmp, "webaudio", "rtc-audio.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const ctx = new AudioContext();\n"
                         "const pc = new RTCPeerConnection();\n"
                         "pc.ondatachannel = t.step_func_done(() => {});</script>")
        check(classify_source("/webaudio/rtc-audio.html", tmp, {}) == "webrtc-no-remote-peer",
              "an AudioContext in a WebRTC test must not outrank the peer cause")

        # BUG-568: a written `<script>` never runs; written markup does.
        os.makedirs(os.path.join(tmp, "dmi"), exist_ok=True)
        with open(os.path.join(tmp, "dmi", "write-script.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "t.step(function () {\n"
                         "  document.write('<scr' + 'ipt>t.done();</scr' + 'ipt>');\n"
                         "});</script>")
        check(classify_source("/dmi/write-script.html", tmp, {})
              == "document-write-script-inert",
              "a document.write of a <script> was not claimed")
        with open(os.path.join(tmp, "dmi", "write-markup.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>document.write('<p id=x>hi</p>');\n"
                         "test(() => assert_true(!!document.getElementById('x')));</script>")
        check(classify_source("/dmi/write-markup.html", tmp, {}) is None,
              "document.write of plain markup works and must stay unclassified")

        # BUG-831: a *string* handed to `setTimeout`/`setInterval` is dropped.
        os.makedirs(os.path.join(tmp, "timers"), exist_ok=True)
        with open(os.path.join(tmp, "timers", "string.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  setTimeout(\"t.done();\", 0);\n"
                         "});</script>")
        check(classify_source("/timers/string.html", tmp, {}) == "timer-string-handler",
              "a string handed to setTimeout was not claimed")
        # The `string-compilation-*` shape: the string never appears next to
        # the call, only the evaluator table does.
        with open(os.path.join(tmp, "timers", "evaluators.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>const evaluators = {\n"
                         "  'eval': src => eval(src),\n"
                         "  'setTimeout': src => setTimeout(src, 0),\n"
                         "};\npromise_test(async () => { await evaluate(); });</script>")
        check(classify_source("/timers/evaluators.html", tmp, {}) == "timer-string-handler",
              "an evaluator table with a setTimeout entry was not claimed")
        # The control: an ordinary function handler is the overwhelmingly
        # common shape and must never be claimed.
        with open(os.path.join(tmp, "timers", "function.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  setTimeout(t.step_func_done(function () {}), 0);\n"
                         "});</script>")
        check(classify_source("/timers/function.html", tmp, {}) is None,
              "a function handler must stay unclassified")

        # BUG-832: the listener is attached after the assignment that fires the
        # (synchronously dispatched) event.
        os.makedirs(os.path.join(tmp, "fragid"), exist_ok=True)
        with open(os.path.join(tmp, "fragid", "late.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "location.hash = 'x';\n"
                         "addEventListener('hashchange', t.step_func_done(function () {}));\n"
                         "</script>")
        check(classify_source("/fragid/late.html", tmp, {}) == "hashchange-listener-too-late",
              "a hashchange listener attached after the assignment was not claimed")
        # The same two lines in the other order are a working page.
        with open(os.path.join(tmp, "fragid", "early.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "addEventListener('hashchange', t.step_func_done(function () {}));\n"
                         "location.hash = 'x';\n"
                         "</script>")
        check(classify_source("/fragid/early.html", tmp, {}) is None,
              "a hashchange listener attached first must stay unclassified")

        # BUG-717: the modern `postMessage` overloads are dropped silently.
        os.makedirs(os.path.join(tmp, "wm"), exist_ok=True)
        with open(os.path.join(tmp, "wm", "options.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  onmessage = t.step_func_done(function () {});\n"
                         "  window.postMessage('x', {targetOrigin: '*'});\n"
                         "});</script>")
        check(classify_source("/wm/options.html", tmp, {}) == "postmessage-options-dropped",
              "a WindowPostMessageOptions post was not claimed")
        with open(os.path.join(tmp, "wm", "one-arg.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  addEventListener('message', t.step_func_done(function () {}));\n"
                         "  window.postMessage('x');\n"
                         "});</script>")
        check(classify_source("/wm/one-arg.html", tmp, {}) == "postmessage-options-dropped",
              "a one-argument postMessage was not claimed")
        # The legacy form works, so a test that uses it is not this mechanism.
        with open(os.path.join(tmp, "wm", "legacy.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  onmessage = t.step_func_done(function () {});\n"
                         "  window.postMessage('x', '*');\n"
                         "});</script>")
        check(classify_source("/wm/legacy.html", tmp, {}) is None,
              "a legacy postMessage must stay unclassified")

        # BUG-480, fifth stage: the wait lives in the *child* document.
        os.makedirs(os.path.join(tmp, "frames", "resources"), exist_ok=True)
        with open(os.path.join(tmp, "frames", "resources", "init.htm"), "w", encoding="utf-8") as handle:
            handle.write("<script>parent.done_from_child();</script>")
        with open(os.path.join(tmp, "frames", "parent.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "function done_from_child() { t.done(); }</script>\n"
                         '<iframe src="resources/init.htm"></iframe>')
        check(classify_source("/frames/parent.html", tmp, {})
              == "iframe-child-callback-never-runs",
              "a callback made from the subframe was not claimed")
        # `setAttribute('src', ...)` is how two of the history tests load
        # their child, so the stage has to see that shape too.
        with open(os.path.join(tmp, "frames", "setattr.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "var f = document.createElement('iframe');\n"
                         "f.setAttribute('src', 'resources/init.htm');\n"
                         "document.body.appendChild(f);</script>")
        check(classify_source("/frames/setattr.html", tmp, {})
              == "iframe-child-callback-never-runs",
              "a setAttribute-loaded subframe was not claimed")
        # A child that talks to nobody is not evidence of a wait.
        with open(os.path.join(tmp, "frames", "resources", "quiet.htm"), "w", encoding="utf-8") as handle:
            handle.write("<p>nothing</p>")
        with open(os.path.join(tmp, "frames", "quiet.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>test(function () { assert_true(true); });</script>\n"
                         '<iframe src="resources/quiet.htm"></iframe>')
        check(classify_source("/frames/quiet.html", tmp, {}) is None,
              "a subframe that calls nothing must stay unclassified")
        # A cross-origin or `javascript:` frame is a different mechanism and is
        # skipped rather than guessed at. This one documents intent rather than
        # guarding behaviour: dropping the `"://"` test leaves the outcome
        # unchanged, because the resolved path does not exist on disk either
        # way (measured as an equivalent mutant, WPT-RUN-6 slice 21).
        with open(os.path.join(tmp, "frames", "remote.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "function done_from_child() { t.done(); }</script>\n"
                         '<iframe src="http://example.test/x.htm"></iframe>')
        check(classify_source("/frames/remote.html", tmp, {}) is None,
              "a cross-origin subframe must not be read from disk")

        # ── slice 22 ────────────────────────────────────────────────────────
        # BUG-842: a transaction held open by a self-rearming request. The
        # helper form and the hand-written one both count, and the helper's
        # own *definition* must not: `resources/support.js` is included by
        # every IndexedDB test, so matching the joined text claimed 16 ids
        # instead of 5 while this slice was being written.
        os.makedirs(os.path.join(tmp, "idb", "resources"), exist_ok=True)
        with open(os.path.join(tmp, "idb", "resources", "support.js"), "w", encoding="utf-8") as handle:
            handle.write("function keep_alive(tx, store_name) {\n"
                         "  let keepSpinning = true;\n"
                         "  function spin() {\n"
                         "    if (!keepSpinning) return;\n"
                         "    tx.objectStore(store_name).get(0).onsuccess = spin;\n"
                         "  }\n"
                         "  spin();\n"
                         "}\n")
        with open(os.path.join(tmp, "idb", "keepalive.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="resources/support.js"></script>\n'
                         "<script>async_test(function (t) {\n"
                         "  var tx = db.transaction('store', 'readonly');\n"
                         "  var release = keep_alive(tx, 'store');\n"
                         "  indexedDB.open('d', 1);\n"
                         "});</script>")
        check(classify_source("/idb/keepalive.html", tmp, {}) == "idb-keep-alive-spin",
              "a keep_alive() spin was not claimed")
        with open(os.path.join(tmp, "idb", "handspin.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var open = indexedDB.open('d', 1);\n"
                         "  function doGet() {\n"
                         "    var request = tx.objectStore('store').get('key');\n"
                         "    request.onsuccess = t.step_func(function () { doGet(); });\n"
                         "  }\n"
                         "  doGet();\n"
                         "});</script>")
        check(classify_source("/idb/handspin.html", tmp, {}) == "idb-keep-alive-spin",
              "a hand-written request spin was not claimed")
        # The other way to write the same loop: the handler *is* the function,
        # passed by reference rather than called from a wrapper.
        with open(os.path.join(tmp, "idb", "refspin.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var open = indexedDB.open('d', 1);\n"
                         "  function spin() {\n"
                         "    tx.objectStore('store').get(0).onsuccess = spin;\n"
                         "  }\n"
                         "  spin();\n"
                         "});</script>")
        check(classify_source("/idb/refspin.html", tmp, {}) == "idb-keep-alive-spin",
              "a by-reference request spin was not claimed")
        # The helper alone is not evidence: a test that merely includes
        # support.js and issues one request is a different (or no) mechanism.
        with open(os.path.join(tmp, "idb", "plain.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="resources/support.js"></script>\n'
                         "<script>async_test(function (t) {\n"
                         "  var open = indexedDB.open('d', 1);\n"
                         "  open.onsuccess = t.step_func_done(function () {});\n"
                         "});</script>")
        check(classify_source("/idb/plain.html", tmp, {}) is None,
              "including support.js must not count as a spin")

        # BUG-843: the connection queue. `'versionchange'` as a bare string is
        # also the *mode* of an upgrade transaction, so only the event forms
        # count.
        with open(os.path.join(tmp, "idb", "queue.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var r = indexedDB.open('d', 2);\n"
                         "  r.onblocked = t.step_func(function () {});\n"
                         "  db.onversionchange = t.step_func_done(function () {});\n"
                         "});</script>")
        check(classify_source("/idb/queue.html", tmp, {}) == "idb-no-connection-queue",
              "a versionchange/blocked wait was not claimed")
        with open(os.path.join(tmp, "idb", "mode.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>test(function () {\n"
                         "  var r = indexedDB.open('d', 1);\n"
                         "  assert_equals(tx.mode, 'versionchange');\n"
                         "});</script>")
        check(classify_source("/idb/mode.html", tmp, {}) is None,
              "the transaction *mode* string must not be read as a queue wait")

        # BUG-841: a transaction that never gets a request never commits. The
        # window has to end at the next transaction — `transaction-lifetime-
        # empty.any.js` opens three in a row and issues a request on the
        # first one's store below all three.
        with open(os.path.join(tmp, "idb", "empty.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var db = open.result;\n"
                         "  var tx = db.transaction('store', 'readonly');\n"
                         "  tx.oncomplete = t.step_func_done(function () {});\n"
                         "  indexedDB.cmp(1, 2);\n"
                         "});</script>")
        check(classify_source("/idb/empty.html", tmp, {}) == "idb-empty-transaction",
              "a transaction awaited without any request was not claimed")
        with open(os.path.join(tmp, "idb", "three.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var tx1 = db.transaction('store', 'readwrite');\n"
                         "  var store = tx1.objectStore('store');\n"
                         "  var tx2 = db.transaction('store', 'readonly');\n"
                         "  tx2.oncomplete = t.step_func(function () {});\n"
                         "  var rq = store.put('b', 2);\n"
                         "  indexedDB.cmp(1, 2);\n"
                         "});</script>")
        check(classify_source("/idb/three.html", tmp, {}) == "idb-empty-transaction",
              "an empty transaction next to a busy one was not claimed")
        # A transaction with a request in it completes correctly (measured),
        # so it must not be claimed.
        with open(os.path.join(tmp, "idb", "busy.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  var tx = db.transaction('store', 'readwrite');\n"
                         "  tx.oncomplete = t.step_func_done(function () {});\n"
                         "  tx.objectStore('store').put('a', 1);\n"
                         "  indexedDB.cmp(1, 2);\n"
                         "});</script>")
        check(classify_source("/idb/busy.html", tmp, {}) is None,
              "a transaction with a request must stay unclassified")

        # BUG-839: a `resource` entry (or the callback's options) is awaited.
        os.makedirs(os.path.join(tmp, "rt"), exist_ok=True)
        with open(os.path.join(tmp, "rt", "observe.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  new PerformanceObserver(t.step_func_done(function () {}))\n"
                         "      .observe({entryTypes: ['resource']});\n"
                         "  fetch('x.js');\n"
                         "});</script>")
        check(classify_source("/rt/observe.html", tmp, {})
              == "resource-timing-entry-never-delivered",
              "an observer waiting for a resource entry was not claimed")
        with open(os.path.join(tmp, "rt", "dropped.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  new PerformanceObserver(t.step_func(function (l, o, options) {\n"
                         "    assert_equals(options.droppedEntriesCount, 0);\n"
                         "  })).observe({type: 'mark'});\n"
                         "});</script>")
        check(classify_source("/rt/dropped.html", tmp, {})
              == "resource-timing-entry-never-delivered",
              "a droppedEntriesCount read was not claimed")
        # `mark`/`measure` are delivered, so an observer over them is not this
        # mechanism.
        with open(os.path.join(tmp, "rt", "marks.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>async_test(function (t) {\n"
                         "  new PerformanceObserver(t.step_func_done(function () {}))\n"
                         "      .observe({entryTypes: ['mark', 'measure']});\n"
                         "  performance.mark('m');\n"
                         "});</script>")
        check(classify_source("/rt/marks.html", tmp, {}) is None,
              "a mark/measure observer must stay unclassified")

        # BUG-844: every WPT EventSource stream is a Content-Length response,
        # which the engine never treats as ended.
        os.makedirs(os.path.join(tmp, "es"), exist_ok=True)
        with open(os.path.join(tmp, "es", "retry.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>var t = async_test();\n"
                         "var source = new EventSource('resources/message.py');\n"
                         "source.onopen = function () { t.done(); };\n"
                         "</script>")
        check(classify_source("/es/retry.html", tmp, {}) == "eventsource-no-reconnect",
              "an EventSource open/message wait was not claimed")
        # Constructing one and asserting synchronously is a FAIL, not a hang.
        with open(os.path.join(tmp, "es", "ctor.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>test(function () {\n"
                         "  var source = new EventSource('resources/message.py');\n"
                         "  assert_equals(source.readyState, 0);\n"
                         "});</script>")
        check(classify_source("/es/ctor.html", tmp, {}) is None,
              "an EventSource test with no event wait must stay unclassified")

        # BUG-846: a compression stream read before the writable side closes.
        os.makedirs(os.path.join(tmp, "cs"), exist_ok=True)
        with open(os.path.join(tmp, "cs", "read.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async function () {\n"
                         "  const ds = new DecompressionStream('deflate');\n"
                         "  const reader = ds.readable.getReader();\n"
                         "  ds.writable.getWriter().write(chunk);\n"
                         "  const { value } = await reader.read();\n"
                         "});</script>")
        check(classify_source("/cs/read.html", tmp, {})
              == "compression-stream-read-before-close",
              "a read before close was not claimed")
        with open(os.path.join(tmp, "cs", "closed.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>promise_test(async function () {\n"
                         "  const ds = new DecompressionStream('deflate');\n"
                         "  const writer = ds.writable.getWriter();\n"
                         "  writer.write(chunk);\n"
                         "  writer.close();\n"
                         "  const { value } = await ds.readable.getReader().read();\n"
                         "});</script>")
        check(classify_source("/cs/closed.html", tmp, {}) is None,
              "a stream whose writer is closed must stay unclassified")

        # BUG-847: a delay that does not fit a signed 32-bit int.
        os.makedirs(os.path.join(tmp, "tm"), exist_ok=True)
        with open(os.path.join(tmp, "tm", "long.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>setup({single_test: true});\n"
                         "setTimeout(done, Math.pow(2, 32));\n"
                         "setTimeout(assert_unreached, 100);</script>")
        check(classify_source("/tm/long.html", tmp, {}) == "timer-overflow-delay",
              "an overflowing timer delay was not claimed")
        with open(os.path.join(tmp, "tm", "short.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>setup({single_test: true});\n"
                         "setTimeout(done, 3000);</script>")
        check(classify_source("/tm/short.html", tmp, {}) is None,
              "an ordinary delay must stay unclassified")

        # BUG-848/BUG-825: a subresource induced through an element the
        # request collector does not know.
        os.makedirs(os.path.join(tmp, "fm"), exist_ok=True)
        with open(os.path.join(tmp, "fm", "poster.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>function induceRequest(url) {\n"
                         "  const video = document.createElement('video');\n"
                         "  video.setAttribute('poster', url);\n"
                         "  return new Promise((resolve) => { video.onload = resolve; });\n"
                         "}\n"
                         "promise_test(() => induceRequest('x.png'));</script>")
        check(classify_source("/fm/poster.html", tmp, {})
              == "element-subresource-never-requested",
              "a poster-induced request was not claimed")
        with open(os.path.join(tmp, "fm", "svg.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>function induceRequest(url) {\n"
                         "  const image = document.createElementNS(\n"
                         "    'http://www.w3.org/2000/svg', 'image');\n"
                         "  image.setAttribute('href', url);\n"
                         "}\n"
                         "promise_test(() => induceRequest('x.svg'));</script>")
        check(classify_source("/fm/svg.html", tmp, {})
              == "element-subresource-never-requested",
              "an SVG <image> built through createElementNS was not claimed")
        # `fetch()`-induced requests work, so the same harness around a fetch
        # is not this mechanism.
        with open(os.path.join(tmp, "fm", "fetch.html"), "w", encoding="utf-8") as handle:
            handle.write("<script>function induceRequest(url) {\n"
                         "  return fetch(url);\n"
                         "}\n"
                         "promise_test(() => induceRequest('x.json'));</script>")
        check(classify_source("/fm/fetch.html", tmp, {}) is None,
              "a fetch-induced request must stay unclassified")

        # Stage 1b (WPT-RUN-6 slice 23): the ids measured to hang standalone.
        # The table is the probe's, so the first two checks guard the join
        # rather than the audit — a mechanism renamed in one file and not the
        # other would otherwise silently report an unowned key.
        check(MEASURED_HANGS, "the measured-hang table is empty — probe missing?")
        check(set(MEASURED_HANGS.values()) == set(MEASURED_REFS),
              f"probe mechanisms and MEASURED_REFS disagree: "
              f"{sorted(set(MEASURED_HANGS.values()) ^ set(MEASURED_REFS))}")
        check(all(test_id.startswith("/") for test_id in MEASURED_HANGS),
              "a measured id is not a manifest id (must start with '/')")
        check(classify_measured("/css/css-transforms/2d-rotate-notref.html")
              == "svg-transform-loop",
              "a measured id was not claimed")
        check(classify_measured("/css/css-transforms/2d-rotate-notref.html?x=1")
              == "svg-transform-loop",
              "a measured id with a query string was not matched on its path")
        check(classify_measured("/a/clean.html") is None,
              "an unmeasured id must not be claimed")
        check(classify_measured(None) is None, "a missing id must not be claimed")
        # End to end, both ways round: the stage must claim the test in the
        # synthetic shard, and `--no-measured` must hand it back to the residual.
        check(mech.get("svg-transform-loop") == 1,
              f"the measured stage did not claim its test: {mech}")
        unmeasured = audit(tmp, use_source=False, use_measured=False)
        check(unmeasured["mechanisms"].get("svg-transform-loop") is None,
              "--no-measured must not classify the measured test")
        check(unmeasured["mechanisms"].get(UNCLASSIFIED) == 2,
              f"--no-measured must leave the measured test unclassified: "
              f"{unmeasured['mechanisms']}")

        # Stage 2, WPT-RUN-6 slice 24: a `setup({single_test})` page whose
        # body runs from `<body onload>`. All three markers are required —
        # the two negatives below are the reason.
        with open(os.path.join(tmp, "a", "shape.html"), "w", encoding="utf-8") as handle:
            handle.write('<script src="/resources/testharness.js"></script>\n'
                         '<script>setup({ single_test: true });\n'
                         'function check() { assert_equals(1, 2); done(); }</script>\n'
                         '<body onload="check();">x</body>')
        check(classify_source("/a/shape.html", tmp, {}) == "single-test-load-handler-throw",
              "the load-handler marker did not claim its page")
        with open(os.path.join(tmp, "a", "shape-async.html"), "w", encoding="utf-8") as handle:
            handle.write('<script>setup({ single_test: true });\n'
                         'window.addEventListener("load", () => done());</script>')
        check(classify_source("/a/shape-async.html", tmp, {}) is None,
              "a page without the `<body onload>` attribute must not match")
        with open(os.path.join(tmp, "a", "shape-plain.html"), "w", encoding="utf-8") as handle:
            handle.write('<script>function check() { done(); }</script>\n'
                         '<body onload="check();">x</body>')
        check(classify_source("/a/shape-plain.html", tmp, {}) is None,
              "an `onload` attribute + `done()` without `single_test` must not match")
        with open(os.path.join(tmp, "a", "shape-nodone.html"), "w", encoding="utf-8") as handle:
            handle.write('<script>setup({ single_test: true });\n'
                         'function check() { assert_equals(1, 2); }</script>\n'
                         '<body onload="check();">x</body>')
        check(classify_source("/a/shape-nodone.html", tmp, {}) is None,
              "a single_test page that never calls done() is a different shape")

        # Stage 1c (WPT-RUN-6 slice 24): the run's own partial harness report.
        check(mech.get("nbc-element-never-loads") == 1,
              f"the subtest stage did not claim the nested-context test: {mech}")
        check(mech.get("permission-element-unimplemented") == 1,
              f"the subtest stage did not claim the permission-element test: {mech}")
        # The majority rule, not table order: the iframe marker matches the
        # same file (one subtest) and must lose to the three-subtest one.
        check(mech.get("iframe-no-nested-context") is None,
              f"a minority marker won the file: {mech}")
        check(classify_subtests("/d/nbc.html", [
            ("load nested browsing context <iframe src>", "TIMEOUT"),
            ("load nested browsing context <frame src>", "PASS"),
        ]) == "iframe-no-nested-context",
              "with only the iframe subtest hung, the iframe marker must win")
        # Table order must not be able to stand in for the majority rule: here
        # the marker that sorts *first* matches one hung subtest and the one
        # that sorts second matches three.
        check(classify_subtests("/d/nbc.html", [
            ("load nested browsing context <frame src>", "TIMEOUT"),
            ("load nested browsing context <iframe src>", "TIMEOUT"),
            ("same-origin <iframe>", "TIMEOUT"),
            ("cross-origin <iframe>", "NOTRUN"),
        ]) == "iframe-no-nested-context",
              "the majority of hung subtests must decide, not table order")
        # A subtest that PASSed is not evidence of a wait, and a test with no
        # hung subtest at all must fall through to the later stages.
        check(classify_subtests("/d/nbc.html", [
            ("load nested browsing context <frame src>", "PASS")]) is None,
              "a passing subtest must not attribute anything")
        check(classify_subtests("/d/nbc.html", []) is None,
              "a test with no subtest record must fall through")
        # The `test=` filter: the same subtest name outside its directory is
        # not the same mechanism.
        check(classify_subtests("/elsewhere/auto-001.html",
                                [("contain-intrinsic-size: auto", "TIMEOUT")]) is None,
              "an id-scoped marker fired outside its directory")
        # The residual keeps the evidence that did not attribute it.
        check(result["residual_hung_subtests"].get("/a/clean.html")
              == ["an unattributed wait"],
              f"residual subtest evidence not recorded: "
              f"{result['residual_hung_subtests']}")
        check("/a/ok.html" not in result["residual_hung_subtests"],
              "a test that finished must not appear in the residual evidence")
        nosub = audit(tmp, use_source=False, use_subtests=False)
        check(nosub["mechanisms"].get("nbc-element-never-loads") is None,
              "--no-subtests must not classify by the harness report")
        check(nosub["mechanisms"].get(UNCLASSIFIED) == 3,
              f"--no-subtests must hand both tests back to the residual: "
              f"{nosub['mechanisms']}")
        check(not nosub["residual_hung_subtests"],
              "--no-subtests must not collect subtest evidence either")
        # A run whose wptreport is missing entirely disables the stage rather
        # than failing the audit.
        check(read_subtest_report(os.path.join(tmp, "nowhere")) == {},
              "a missing wptreport must read as no evidence")

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
    parser.add_argument("--no-measured", action="store_true",
                        help="skip the measured-hang stage (the ids "
                             "verify_layout_hangs.py ran standalone)")
    parser.add_argument("--no-subtests", action="store_true",
                        help="skip the partial-harness-report stage (the "
                             "hung subtest names wptrunner recorded)")
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
                   follow_helpers=not args.no_helpers,
                   use_measured=not args.no_measured,
                   use_subtests=not args.no_subtests)
    print_report(result, top=args.top, examples=args.examples)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, ensure_ascii=False, indent=1)
        print(f"\nwrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
