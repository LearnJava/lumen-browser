#!/usr/bin/env python3
"""WPT-RUN-6 slice 28: auxiliary browsing contexts, `javascript:` URLs,
same-document history traversal and the `targetOrigin` forms of
`window.postMessage`.

The residual of slice 27 is 133 ids and has no dominant directory left, but it
still has a dominant *question*: what happens when a page stops talking to
itself. Thirty-odd of those ids open a second window (`open()`, `<a
target=_blank>`), close one (`window.close()`), run script through a URL
instead of an element (`javascript:`), step backwards through their own
session history (`history.back()` / `go(-1)` / `location.hash`), or post a
message to themselves through a `targetOrigin` that is spelled as a URL rather
than as `'*'`. Each of those is a *wait* — `popstate`, `load`, `message`,
`storage`, `unload` — so a mechanism that never produces the event can only
show up as a TIMEOUT, which is exactly what the run recorded.

What the probe separates, per variant:

* `open()` (`cross-origin-top-navigation-*.window.html`, 5 ids, and
  `noreferrer-null-opener.html`) — whether an auxiliary context is created at
  all. Decided on the probe's own server, not in the page: BUG-826 means the
  browser log is not evidence that a document was fetched, so "the child page
  was never requested" is the only sound reading of "no window".
* `window.close()` (`prompt-and-unload-script-closeable.html`) — whether
  `beforeunload`/`unload` fire on a script-closeable context. BUG-834 already
  records that neither fires on an ordinary navigation; the `close()` path is
  a different call site and is measured separately.
* `javascript:` URLs (`iframe_javascript_url_*.html`, the two
  `xhr/open-url-javascript-window*.htm`, `to-javascript-url-frame-src.html`)
  in the four places a test can put one: `<iframe src>` written by the parser,
  `iframe.src =` from script, `location.href =`, and `open()`.
* Same-document traversal (`the-history-interface/005.html`,
  `back-pushstate-back-history-state.html`,
  `anchor-fragment-history-back-on-click.html`) — `popstate` after
  `history.back()` / `go(-1)` following `pushState`, in both listener forms
  including the `<body onpopstate>` content attribute test 005 needs.
  BUG-835 measured the *cross-document* back (it freezes the document); the
  same-document one is a different path and is unmeasured.
* `postMessage` `targetOrigin` (`webmessaging/with{,out}-ports/00{2,6}.html`,
  `with-options/*`, 6 ids) — the seven spellings those tests use, all of them
  a self-post, reporting `e.data`/`e.origin` per form. BUG-717 says only the
  literal `'*'` is honoured; which of the URL forms and dictionary forms drop
  the message silently is what decides whether those ids belong to it.
* `storage` events across windows (`noreferrer-null-opener.html`), `document.open`
  singleton replacement (`document.open-03.html`), frame re-parenting
  (`change_parentage.html`) and the canvas surfaces of the residual
  (`filter`, SVG-image tainting, `OffscreenCanvas`).

Same harness as slices 15/17-22/24/25/26/27 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died", and a variant per hazard so a page that
freezes (BUG-835, BUG-856) cannot hide the measurements of its neighbours.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_window_history_jsurl_gaps.py
        [--binary target/dev-release/lumen] [--seconds 8] [--variant NAME]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import collections
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

#: Paths the probe server was asked for, with the request method. A document
#: that never appears here was never fetched, whatever the page or the
#: browser's own log says (BUG-826) — this is how "no auxiliary window" is
#: told from "a window that loaded and stayed quiet".
SERVED = []
_SERVED_LOCK = threading.Lock()

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-28 probe: __NAME__</title>
<body __BODYATTR__>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: Content attributes the parser must put on `<body>` for a variant. Test 005
#: of `the-history-interface` asserts specifically that `<body onpopstate>`
#: registers a listener, and an attribute set later from script is a different
#: code path from one the parser reflected.
BODY_ATTRS = {
    "hist-popstate": 'onpopstate="console.log(\'PROBE body-attr-popstate\')"',
}

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("vwjh-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, timeout, fetch-ok, load"),

    # `cross-origin-top-navigation-*.window.html` (5 ids) all start with
    # `const w = open(...)` and then wait for a `message` from the opened
    # document. Nothing downstream of that can run if no context is created,
    # and the server is the only witness that says so.
    "win-open": ("""
