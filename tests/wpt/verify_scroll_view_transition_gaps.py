#!/usr/bin/env python3
"""WPT-RUN-6 slice 31: scroll side-effects and the view-transition surface
`timeout_audit.py` cannot classify by source alone.

Slice 30 left 68 `unclassified` ids and flagged its own residual: no
dominant directory, `canvas-with-padding.html` (cause not found — it passes
standalone), and `css/css-view-transitions` (3 ids). Reading the 68 by hand
(rather than by directory) surfaces a second coherent family the same size as
the view-transition one: a page that scrolls — anchoring, `behavior: smooth`,
`.focus()`, `ResizeObserver` on a scrollbar-bearing box, a scroll timeline —
and waits for the engine to say something back:

    /css/css-scroll-anchoring/reading-scroll-forces-anchoring.html
    /css/cssom-view/background-change-during-smooth-scroll.html
    /focus/scroll-matches-focus.html
    /resize-observer/scrollbars-2.html
    /scroll-animations/scroll-timelines/scroll-timeline-snapshot-elementsFromPoint.html
    /css/css-view-transitions/elements-at-point.html
    /css/css-view-transitions/pseudo-computed-style-stays-in-sync-with-new-element.html

Six more of the 68 needed no live probe at all — grepping the shim source
answered the question directly, and are filed straight into `timeout_audit.py`
without a variant here. A seventh, `window.length`/`window[0]`
(`iframe-marginwidth-marginheight.html`), looked the same way at first —
`crates/js/src/shim/*.js` really does hardcode `window.length = 0` — but that
grep only covered the `WEB_API_SHIM*` consts, not `frame_bridge.rs`'s own
separate `rt.eval`, which installs a *lazy* `window.length`/`window[n]`/
`window[name]` accessor the moment a frame registers (BUG-480 срез 3,
2026-08-23, already on `main`). A direct check (an iframe, load, read
`window.length`) shows `1`, `window[0].document` a live object, and
`window[0] === window.myframe` — confirmed live, not filed. Kept here as the
concrete instance of the "per-feature shim" gotcha in `CLAUDE.md`: a single
grep root is not the whole story, check every place a comment mentions before
trusting a zero:

`currentCSSZoom`/CSS zoom OM has zero references (`resize-observer/zoom.html`),
`scroll-initial-target` has zero references (`scroll-initial-target-shadow-
dom.tentative.html`), `pagereveal` has zero references (`pagereveal-no-view-
transition.html`), no shim file ever reads `trustedTypes`/`defaultPolicy`
(`trusted-types/Window-setTimeout-setInterval.html`), the Notification
constructor's own comment says a denied permission is "silent drop" where the
spec fires `error` (`notifications/constructor-non-secure.html`), and
`longtask`/`long-animation-frame` are deliberately absent from
`_PERF_SUPPORTED_ENTRY_TYPES` because no entry constructor produces them
(`longtask-timing/supported-longtask-types.window.html`,
`long-animation-frame/loaf-toJSON.html`).

`po-mark-measure.any.html` is included as a control: `Performance.prototype.
mark`/`measure` push straight into `_perf_observer_notify`, which looks
correct by inspection, so the variant either confirms the read (no engine
gap, drop it as an artifact of the corpus) or finds what a static read
missed.

Same harness as slice 30 and the reasons recorded in `CLAUDE.md`: one browser
process per page, served over http, evidence read off the browser's own
stderr, a 500 ms tick so "alive and heard nothing" is separable from "died".

## Results (2026-09-01, dev-release, Linux, `main` = `b52550bb6`)

Three of the seven variants confirm a real, standalone-reproducible gap and
are filed in `timeout_audit.py` as `_exact_id_marker`-keyed mechanisms:

- `scroll-anchor-read` → `scroll-position-read-async` (BUG-949):
  `scrollY` reads the pre-scroll value through a `raf` and a `setTimeout(…,
  0)` and only picks up the real number ~500 ms later — `scrollTo()` queues
  the move rather than applying it before returning.
- `focus-scroll-into-view` → `focus-no-scroll-into-view` (BUG-560, not a new
  number — same root cause): no `scroll` marker at all in 2.5 s and
  `scrollY` stays `0`. First pass here was wrong — the variant's CSS dropped
  the real test's `:focus` rule, so the probed element never actually moved
  off-screen on focus and the "gap" proved nothing. Fixed variant (the
  `:focus` rule restored, matching `scroll-matches-focus.html` exactly)
  reproduces the same silence: `.focus()`'s `scrollIntoView()` call runs
  synchronously and reads pre-pump layout, before the `:focus`-driven style
  change BUG-560 already tracks has applied — so there is nothing to scroll
  to yet.
- `scroll-timeline-elementsfrompoint` → `scroll-timeline-not-driven`
  (BUG-950, filed as a ДОРАБОТКА against the existing `P2-scrolldriven`
  `ROADMAP.md` task, not a fresh defect — `scroll_timeline.rs`'s own doc
  comment names this exact wiring "Phase 1", not yet done): `backgroundColor`
  reads the same unanimated value before and after the scroll —
  `animation-timeline: scroll(self)` never drives the computed style.

Four variants came back clean — read as evidence the corresponding id is
*not* explained by this family, not filed as anything:

- `smooth-scroll-completion` — no freeze. The original hypothesis (a
  7500×7500px scrolled child hangs the render/event loop outright, ticks
  never resuming) does **not** reproduce: five repeated runs at 6/8/12/20 s
  all show `setInterval` ticking at the expected ~500 ms cadence straight
  through and past the scroll, `polls-taken = 1` (the "smooth" scroll lands
  in a single jump — a real but separate defect, not a hang). Do not re-file
  this without a fresh, more specific reproduction.
- `resize-observer-scrollbar-initial` — `ResizeObserver` delivers a first
  observation for an ordinary scrollbar-bearing box exactly as spec'd
  (`ro-delivered`/`ro-ever-fired = true`). The existing `resize-observer-no-
  initial` mechanism stays narrowed to `contain-intrinsic-size`; it does not
  generalize to `resize-observer/scrollbars-2.html`.
- `view-transition-elements-at-point` — `startViewTransition` exists, the
  callback runs synchronously, and `elementsFromPoint` inside it resolves to
  owning elements (`LI,UL,DIV,BODY,HTML,#document`, no `::` pseudo tags).
- `view-transition-pseudo-computed-style` — `ready`/`updateCallbackDone`
  both settle and the pseudo computed style read does not throw.

`po-mark-measure` (control) also came back clean — all four entries delivered
in one batch — confirming the static read and ruling out that path as an
artifact.

The four ids behind the clean variants stay in `timeout_audit.py`'s
`unclassified` bucket; whatever makes them TIMEOUT in a full corpus run is
something this slice's hypotheses did not name.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_scroll_view_transition_gaps.py
        [--binary target/dev-release/lumen] [--seconds 8] [--variant NAME]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import http.server
import os
import re
import socket
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-31 probe: __NAME__</title>
<body>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

REPORT = """
<script>
function rep(label, fn) {
  try {
    var v = fn();
    console.log("PROBE " + label + " = " + v);
  } catch (e) {
    console.log("PROBE " + label + " THREW " + (e && e.name ? e.name + ": " : "") +
                (e && e.message ? e.message : e));
  }
}
</script>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, load"),

    # `reading-scroll-forces-anchoring.html`'s exact shape: scroll to a fixed
    # offset, grow an element above the viewport, then read `scrollY` — CSS
    # Scroll Anchoring §3 says the read must force the pending adjustment
    # first, so the value must reflect the new height.
    "scroll-anchor-read": ("""
<style>body { height: 1000px; background: teal; } div { height: 100px; background: navy; }</style>
<div id=block1>abc</div>
<div id=block2>def</div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  scrollTo(0, 150);
  rep("scrollY-immediately-after-scrollTo", function () { return String(scrollY); });
  requestAnimationFrame(function () {
    rep("scrollY-after-raf", function () { return String(scrollY); });
    setTimeout(function () {
      rep("scrollY-before-grow", function () { return String(scrollY); });
      document.getElementById("block1").style.height = "200px";
      rep("scrollY-after-grow", function () { return String(scrollY); });
      setTimeout(function () {
        rep("scrollY-after-second-timeout", function () { return String(scrollY); });
        console.log("PROBE anchor-done");
      }, 500);
    }, 0);
  });
});
</script>
""", "scrollY reflects the +100px growth (250) without a second scroll"),

    # `background-change-during-smooth-scroll.html`'s exact shape: does a
    # `behavior: smooth` `scrollTo` ever actually reach its target, polled
    # across rAFs rather than asserted once.
    # Reproduces the real test's exact structure (`background-change-during-
    # smooth-scroll.html`): background flips to transparent one rAF into the
    # smooth scroll, then an UNBOUNDED `requestAnimationFrame` loop polls
    # `scrollTop` with no escape hatch — if it never lands on exactly 6000,
    # this loop is the hang. `polls-taken` has no cap here on purpose.
    "smooth-scroll-completion": ("""
<style>
#c { width: 200px; height: 200px; overflow: scroll; background: white; }
#inner { width: 7500px; height: 7500px; }
</style>
<div id=c><div id=inner></div></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var c = document.getElementById("c");
  requestAnimationFrame(function () {
    c.scrollTo({behavior: "smooth", top: 6000});
    requestAnimationFrame(function () {
      c.style.background = "transparent";
      var polls = 0;
      function poll() {
        polls++;
        if (c.scrollTop === 6000) {
          rep("final-scrollTop", function () { return String(c.scrollTop); });
          rep("polls-taken", function () { return String(polls); });
          console.log("PROBE smooth-done");
          return;
        }
        if (polls % 20 === 0) {
          console.log("PROBE still-polling " + polls + " scrollTop=" + c.scrollTop);
        }
        requestAnimationFrame(poll);
      }
      requestAnimationFrame(poll);
    });
  });
});
</script>
""", "scrollTop reaches 6000 and the unbounded rAF poll loop exits"),

    # `scroll-matches-focus.html`'s exact shape (fixed after slice 31's first
    # pass wrongly dropped the `:focus` rule, see the module docstring
    # "correction" note): `#focusable` sits at (0, 0) — already in view — and
    # only the `:focus` rule un-pins it back to its static in-flow position,
    # BETWEEN the two 200vh paddings and off-screen. The element only needs
    # scrolling to AFTER `:focus` starts matching, so a version without that
    # rule can never observe a real gap here — the element never moves.
    "focus-scroll-into-view": ("""
<style>
body { margin: 0; }
.padding { height: 200vh; background: purple; }
#focusable { z-index: 0; position: absolute; top: 0; left: 0; }
#focusable:focus { z-index: 1; left: auto; top: auto; }
</style>
<div class=padding></div>
<div id=focusable tabindex=0>I am focusable</div>
<div class=padding></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var el = document.getElementById("focusable");
  window.addEventListener("scroll", function () {
    rep("activeElement-is-focusable", function () { return String(document.activeElement === el); });
    rep("matches-focus", function () { return String(el.matches(":focus")); });
    console.log("PROBE scroll-on-focus");
  }, { once: true });
  el.focus();
  setTimeout(function () {
    rep("late-scrollY", function () { return String(scrollY); });
    console.log("PROBE focusscroll-done");
  }, 2500);
});
</script>
""", "scroll event fires after focus(); :focus already matches by then"),

    # `resize-observer/scrollbars-2.html`: does `ResizeObserver` deliver an
    # initial observation at all for an ordinary scrollable box (the existing
    # `resize-observer-no-initial` mechanism is narrowed to
    # `contain-intrinsic-size` — this checks whether the gap is general).
    "resize-observer-scrollbar-initial": ("""
<style>
#sc { width: 100px; height: 100px; padding: 30px; border: 10px solid blue; overflow: scroll; }
</style>
<div id=sc></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var sc = document.getElementById("sc");
  var got = false;
  var observer = new ResizeObserver(function (entries) {
    got = true;
    rep("contentBoxSize-present", function () { return String(!!entries[0].contentBoxSize); });
    console.log("PROBE ro-delivered");
  });
  observer.observe(sc);
  setTimeout(function () {
    rep("ro-ever-fired", function () { return String(got); });
    console.log("PROBE ro-done");
  }, 3000);
});
</script>
""", "ResizeObserver's callback fires at least once for a static box"),

    # `scroll-timeline-snapshot-elementsFromPoint.html`: is `animation-
    # timeline: scroll(self)` honoured at all (background-color driven by
    # scroll position), and does `elementsFromPoint()` leave it alone.
    "scroll-timeline-elementsfrompoint": ("""
<style>
@keyframes anim { from { background-color: green; } to { background-color: red; } }
#scroller {
  width: 200px; height: 200px; overflow: auto;
  animation-name: anim; animation-duration: 10s; animation-timeline: scroll(self);
}
#filler { height: 400px; }
</style>
<div id=scroller><div id=filler></div></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  requestAnimationFrame(function () { requestAnimationFrame(function () {
    var scroller = document.getElementById("scroller");
    scroller.scrollTop = 200;
    rep("bg-before-efp", function () { return getComputedStyle(scroller).backgroundColor; });
    rep("elementsFromPoint-typeof", function () { return typeof document.elementsFromPoint; });
    if (document.elementsFromPoint) document.elementsFromPoint(10, 10);
    rep("bg-after-efp", function () { return getComputedStyle(scroller).backgroundColor; });
    console.log("PROBE scrolltimeline-done");
  }); });
});
</script>
""", "scroll(self) drives background-color; elementsFromPoint doesn't snapshot it"),

    # `elements-at-point.html`: does `document.startViewTransition` exist,
    # does its callback run, and does `elementsFromPoint` inside it resolve
    # pseudo-elements to their owning element (no `::` tag names in the list).
    "view-transition-elements-at-point": ("""
<div><ul style="view-transition-name: list1">
  <li id=target>One</li><li>Two</li>
</ul></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("startViewTransition-typeof", function () { return typeof document.startViewTransition; });
  if (typeof document.startViewTransition !== "function") {
    console.log("PROBE vtefp-no-api");
    return;
  }
  var target = document.getElementById("target");
  var rect = target.getBoundingClientRect();
  var transition = document.startViewTransition(function () {
    var list = document.elementsFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    rep("efp-tags", function () { return list.map(function (e) { return e.tagName; }).join(","); });
  });
  transition.finished.then(function () { console.log("PROBE vtefp-finished"); },
                            function (e) { console.log("PROBE vtefp-rejected " + e); });
  setTimeout(function () { console.log("PROBE vtefp-done"); }, 3000);
});
</script>
""", "startViewTransition exists; callback runs synchronously; no '::' tags"),

    # `pseudo-computed-style-stays-in-sync-with-new-element.html`: does
    # `getComputedStyle(el, '::view-transition-new(name)')` return anything,
    # and does `transition.ready`/`updateCallbackDone` ever settle.
    "view-transition-pseudo-computed-style": ("""
<style>
#first { width: 100px; height: 100px; background: blue; view-transition-name: target; contain: paint; }
</style>
<div id=first></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("startViewTransition-typeof", function () { return typeof document.startViewTransition; });
  if (typeof document.startViewTransition !== "function") {
    console.log("PROBE vtpseudo-no-api");
    return;
  }
  var transition = document.startViewTransition();
  transition.updateCallbackDone.then(function () { console.log("PROBE updateCallbackDone"); },
                                      function (e) { console.log("PROBE updateCallbackDone-rejected " + e); });
  transition.ready.then(function () {
    console.log("PROBE ready");
    rep("pseudo-computed-style", function () {
      var cs = getComputedStyle(document.documentElement, "::view-transition-new(target)");
      return cs ? String(cs.objectViewBox) : String(cs);
    });
  }, function (e) { console.log("PROBE ready-rejected " + e); });
  transition.finished.then(function () { console.log("PROBE vtpseudo-finished"); },
                            function (e) { console.log("PROBE vtpseudo-rejected " + e); });
  setTimeout(function () { console.log("PROBE vtpseudo-done"); }, 3000);
});
</script>
""", "ready/updateCallbackDone settle; pseudo computed style is readable"),

    # Control for `po-mark-measure.any.html`: the mark/measure delivery path
    # reads correct by inspection (`Performance.prototype.mark`/`measure` push
    # straight into `_perf_observer_notify`) — this either confirms that or
    # finds what a static read missed.
    "po-mark-measure": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var seen = [];
  var observer = new PerformanceObserver(function (list) {
    seen = seen.concat(list.getEntries().map(function (e) { return e.entryType + ":" + e.name; }));
    console.log("PROBE po-callback " + seen.length);
  });
  observer.observe({entryTypes: ["mark", "measure"]});
  performance.mark("mark1");
  performance.mark("mark2");
  performance.measure("measure1");
  performance.measure("measure2");
  setTimeout(function () {
    rep("seen-entries", function () { return seen.join(","); });
    console.log("PROBE pomarkmeasure-done");
  }, 2000);
});
</script>
""", "observer callback fires with all four entries, in some batching"),
}

_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds):
    log_path = os.path.join(REPO, ".tmp", f"svt-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".svt-{name}.html"
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/{page}"],
            stdout=subprocess.DEVNULL, stderr=log, text=True)
        try:
            time.sleep(seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    with open(log_path, encoding="utf-8", errors="replace") as log:
        text = log.read()
    ticks = len(_TICK_RE.findall(text))
    markers = []
    seen_markers = set()
    dropped = 0
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker.startswith("tick ") or marker in seen_markers:
            continue
        if len(markers) >= _MAX_MARKERS:
            dropped += 1
            continue
        seen_markers.add(marker)
        markers.append(marker)
    if dropped:
        markers.append(f"[+{dropped} more distinct markers, not shown]")
    for err in dict.fromkeys(_ERROR_RE.findall(text)):
        markers.append(f"[engine] {err.strip()}")
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=8.0)
    parser.add_argument("--variant", action="append", default=None)
    args = parser.parse_args()

    wanted = args.variant or list(VARIANTS)
    unknown = [name for name in wanted if name not in VARIANTS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    http_port, shutdown = _serve(HERE)
    written = []
    for name in wanted:
        path = os.path.join(HERE, f".svt-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)

    try:
        print(f"{'variant':32s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:32s} {ticks:5d}  {expected:62s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
    finally:
        shutdown()
        for path in written:
            try:
                os.remove(path)
            except OSError:
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