<script>
window.addEventListener("message", function (e) {
  console.log("PROBE parent-heard data=" + e.data + " origin=" + e.origin);
});
var w = null;
try {
  w = open("vwjh-child.html?from=open", "vwjhchild");
  console.log("PROBE open-returned type=" + (typeof w) + " null=" + (w === null));
} catch (e) {
  console.log("PROBE open-threw " + e);
}
try {
  console.log("PROBE open-props closed=" + (w && w.closed) +
              " name=" + (w && w.name) +
              " haspm=" + (w && typeof w.postMessage));
} catch (e) {
  console.log("PROBE open-props-threw " + e);
}
setTimeout(function () {
  try {
    w.postMessage("ping-from-parent", "*");
    console.log("PROBE parent-posted");
  } catch (e) { console.log("PROBE parent-post-threw " + e); }
}, 1500);
setTimeout(function () {
  try {
    w.close();
    console.log("PROBE child-close-called closed=" + w.closed);
  } catch (e) { console.log("PROBE child-close-threw " + e); }
}, 3000);
</script>
""", "open-returned + server GET of vwjh-child.html + parent-heard"),

    # `noreferrer-null-opener.html`: an `<a target=_blank rel=noreferrer>` that
    # is clicked and then *removed*, with the opened document reporting
    # `window.opener` through a `storage` event. Three separate mechanisms in
    # one id, so the probe prints each.
    "win-anchor-target": ("""
<a id=lnk target="_blank" rel="noreferrer" href="vwjh-child.html?from=anchor">child</a>
<script>
window.addEventListener("storage", function (e) {
  console.log("PROBE parent-storage key=" + e.key + " new=" + e.newValue);
});
window.addEventListener("message", function (e) {
  console.log("PROBE parent-heard data=" + e.data);
});
var lnk = document.getElementById("lnk");
console.log("PROBE anchor-props target=" + lnk.target + " rel=" + lnk.rel);
lnk.click();
console.log("PROBE anchor-clicked");
setTimeout(function () {
  try { localStorage.setItem("vwjh-parent", "written-later"); } catch (e) {}
}, 2500);
</script>
""", "server GET of vwjh-child.html?from=anchor + parent-storage"),

    # `prompt-and-unload-script-closeable.html` waits for `beforeunload` then
    # `unload`, both triggered by `window.close()`. Whether this top-level
    # context is script-closeable at all is itself part of the answer, so
    # `window.closed` is printed before and after.
    "win-close": ("""
<script>
window.onbeforeunload = function () { console.log("PROBE beforeunload"); };
window.onunload = function () { console.log("PROBE unload"); };
window.addEventListener("pagehide", function () { console.log("PROBE pagehide"); });
window.addEventListener("load", function () {
  console.log("PROBE before-close closed=" + window.closed +
              " hasclose=" + (typeof window.close));
  try {
    window.close();
    console.log("PROBE close-returned closed=" + window.closed);
  } catch (e) { console.log("PROBE close-threw " + e); }
  setTimeout(function () {
    console.log("PROBE after-close closed=" + window.closed);
  }, 500);
});
</script>
""", "beforeunload, unload, close-returned"),

    # `iframe_javascript_url_initial_insertion.html` /
    # `iframe_javascript_url_not_about_blank.html`: a parser-written
    # `javascript:` src must run and fire exactly one `load`, and re-assigning
    # `src` must run it a second time.
    "jsurl-iframe": ("""
<script>window.jsUrlRan = 0;</script>
<iframe id=fr src="javascript:(function(){ parent.jsUrlRan++; parent.console.log('PROBE jsurl-iframe-ran ' + parent.jsUrlRan); })()"
        onload="console.log('PROBE jsurl-iframe-load ran=' + window.jsUrlRan)"></iframe>
<script>
window.addEventListener("load", function () {
  var fr = document.getElementById("fr");
  console.log("PROBE jsurl-iframe-state ran=" + window.jsUrlRan +
              " src=" + String(fr.src).slice(0, 30) +
              " cw=" + (typeof fr.contentWindow));
  try {
    fr.src = fr.src + ";";
    console.log("PROBE jsurl-iframe-reassigned");
  } catch (e) { console.log("PROBE jsurl-iframe-reassign-threw " + e); }
  setTimeout(function () {
    console.log("PROBE jsurl-iframe-final ran=" + window.jsUrlRan);
  }, 400);
});
</script>
""", "jsurl-iframe-ran 1, jsurl-iframe-load, final ran=2"),

    # The other three places a `javascript:` URL appears in the residual:
    # `location.href =` (CSP's `to-javascript-url-frame-src.html` navigates a
    # frame that way), an `<a href="javascript:">` click, and `open()`.
    "jsurl-nav": ("""
<a id=lnk href="javascript:console.log('PROBE jsurl-anchor-ran')">go</a>
<script>
window.addEventListener("load", function () {
  document.getElementById("lnk").click();
  console.log("PROBE jsurl-anchor-clicked");
  try {
    location.href = "javascript:console.log('PROBE jsurl-location-ran')";
    console.log("PROBE jsurl-location-assigned href=" + String(location.href).slice(0, 40));
  } catch (e) { console.log("PROBE jsurl-location-threw " + e); }
  try {
    var w = open("javascript:console.log('PROBE jsurl-open-ran')");
    console.log("PROBE jsurl-open-returned null=" + (w === null));
  } catch (e) { console.log("PROBE jsurl-open-threw " + e); }
  setTimeout(function () { console.log("PROBE jsurl-nav-alive href=" +
                                       String(location.href).slice(-24)); }, 800);
});
</script>
""", "jsurl-anchor-ran, jsurl-location-ran, jsurl-open-ran"),

    # `the-history-interface/005.html`: two `pushState`s then `history.go(-1)`
    # twice, with the listener installed once as a `<body onpopstate>` content
    # attribute (BODY_ATTRS) and once as `window.onpopstate`.
    "hist-popstate": ("""
<script>
window.addEventListener("load", function () {
  setTimeout(function () {
    history.pushState({x: 1}, "");
    history.pushState({x: 2}, "");
    console.log("PROBE pushed len=" + history.length +
                " state=" + JSON.stringify(history.state));
    window.onpopstate = function (e) {
      console.log("PROBE popstate-prop state=" + JSON.stringify(e.state) +
                  " hist=" + JSON.stringify(history.state));
    };
    window.addEventListener("popstate", function (e) {
      console.log("PROBE popstate-listener state=" + JSON.stringify(e.state));
    });
    history.go(-1);
    console.log("PROBE go-called");
    setTimeout(function () {
      console.log("PROBE after-go state=" + JSON.stringify(history.state) +
                  " len=" + history.length);
      history.back();
      console.log("PROBE back-called");
      setTimeout(function () {
        console.log("PROBE after-back state=" + JSON.stringify(history.state));
      }, 300);
    }, 300);
  }, 300);
});
</script>
""", "popstate-prop, popstate-listener, body-attr-popstate"),

    # `anchor-fragment-history-back-on-click.html`: `location.hash` twice, then
    # a click on an `<a href="#3">` whose handler calls `history.back()`. Two
    # `popstate`s are expected, in the order `#3` then `#1`.
    "hist-hash": ("""
<a id=lnk href="#3">three</a>
<script>
window.addEventListener("hashchange", function () {
  console.log("PROBE hashchange hash=" + location.hash);
});
window.addEventListener("popstate", function () {
  console.log("PROBE popstate hash=" + location.hash);
});
window.addEventListener("load", function () {
  setTimeout(function () {
    location.hash = "#1";
    console.log("PROBE set-1 hash=" + location.hash + " len=" + history.length);
    location.hash = "#2";
    console.log("PROBE set-2 hash=" + location.hash + " len=" + history.length);
    var lnk = document.getElementById("lnk");
    lnk.onclick = function () {
      console.log("PROBE anchor-onclick hash=" + location.hash);
      history.back();
    };
    lnk.click();
    console.log("PROBE anchor-clicked hash=" + location.hash);
    setTimeout(function () {
      console.log("PROBE hash-final hash=" + location.hash + " len=" + history.length);
    }, 600);
  }, 300);
});
</script>
""", "hashchange x2, anchor-onclick, popstate x2, final hash=#1"),

    # `location_reload.html` and `joint-session-history-*.html` read
    # `history.length` and expect it not to move; `009/010` need a subframe's
    # navigation to be visible in the parent's history at all.
    "hist-length": ("""
<script>
window.addEventListener("load", function () {
  console.log("PROBE len-initial " + history.length +
              " scroll=" + history.scrollRestoration);
  history.replaceState({r: 1}, "");
  console.log("PROBE len-after-replace " + history.length);
  history.pushState({p: 1}, "");
  console.log("PROBE len-after-push " + history.length);
  try {
    history.go(0);
    console.log("PROBE go0-called");
  } catch (e) { console.log("PROBE go0-threw " + e); }
  var fr = document.createElement("iframe");
  fr.src = "vwjh-child.html?from=histlen";
  fr.onload = function () {
    console.log("PROBE frame-load len=" + history.length);
  };
  document.body.appendChild(fr);
  setTimeout(function () {
    console.log("PROBE len-final " + history.length +
                " frames=" + window.length +
                " framesobj=" + (typeof window.frames));
  }, 1500);
});
</script>
""", "len grows by 1 on push, frame-load, len-final"),

    # The six `webmessaging` ids, each of which is a *self*-post differing only
    # in how `targetOrigin` is spelled. `e.origin` is asserted by 002.
    "pm-target-origin": ("""
<script>
var got = 0;
window.addEventListener("message", function (e) {
  console.log("PROBE message #" + (++got) + " data=" + JSON.stringify(e.data) +
              " origin=" + e.origin + " ports=" + (e.ports ? e.ports.length : "none"));
});
window.addEventListener("load", function () {
  var host = location.protocol + "//" + location.host;
  var forms = [
    ["star", "*"],
    ["origin", host],
    ["trailing-slash", host + "/"],
    ["double-slash", host + "//"],
    ["slash-only", "/"],
    ["empty-dict", {}],
    ["bogus-dict", {someBogusParameterOnThisDictionary: "food"}],
    ["one-arg", undefined]
  ];
  forms.forEach(function (pair) {
    try {
      if (pair[1] === undefined) { postMessage("d:" + pair[0]); }
      else { postMessage("d:" + pair[0], pair[1], []); }
      console.log("PROBE posted " + pair[0]);
    } catch (e) { console.log("PROBE post-threw " + pair[0] + " " + e); }
  });
  setTimeout(function () { console.log("PROBE pm-total " + got); }, 800);
});
</script>
""", "one message per form; 8 expected"),

    # `document.open-03.html` ("document.open and no singleton replacement")
    # plus the `document.write`d script BUG-568 says never executes.
    "doc-open": ("""
<script>
window.addEventListener("load", function () {
  try {
    var d = document.open();
    console.log("PROBE open-returned same=" + (d === document) +
                " type=" + (d && typeof d.write));
    document.write("<p id=written>written</p>");
    document.write("<scr" + "ipt>console.log('PROBE written-script-ran')</scr" + "ipt>");
    document.close();
    console.log("PROBE after-close found=" + !!document.getElementById("written") +
                " ready=" + document.readyState);
  } catch (e) { console.log("PROBE docopen-threw " + e); }
  setTimeout(function () {
    console.log("PROBE docopen-alive found=" + !!document.getElementById("written"));
  }, 500);
});
</script>
""", "open-returned same=true, written-script-ran"),

    # `change_parentage.html` moves a live `<iframe>` to another parent and
    # expects it to keep its document; `joint-session-history-remove-iframe`
    # removes one. Both need `contentWindow` to be a real context first.
    "frame-parentage": ("""
<div id=a></div><div id=b></div>
<script>
window.addEventListener("load", function () {
  var fr = document.createElement("iframe");
  fr.src = "vwjh-child.html?from=parentage";
  fr.onload = function () { console.log("PROBE frame-load"); };
  document.getElementById("a").appendChild(fr);
  setTimeout(function () {
    var cw = fr.contentWindow;
    console.log("PROBE before-move frames=" + window.length +
                " cw=" + (typeof cw) +
                " doc=" + (fr.contentDocument ? fr.contentDocument.readyState : "none"));
    document.getElementById("b").appendChild(fr);
    console.log("PROBE moved same-cw=" + (fr.contentWindow === cw) +
                " frames=" + window.length);
    fr.remove();
    console.log("PROBE removed frames=" + window.length +
                " len=" + history.length);
  }, 1200);
});
</script>
""", "frame-load, before-move cw=object, same-cw=true"),

    # The canvas surfaces still in the residual: `ctx.filter`, an SVG image
    # drawn into a canvas that must not taint it, `OffscreenCanvas` and
    # `transferControlToOffscreen`.
    "canvas-misc": ("""
<canvas id=cv width=40 height=40></canvas>
<script>
window.addEventListener("load", function () {
  var cv = document.getElementById("cv");
  var ctx = cv.getContext("2d");
  console.log("PROBE ctx=" + (ctx ? "yes" : "no") +
              " filter=" + (ctx ? JSON.stringify(ctx.filter) : "-") +
              " oc=" + (typeof OffscreenCanvas) +
              " tco=" + (typeof cv.transferControlToOffscreen) +
              " cib=" + (typeof createImageBitmap));
  try {
    ctx.filter = "blur(2px)";
    console.log("PROBE filter-set now=" + JSON.stringify(ctx.filter));
  } catch (e) { console.log("PROBE filter-threw " + e); }
  var img = new Image();
  img.onload = function () {
    try {
      ctx.drawImage(img, 0, 0);
      console.log("PROBE drew-svg");
      console.log("PROBE toDataURL len=" + cv.toDataURL().length);
    } catch (e) { console.log("PROBE taint-threw " + e); }
  };
  img.onerror = function () { console.log("PROBE svg-img-error"); };
  img.src = "vwjh-square.svg";
  setTimeout(function () {
    console.log("PROBE canvas-alive complete=" + img.complete);
  }, 1500);
});
</script>
""", "ctx yes, filter string, drew-svg, toDataURL"),

    # `websockets/unload-a-document/003.html` navigates a *subframe* and waits
    # for the parent to hear about it; the generic half of that — does a frame
    # navigation reach the parent at all — is measured here without a socket.
    "frame-navigate": ("""
<script>
window.addEventListener("load", function () {
  var fr = document.createElement("iframe");
  var loads = 0;
  fr.onload = function () {
    loads++;
    console.log("PROBE frame-load #" + loads);
    if (loads === 1) {
      setTimeout(function () {
        try {
          fr.contentWindow.location.href = "vwjh-child.html?from=second";
          console.log("PROBE frame-renavigated");
        } catch (e) { console.log("PROBE frame-renav-threw " + e); }
      }, 300);
    }
  };
  fr.src = "vwjh-child.html?from=first";
  document.body.appendChild(fr);
  setTimeout(function () { console.log("PROBE frame-nav-final loads=" + loads); }, 2000);
});
</script>
""", "frame-load #1 and #2, server GET of both child URLs"),

    # `iframe_sandbox_allow_top_navigation_by_user_activation_*`: the sandbox
    # flags are parsed (the browser logs them), but what the test waits for is
    # a `message` from the framed document reporting whether it could navigate
    # the top. `navigator.userActivation` is the gate those two ids differ on.
    "user-activation": ("""
<script>
window.addEventListener("load", function () {
  console.log("PROBE ua=" + (typeof navigator.userActivation) +
              " has=" + (navigator.userActivation ?
                         JSON.stringify({
                           isActive: navigator.userActivation.isActive,
                           hasBeenActive: navigator.userActivation.hasBeenActive
                         }) : "-") +
              " hasFocus=" + (typeof document.hasFocus === "function" ?
                              document.hasFocus() : "no-fn"));
  var btn = document.createElement("button");
  btn.onclick = function () {
    console.log("PROBE click-ua=" + (navigator.userActivation ?
                navigator.userActivation.isActive : "-"));
  };
  document.body.appendChild(btn);
  btn.click();
  var fr = document.createElement("iframe");
  fr.setAttribute("sandbox", "allow-scripts allow-top-navigation-by-user-activation");
  fr.src = "vwjh-child.html?from=sandbox";
  fr.onload = function () { console.log("PROBE sandbox-frame-load"); };
  document.body.appendChild(fr);
  setTimeout(function () { console.log("PROBE ua-final"); }, 1500);
});
</script>
""", "navigator.userActivation object, click-ua=true"),

    # BUG-480 slice 1 (P3, 2026-08-23) added a subframe pipeline that fetches
    # and runs `<iframe src>`; `frame-parentage`/`frame-navigate` saw no
    # request at all for a frame built with `createElement`. Whether the
    # pipeline covers only *parser-written* frames is the difference between
    # "frames do not load" and "frames load unless script made them", so the
    # two are measured side by side against the same server.
    "frame-parser": ("""
<iframe id=pf src="vwjh-child.html?from=parser"
        onload="console.log('PROBE parser-frame-load-attr')"></iframe>
<script>
document.getElementById("pf").addEventListener("load", function () {
  console.log("PROBE parser-frame-load-listener");
});
window.addEventListener("message", function (e) {
  console.log("PROBE parent-heard " + e.data);
});
window.addEventListener("load", function () {
  var pf = document.getElementById("pf");
  console.log("PROBE parser-frame cw=" + (typeof pf.contentWindow) +
              " doc=" + (pf.contentDocument ? pf.contentDocument.readyState : "none") +
              " frames=" + window.length);
  var df = document.createElement("iframe");
  df.src = "vwjh-child.html?from=dynamic";
  df.onload = function () { console.log("PROBE dynamic-frame-load"); };
  document.body.appendChild(df);
  setTimeout(function () {
    console.log("PROBE frame-parser-final frames=" + window.length +
                " dyn-cw=" + (typeof df.contentWindow));
  }, 1500);
});
</script>
""", "both child URLs on the server; parser- and dynamic-frame load"),

    # The third insertion shape, and the one three residual ids actually use:
    # a frame the *parser* wrote (so BUG-480 slice 1 saw it) whose `src` is
    # assigned later from script. `joint-session-history-remove-iframe.html`
    # and `location_reload.html` both start from `<iframe src=about:blank>`
    # or a bare `<iframe>` and then navigate it.
    "frame-late-src": ("""
<iframe id=blank src="about:blank"></iframe>
<iframe id=bare></iframe>
<script>
window.addEventListener("load", function () {
  var a = document.getElementById("blank"), b = document.getElementById("bare");
  a.onload = function () { console.log("PROBE late-src-load-blank"); };
  b.onload = function () { console.log("PROBE late-src-load-bare"); };
  a.src = "vwjh-child.html?from=late-blank";
  b.src = "vwjh-child.html?from=late-bare";
  console.log("PROBE late-src-assigned a=" + String(a.src).split("/").pop() +
              " b=" + String(b.src).split("/").pop());
  setTimeout(function () {
    console.log("PROBE late-src-final frames=" + window.length +
                " len=" + history.length);
  }, 1800);
});
</script>
""", "both late URLs on the server, both late-src-load markers"),

    # Round two, narrowing what `win-open` and `hist-*` found. `open()` both
    # returns an object *and* leaves the caller's document dead, so which of
    # the two happened has to be asked of the object itself.
    "win-open-detail": ("""
<script>
window.addEventListener("load", function () {
  var w = open();
  console.log("PROBE noargs type=" + (typeof w) +
              " is-self=" + (w === window) +
              " opener=" + (window.opener === null ? "null" : typeof window.opener) +
              " closed=" + window.closed +
              " name=" + JSON.stringify(window.name) +
              " hasfocus=" + (w && typeof w.focus));
  try {
    window.name = "vwjh-named";
    console.log("PROBE name-set now=" + JSON.stringify(window.name));
  } catch (e) { console.log("PROBE name-set-threw " + e); }
  try {
    console.log("PROBE noargs-loc=" + (w ? String(w.location).slice(0, 40) : "-") +
                " doc=" + (w ? typeof w.document : "-"));
  } catch (e) { console.log("PROBE noargs-loc-threw " + e); }
  setTimeout(function () { console.log("PROBE open-detail-alive"); }, 700);
});
</script>
""", "noargs is-self / a real second context; window.name settable"),

    # `win-open` and `win-open-detail` both came back with *zero* ticks, i.e.
    # the caller's own `setInterval` never fired again after `open()`. That is
    # a second, independent blocker on top of BUG-797's missing channel, so it
    # is measured on its own clock: the page ticks for two seconds before it
    # opens anything, and keeps a pre-scheduled timer chain running across the
    # call.
    "win-open-freeze": ("""
<script>
var beat = 0;
(function heartbeat() {
  console.log("PROBE beat " + (++beat) + " vis=" + document.visibilityState +
              " hidden=" + document.hidden);
  setTimeout(heartbeat, 400);
})();
setTimeout(function () {
  console.log("PROBE opening at beat " + beat);
  var w = open("vwjh-child.html?from=freeze");
  console.log("PROBE opened w=" + (typeof w) + " beat=" + beat);
}, 2000);
setTimeout(function () { console.log("PROBE post-open-timer beat=" + beat); }, 3500);
setTimeout(function () { console.log("PROBE late-timer beat=" + beat); }, 6000);
</script>
""", "beats continue past `opening`; post-open-timer and late-timer fire"),

    # `location_reload.html` pings five times from a reloading subframe; the
    # generic half — what a reload does to *this* document — is measured with
    # `localStorage` as the counter, because `sessionStorage` is empty on every
    # new document (BUG-836) and would make the page reload forever.
    "hist-reload": ("""
<script>
window.addEventListener("load", function () {
  var n = Number(localStorage.getItem("vwjh-reloads") || "0");
  console.log("PROBE reload-gen " + n + " href=" + String(location.href).slice(-28));
  if (n >= 2) { console.log("PROBE reload-stop"); return; }
  localStorage.setItem("vwjh-reloads", String(n + 1));
  setTimeout(function () {
    console.log("PROBE reloading");
    location.reload();
    console.log("PROBE reload-returned");
  }, 600);
});
</script>
""", "reload-gen 0, 1, 2 then reload-stop, one server GET per generation"),

    # Whether `location.reload()` re-requests the document. `hist-reload`
    # could not say: its GETs were of a probe page, which `server saw`
    # filters out. Relevant beyond history — `bypass-cache-revalidation.html`
    # and `Images can bypass no-cache` are in the same residual.
    "hist-reload-fetch": ("""
<script>
window.addEventListener("load", function () {
  try { localStorage.removeItem("vwjh-rt"); } catch (e) {}
  setTimeout(function () {
    console.log("PROBE going-to-target");
    location.href = "vwjh-reload-target.html";
  }, 400);
});
</script>
""", "three GETs of /vwjh-reload-target.html, one per generation"),

    # `history.go(0)` is the same navigation spelled through the traversal API,
    # and it is what killed the `hist-length` page outright.
    "hist-go0": ("""
<script>
window.addEventListener("load", function () {
  var n = Number(localStorage.getItem("vwjh-go0") || "0");
  console.log("PROBE go0-gen " + n);
  if (n >= 1) { console.log("PROBE go0-stop"); return; }
  localStorage.setItem("vwjh-go0", String(n + 1));
  setTimeout(function () {
    console.log("PROBE go0-calling");
    history.go(0);
    console.log("PROBE go0-returned");
    setTimeout(function () { console.log("PROBE go0-alive-after"); }, 800);
  }, 600);
});
</script>
""", "go0-calling, go0-returned, go0-gen 1"),

    # `back-pushstate-back-history-state.html` waits on a single `popstate`
    # after `history.back()`; `hist-hash` got one for a *fragment* entry and
    # `hist-popstate` none for a `pushState` entry, so the two are separated
    # here with a longer window in case the event is merely late.
    "hist-popstate-late": ("""
<script>
window.addEventListener("popstate", function (e) {
  console.log("PROBE late-popstate state=" + JSON.stringify(e.state) +
              " hash=" + location.hash);
});
window.addEventListener("hashchange", function () {
  console.log("PROBE late-hashchange hash=" + location.hash);
});
window.addEventListener("load", function () {
  setTimeout(function () {
    history.pushState({x: 1}, "");
    console.log("PROBE late-pushed len=" + history.length);
    history.back();
    console.log("PROBE late-back-called state=" + JSON.stringify(history.state));
    setTimeout(function () {
      console.log("PROBE late-t1 state=" + JSON.stringify(history.state));
    }, 1000);
    setTimeout(function () {
      console.log("PROBE late-t2 state=" + JSON.stringify(history.state) +
                  " len=" + history.length);
    }, 3000);
  }, 300);
});
</script>
""", "late-popstate within 3 s of late-back-called"),

    # Which half of `pushState` the traversal loses: `hist-hash` got a
    # `popstate` for an entry made by `location.hash =` and `hist-popstate`
    # none for one made by `pushState(state, "")`. The third argument is the
    # only difference between those two entries, so it is varied alone.
    "hist-pushstate-url": ("""
<script>
window.addEventListener("popstate", function (e) {
  console.log("PROBE url-popstate state=" + JSON.stringify(e.state) +
              " search=" + location.search + " hash=" + location.hash);
});
window.addEventListener("load", function () {
  setTimeout(function () {
    history.pushState({u: 1}, "", "?a");
    console.log("PROBE url-pushed href=" + String(location.href).slice(-30) +
                " search=" + location.search + " len=" + history.length);
    history.back();
    console.log("PROBE url-back-called");
    setTimeout(function () {
      console.log("PROBE url-t1 state=" + JSON.stringify(history.state) +
                  " href=" + String(location.href).slice(-30));
      history.pushState({h: 1}, "", "#frag");
      console.log("PROBE frag-pushed hash=" + location.hash);
      history.back();
      console.log("PROBE frag-back-called");
    }, 1200);
    setTimeout(function () { console.log("PROBE url-t2 hash=" + location.hash); }, 2600);
  }, 300);
});
</script>
""", "url-popstate after url-back-called and after frag-back-called"),

    # `document.open-03.html` needs `document.open()`; the probe found it
    # missing, so this variant separates the three members of the API and
    # re-measures the BUG-568 half (`document.write` of a `<script>`).
    "doc-write": ("""
<script>
window.addEventListener("load", function () {
  console.log("PROBE dmi open=" + (typeof document.open) +
              " close=" + (typeof document.close) +
              " write=" + (typeof document.write) +
              " writeln=" + (typeof document.writeln));
  try {
    document.write("<p id=w1>plain</p>");
    console.log("PROBE wrote-plain found=" + !!document.getElementById("w1"));
  } catch (e) { console.log("PROBE write-threw " + e); }
  try {
    document.write("<scr" + "ipt>console.log('PROBE wrote-script-ran')</scr" + "ipt>");
    console.log("PROBE wrote-script-tag");
  } catch (e) { console.log("PROBE write-script-threw " + e); }
  setTimeout(function () {
    console.log("PROBE doc-write-alive ready=" + document.readyState);
  }, 700);
});
</script>
""", "dmi open=function, wrote-plain found=true, wrote-script-ran"),

    # BUG-834 recorded that `unload`/`beforeunload` never fire on a navigation.
    # Re-measured here as the control for `win-close`: if neither path fires
    # them, the mechanism is the events, not `close()`.
    "unload-nav": ("""
<script>
window.onbeforeunload = function () { console.log("PROBE nav-beforeunload"); };
window.onunload = function () { console.log("PROBE nav-unload"); };
window.addEventListener("pagehide", function () { console.log("PROBE nav-pagehide"); });
window.addEventListener("load", function () {
  setTimeout(function () {
    console.log("PROBE navigating-away");
    location.href = "vwjh-child.html?from=unload";
  }, 800);
});
</script>
""", "nav-beforeunload, nav-unload, nav-pagehide, then child-ran"),
}

#: Files the probe pages reference. `vwjh-child.html` is the document every
#: window/frame variant opens: it reports its own relationship to its opener
#: and writes a `storage` key, which is how `noreferrer-null-opener.html`
#: transports `window.opener` back to the test.
ASSETS = {
    "vwjh-child.html": """<!doctype html>
<meta charset=utf-8>
<title>slice-28 probe child</title>
<script>
console.log("PROBE child-ran search=" + location.search +
            " opener=" + (window.opener === null ? "null" : typeof window.opener) +
            " parent-is-self=" + (window.parent === window) +
            " name=" + window.name);
try {
  localStorage.setItem("vwjh-opener", String(window.opener));
  console.log("PROBE child-stored");
} catch (e) { console.log("PROBE child-store-threw " + e); }
try {
  var target = window.opener || window.parent;
  if (target && target !== window) {
    target.postMessage("hello-from-child" + location.search, "*");
    console.log("PROBE child-posted");
  } else {
    console.log("PROBE child-no-target");
  }
} catch (e) { console.log("PROBE child-post-threw " + e); }
window.addEventListener("message", function (e) {
  console.log("PROBE child-heard " + e.data);
});
</script>
""",

    # Reloaded by `hist-reload-fetch`. It is a *separate* file from the probe
    # page on purpose: the probe pages are filtered out of `server saw` (they
    # would drown every line), so a reload of the probe page itself is
    # invisible and could not answer "was the document re-requested".
    "vwjh-reload-target.html": """<!doctype html>
<meta charset=utf-8>
<title>slice-28 reload target</title>
<script>
var n = Number(localStorage.getItem("vwjh-rt") || "0");
console.log("PROBE rt-gen " + n);
if (n < 2) {
  localStorage.setItem("vwjh-rt", String(n + 1));
  setTimeout(function () { console.log("PROBE rt-reloading"); location.reload(); }, 600);
} else {
  console.log("PROBE rt-stop");
}
</script>
""",

    "vwjh-asset.js": "window.vwjhAsset = 1;\n",

    "vwjh-square.svg": """<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<rect width="20" height="20" fill="#0a0"/></svg>
""",
}

_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages and their assets, recording every request."""

    protocol_version = "HTTP/1.1"

    def _record(self, method):
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.path}")

    def do_GET(self):  # noqa: N802 — http.server's own casing
        self._record("GET")
        super().do_GET()

    def do_POST(self):  # noqa: N802 — http.server's own casing
        self._record("POST")
        length = int(self.headers.get("content-length") or 0)
        if length:
            self.rfile.read(length)
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    """Start a background http server on `root`, return (port, shutdown)."""
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds):
    """Launch one browser on one probe page; return (ticks, markers, served)."""
    log_path = os.path.join(REPO, ".tmp", f"vwjh-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    page = f".vwjh-{name}.html"
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
    with _SERVED_LOCK:
        served = [p for p in SERVED if "/.vwjh-" not in p]
    return ticks, markers, served


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=8.0,
                        help="how long each page is allowed to run")
    parser.add_argument("--variant", action="append", default=None,
                        help="run only these variants (repeatable)")
    args = parser.parse_args()

    wanted = args.variant or list(VARIANTS)
    unknown = [name for name in wanted if name not in VARIANTS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    http_port, shutdown = _serve(HERE)
    origin = f"http://127.0.0.1:{http_port}"
    written = []
    for name in wanted:
        path = os.path.join(HERE, f".vwjh-{name}.html")
        body = VARIANTS[name][0].replace("__ORIGIN__", origin)
        page = (PAGE.replace("__NAME__", name)
                    .replace("__BODYATTR__", BODY_ATTRS.get(name, ""))
                    .replace("__BODY__", body))
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)

    try:
        print(f"{'variant':18s} {'ticks':>5s}  {'expected':58s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, served = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if served:
                # Counted, not de-duplicated: "was it fetched again?" is a
                # different question from "was it fetched", and `hist-reload`
                # is exactly the case where only the count answers it.
                counts = collections.Counter(served)
                seen += "   [server saw: " + ", ".join(
                    (f"{path} x{n}" if n > 1 else path)
                    for path, n in sorted(counts.items())) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:18s} {ticks:5d}  {VARIANTS[name][1]:58s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half — a document missing there was never fetched, "
              "so no browsing context was created for it, whatever the page or "
              "the browser log says (BUG-826).")
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
