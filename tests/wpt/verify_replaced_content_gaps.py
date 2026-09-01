#!/usr/bin/env python3
"""WPT-RUN-6 slice 30: replaced content — how a page learns that an image,
a video or a font arrived, how big it is, and whether its pixels can be read
back.

Slice 29 left 89 `unclassified` ids with no dominant directory, but sorted by
*what the file waits on* rather than by path they are not 89 unrelated
accidents. The largest family — about a quarter of the residual — is a page
that put a replaced element on screen and then waited for the engine to say
something about it:

    /html/semantics/embedded-content/the-img-element/null-image-source.html
    /html/rendering/replaced-elements/images/img-fallback-baseline-alignment.html
    /html/dom/elements/images/bypass-cache-revalidation.html
    /css/css-contain/content-visibility/content-visibility-input-image.html
    /jpegxl/{html-input-image,svg-image-element}.html
    /html/semantics/embedded-content/the-video-element/video-loading-lazy-*.html
    /html/semantics/embedded-content/the-video-element/video_crash_empty_src.html
    /fetch/range/non-matching-range-response.html
    /fetch/orb/tentative/status.sub.html
    /html/canvas/element/manual/**  (5 ids)
    /html/canvas/offscreen/manual/** (2 ids)
    /svg/embedded/image-crossorigin.sub.html
    /css/css-fonts/test_datafont_same_origin.html

Three existing bugs already claim parts of this ground — BUG-630 (`<img>`
fires no `load`), BUG-848 (a source that is not an `<img>` is never
requested), BUG-825 (no media resource selection) — yet none of these ids is
attributed to them, and the reason is visible in the sources: the mechanisms
were written around the *load* half. `null-image-source.html` and
`img-fallback-baseline-alignment.html` wait on `onerror`; `fetch/range` and
`fetch/orb` wait on a `<video>`'s `play`/`error`; `bypass-cache-revalidation`
waits on a second *request* rather than on an event; and the canvas ids wait
on `getImageData` returning pixels of something drawn. Whether the error half
is missing too, whether a cached image is re-requested, and whether the 2D
context can read back what it drew are separate questions from "does `load`
fire", and each of them decides a different bug.

What the probe separates, per variant:

* The error half of the image path (`img-empty-src`, `img-error-404`,
  `img-fallback-layout`) — `<img src="">` must fire `error` per HTML LS
  §4.8.3, an empty `srcset` and a bare `<img>` in a `<picture>` must not, and
  the fallback box must still take part in layout. Three ids turn on exactly
  this and none of them mentions `onload`.
* What the element exposes once a *good* image is served (`img-load-props`,
  `img-decode`, `img-srcset-sizes`) — `complete`, `naturalWidth`,
  `currentSrc`, `decode()`, and which candidate `srcset`/`sizes` picks, read
  off the probe's own server rather than off the element.
* Whether a second element pointed at an already-fetched URL re-requests it
  (`img-cache-revalidate`), which is the whole question of
  `bypass-cache-revalidation.html` and cannot be answered by any event.
* The two non-`<img>` sources of the residual (`input-image-src`,
  `svg-image-href`) — BUG-848 measured them through `fetch/metadata`'s
  `induceRequest` helper; these ids reach them the ordinary way, from script
  and from the parser.
* The `<video>` neighbourhood the residual actually needs (`video-src-events`,
  `video-autoplay-play`, `video-empty-src`, `video-lazy`): `loadstart` →
  `loadedmetadata` → `canplay` → `play`, `currentSrc`/`networkState`, whether
  `src=""` errors instead of crashing, and whether `loading=lazy` is reflected
  at all. `<audio>` is deliberately absent from every page — it freezes the
  document outright (BUG-799).
* Canvas readback (`canvas-getimagedata`, `canvas-drawimage-pixels`,
  `canvas-taint-crossorigin`, `canvas-svg-foreignobject`) — a padded canvas's
  `getImageData` (the entire assertion of `canvas-with-padding.html` is "does
  not crash"), the draw→read roundtrip `bypass-cache-revalidation` compares
  pixels through, and the origin-tainting rules `image-crossorigin.sub.html`
  asserts in both directions. The cross-origin half is served from a *second*
  server on another port, because the run's own `www1.` aliases do not
  resolve (WPT-RUN-10) and a probe must not measure that instead.
* The newer canvas surface two `manual/` ids need (`canvas-filter-offscreen`,
  `canvas-draw-element-image`) — `ctx.filter`, `OffscreenCanvas` in a worker,
  and the tentative `drawElementImage`/`captureElementImage`.
* `document.fonts` as an event target (`fontface-events`) —
  `test_datafont_same_origin.html` hangs in `onloadingdone`, which is a
  FontFaceSet event and not a font-loading question.
* `MediaRecorder` and the capture-stream entry points (`media-recorder`).

Same harness as slices 15/17-22/24/25/26/27/28/29 and for the reasons recorded
in `CLAUDE.md`: one browser process per page, served over http (never
`file://`), evidence read off the browser's own stderr rather than through an
MCP `eval`, a 500 ms `setInterval` tick so "the page is alive and heard
nothing" is separable from "the page died", and a variant per hazard so one
page that freezes cannot hide its neighbours' measurements. Requests are
recorded on the probe's own server and printed with counts — the browser's log
is not evidence that anything was fetched (BUG-826), and de-duplicating them
would erase the difference between "fetched" and "fetched again" (slice 28),
which is the only evidence `img-cache-revalidate` has.

Thirty variants. All settle inside the default 8 s.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_replaced_content_gaps.py
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

#: Paths the probe servers were asked for, with the request method. A file that
#: never appears here was never fetched, whatever the page or the browser's own
#: log says (BUG-826). Counted rather than de-duplicated: "was it fetched
#: again" is the question `img-cache-revalidate` turns on (slice 28).
SERVED = []
_SERVED_LOCK = threading.Lock()

#: A 1x1 green PNG, served dynamically from `vrc-dyn-image.png` so the probe
#: can attach its own `Cache-Control` and count how often the browser comes
#: back for it. Read off the vendored WPT asset rather than inlined, so the
#: bytes are a real decodable image and not a hand-written approximation.
with open(os.path.join(HERE, "media", "1x1-green.png"), "rb") as _png:
    GREEN_PNG = _png.read()

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-30 probe: __NAME__</title>
<body>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: Reporting helper spliced into every variant: one line per question, and a
#: question that throws is reported as a *measurement* rather than killing the
#: rest of the variant.
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
function has(name) { return typeof window[name] !== "undefined"; }
// Every element event a replaced-content test can wait on, armed in both
// spellings, so "the property form works and the listener form does not" is
// visible rather than assumed.
function watch(label, el, types) {
  types.forEach(function (t) {
    el.addEventListener(t, function () { console.log("PROBE " + label + "-lst-" + t); });
    try { el["on" + t] = function () { console.log("PROBE " + label + "-prop-" + t); }; }
    catch (e) { console.log("PROBE " + label + "-prop-" + t + "-THREW"); }
  });
}
</script>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("vrc-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                           function (e) { console.log("PROBE fetch-err " + e); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, timeout, fetch-ok, load"),

    # `null-image-source.html` waits on `onerror` for `<img src="">` and on a
    # 2 s timeout for the two forms that must *not* error. BUG-630 measured the
    # load half only.
    "img-empty-src": ("""
<img id=src_id src="">
<img id=srcset_id srcset="">
<picture><img id=pic_id></picture>
""" + REPORT + """
<script>
watch("empty-src", document.getElementById("src_id"), ["load", "error"]);
watch("empty-srcset", document.getElementById("srcset_id"), ["load", "error"]);
watch("picture", document.getElementById("pic_id"), ["load", "error"]);
window.addEventListener("load", function () {
  var made = document.createElement("img");
  watch("script-empty", made, ["load", "error"]);
  made.src = "";
  document.body.appendChild(made);
  setTimeout(function () {
    rep("empty-src-complete", function () {
      return String(document.getElementById("src_id").complete);
    });
    rep("empty-src-currentSrc", function () {
      return JSON.stringify(document.getElementById("src_id").currentSrc);
    });
    console.log("PROBE empty-done");
  }, 2500);
});
</script>
""", "error on src=\"\" only; no error on empty srcset / bare <picture> img"),

    # The control for the error path: a URL the server answers 404 for, and a
    # URL that does not resolve at all.
    "img-error-404": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var miss = document.createElement("img");
  watch("miss", miss, ["load", "error"]);
  miss.src = "vrc-no-such-image.png";
  document.body.appendChild(miss);
  var good = document.createElement("img");
  watch("good", good, ["load", "error"]);
  good.src = "media/1x1-green.png";
  document.body.appendChild(good);
  setTimeout(function () {
    rep("miss-complete", function () { return String(miss.complete); });
    rep("miss-naturalWidth", function () { return String(miss.naturalWidth); });
    rep("good-complete", function () { return String(good.complete); });
    rep("good-naturalWidth", function () { return String(good.naturalWidth); });
    console.log("PROBE err404-done");
  }, 2500);
});
</script>
""", "error on 404, load on the good one; complete/naturalWidth differ"),

    # `img-fallback-baseline-alignment.html` asserts on `offsetTop`/
    # `offsetHeight` of the fallback box from inside `onerror`. Both halves are
    # measured: does the event arrive, and is the fallback box laid out.
    "img-fallback-layout": ("""
<style>
  #box { background: #ccc; line-height: 200px; width: 100px; }
  #fb { border-right: solid 30px black; width: 30px; height: 30px; }
</style>
<div id=box><img id=fb src="" alt="alt text"></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var img = document.getElementById("fb");
  var box = document.getElementById("box");
  watch("fb", img, ["load", "error"]);
  img.src = "";
  setTimeout(function () {
    rep("img-offsetTop", function () { return String(img.offsetTop); });
    rep("img-offsetHeight", function () { return String(img.offsetHeight); });
    rep("box-offsetTop", function () { return String(box.offsetTop); });
    rep("box-offsetHeight", function () { return String(box.offsetHeight); });
    rep("img-rect", function () {
      var r = img.getBoundingClientRect(); return r.width + "x" + r.height;
    });
    console.log("PROBE fallback-done");
  }, 2000);
});
</script>
""", "offsetTop/offsetHeight of the fallback box, plus the error event"),

    # `implicit-sizes-ignores-width.html`'s question: which candidate is
    # chosen. Only the server can answer it.
    "img-srcset-sizes": ("""
<img id=ss srcset="media/1x1-green.png 1x, images/black-rectangle.png 2x">
<img id=sz sizes="100px" srcset="media/1x1-green.png 200w, images/apng.png 400w"
     width="50">
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  setTimeout(function () {
    rep("ss-currentSrc", function () {
      return JSON.stringify(document.getElementById("ss").currentSrc);
    });
    rep("sz-currentSrc", function () {
      return JSON.stringify(document.getElementById("sz").currentSrc);
    });
    rep("sz-sizes", function () { return document.getElementById("sz").sizes; });
    rep("ss-srcset", function () { return typeof document.getElementById("ss").srcset; });
    console.log("PROBE srcset-done");
  }, 2000);
});
</script>
""", "one candidate fetched; currentSrc names it"),

    "img-decode": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var img = document.createElement("img");
  img.src = "media/1x1-green.png";
  document.body.appendChild(img);
  rep("decode-typeof", function () { return typeof img.decode; });
  try {
    img.decode().then(function () { console.log("PROBE decode-resolved"); },
                      function (e) { console.log("PROBE decode-rejected " + e); });
  } catch (e) { console.log("PROBE decode-THREW " + e); }
  rep("Image-ctor", function () { return typeof new Image(2, 3); });
  rep("loading-attr", function () { return String(img.loading); });
  rep("crossOrigin-attr", function () { return String(img.crossOrigin); });
  setTimeout(function () { console.log("PROBE decode-done"); }, 2500);
});
</script>
""", "decode() settles; Image(w,h); loading/crossOrigin reflect"),

    # `bypass-cache-revalidation.html` in miniature: the same `no-cache` URL is
    # pointed at by a second element, and the only evidence is the server's
    # own request count.
    "img-cache-revalidate": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var url = "vrc-dyn-image.png?id=cache1";
  var first = document.createElement("img");
  first.src = url;
  document.body.appendChild(first);
  setTimeout(function () {
    var second = document.createElement("img");
    second.src = url;
    document.body.appendChild(second);
    var other = document.createElement("img");
    other.src = "vrc-dyn-image.png?id=cache2";
    document.body.appendChild(other);
  }, 1200);
  setTimeout(function () {
    rep("first-complete", function () { return String(first.complete); });
    console.log("PROBE cache-done");
  }, 3000);
});
</script>
""", "server sees ?id=cache1 once or twice; ?id=cache2 once"),

    # BUG-848 through the door these ids actually use: `input.type='image'`
    # from script (content-visibility-input-image.html) and from the parser
    # (jpegxl/html-input-image.html), plus the size readback the first asserts.
    "input-image-src": ("""
<input id=parser type=image src="media/1x1-green.png">
<div id=cv style="content-visibility:hidden"></div>
""" + REPORT + """
<script>
watch("parser-input", document.getElementById("parser"), ["load", "error"]);
window.addEventListener("load", function () {
  var made = document.createElement("input");
  made.type = "image";
  watch("script-input", made, ["load", "error"]);
  made.src = "images/black-rectangle.png";
  document.getElementById("cv").appendChild(made);
  setTimeout(function () {
    rep("parser-width", function () { return String(document.getElementById("parser").width); });
    rep("parser-height", function () { return String(document.getElementById("parser").height); });
    rep("cv-width", function () { return String(made.width); });
    rep("cv-height", function () { return String(made.height); });
    console.log("PROBE inputimage-done");
  }, 2500);
});
</script>
""", "server sees both sources; width/height nonzero"),

    "svg-image-href": ("""
<svg width="40" height="40">
  <image id=parser href="media/1x1-green.png" width="20" height="20"/>
</svg>
<svg id=host width="40" height="40"></svg>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var made = document.createElementNS("http://www.w3.org/2000/svg", "image");
  watch("script-image", made, ["load", "error"]);
  made.setAttribute("href", "images/black-rectangle.png");
  made.setAttribute("width", "20");
  made.setAttribute("height", "20");
  document.getElementById("host").appendChild(made);
  var css = document.createElement("div");
  css.style.backgroundImage = "url(images/apng.png)";
  css.style.width = css.style.height = "10px";
  document.body.appendChild(css);
  setTimeout(function () {
    rep("parser-href", function () {
      return String(document.getElementById("parser").getAttribute("href"));
    });
    console.log("PROBE svgimage-done");
  }, 2500);
});
</script>
""", "server sees the two <image> hrefs and the CSS background"),

    # The event chain `fetch/range` and `fetch/orb` hang in, plus what the
    # element exposes while it is hanging. `<audio>` is deliberately absent
    # from this page (BUG-799 freezes the document).
    "video-src-events": ("""
<video id=v src="media/2x2-green.webm" muted></video>
""" + REPORT + """
<script>
watch("v", document.getElementById("v"),
      ["loadstart", "loadedmetadata", "loadeddata", "canplay", "play", "playing",
       "error", "progress", "suspend", "stalled"]);
window.addEventListener("load", function () {
  var v = document.getElementById("v");
  rep("currentSrc", function () { return JSON.stringify(v.currentSrc); });
  rep("networkState", function () { return String(v.networkState); });
  rep("readyState", function () { return String(v.readyState); });
  rep("load-fn", function () { return typeof v.load; });
  rep("canPlayType", function () { return String(v.canPlayType("video/webm")); });
  rep("videoWidth", function () { return String(v.videoWidth); });
  rep("error-obj", function () { return String(v.error); });
  setTimeout(function () {
    rep("late-networkState", function () { return String(v.networkState); });
    rep("late-readyState", function () { return String(v.readyState); });
    rep("late-currentSrc", function () { return JSON.stringify(v.currentSrc); });
    console.log("PROBE video-done");
  }, 3000);
});
</script>
""", "loadstart/loadedmetadata/canplay; currentSrc set; server sees the webm"),

    # `fetch/range/non-matching-range-response.html`'s exact shape: an
    # `autoplay muted` video built from script, waited on through `play`.
    "video-autoplay-play": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var v = document.createElement("video");
  v.autoplay = true;
  v.muted = true;
  watch("auto", v, ["loadstart", "canplay", "play", "playing", "error"]);
  v.src = "media/2x2-green.webm";
  document.body.appendChild(v);
  rep("autoplay-prop", function () { return String(v.autoplay); });
  rep("muted-prop", function () { return String(v.muted); });
  rep("paused", function () { return String(v.paused); });
  rep("play-fn", function () { return typeof v.play; });
  try {
    var p = v.play();
    rep("play-returns", function () { return String(p && typeof p.then); });
    if (p && p.then) {
      p.then(function () { console.log("PROBE play-resolved"); },
             function (e) { console.log("PROBE play-rejected " + e); });
    }
  } catch (e) { console.log("PROBE play-THREW " + e); }
  setTimeout(function () {
    rep("late-paused", function () { return String(v.paused); });
    rep("late-currentTime", function () { return String(v.currentTime); });
    console.log("PROBE autoplay-done");
  }, 3000);
});
</script>
""", "play event or a rejected play() promise; not silence"),

    # `video_crash_empty_src.html` asserts only "does not crash" — for a page
    # that never gets that far, the interesting half is whether anything at all
    # is reported.
    "video-empty-src": ("""
<video id=blank src="about:blank"></video>
<video id=empty src=""></video>
""" + REPORT + """
<script>
watch("blank", document.getElementById("blank"), ["error", "loadstart", "loadedmetadata"]);
watch("empty", document.getElementById("empty"), ["error", "loadstart", "loadedmetadata"]);
window.addEventListener("load", function () {
  setTimeout(function () {
    rep("blank-error", function () {
      var e = document.getElementById("blank").error;
      return e ? String(e.code) : String(e);
    });
    rep("empty-error", function () {
      var e = document.getElementById("empty").error;
      return e ? String(e.code) : String(e);
    });
    rep("blank-networkState", function () {
      return String(document.getElementById("blank").networkState);
    });
    console.log("PROBE emptysrc-done");
  }, 2000);
});
</script>
""", "an error event or a MediaError; the page survives either way"),

    "video-lazy": ("""
<style>.below { margin-top: 1000vh; }</style>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var v = document.createElement("video");
  v.loading = "lazy";
  v.className = "below";
  watch("lazy", v, ["loadstart", "error"]);
  v.src = "media/2x2-green.webm?lazy=1";
  document.body.appendChild(v);
  rep("loading-prop", function () { return String(v.loading); });
  rep("loading-attr", function () { return String(v.getAttribute("loading")); });
  var img = document.createElement("img");
  img.loading = "lazy";
  img.className = "below";
  img.src = "media/1x1-green.png?lazy=1";
  document.body.appendChild(img);
  rep("img-loading-prop", function () { return String(img.loading); });
  setTimeout(function () {
    v.loading = "eager";
    console.log("PROBE lazy-switched-eager");
  }, 1500);
  setTimeout(function () { console.log("PROBE lazy-done"); }, 3500);
});
</script>
""", "loading reflects; whether the below-viewport source is fetched at all"),

    # The whole assertion of `canvas-with-padding.html` is "does not crash",
    # with a padding value that overflows a signed 32-bit int.
    "canvas-getimagedata": ("""
<canvas id=pad style="padding-right: 4294967292; border-right: 124px solid black;"></canvas>
<canvas id=plain width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("pad-ctx", function () {
    return typeof document.getElementById("pad").getContext("2d");
  });
  rep("pad-getImageData", function () {
    var d = document.getElementById("pad").getContext("2d").getImageData(0, 0, 1, 1);
    return d.data.length;
  });
  var c = document.getElementById("plain");
  var ctx = c.getContext("2d");
  rep("fill-read", function () {
    ctx.fillStyle = "#00ff00";
    ctx.fillRect(0, 0, 20, 20);
    var d = ctx.getImageData(0, 0, 1, 1).data;
    return [d[0], d[1], d[2], d[3]].join(",");
  });
  rep("toDataURL", function () { return c.toDataURL().slice(0, 22); });
  rep("ImageData-ctor", function () { return String(new ImageData(2, 2).data.length); });
  rep("putImageData", function () {
    ctx.putImageData(new ImageData(2, 2), 0, 0);
    return "ok";
  });
  console.log("PROBE getimagedata-done");
});
</script>
""", "no crash on the padded canvas; fill→read roundtrip returns 0,255,0,255"),

    # The roundtrip `bypass-cache-revalidation.html` compares pixels through.
    "canvas-drawimage-pixels": ("""
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var img = document.createElement("img");
  img.src = "media/1x1-green.png";
  document.body.appendChild(img);
  setTimeout(function () {
    var ctx = document.getElementById("c").getContext("2d");
    rep("img-complete", function () { return String(img.complete); });
    rep("drawImage", function () { ctx.drawImage(img, 0, 0, 10, 10); return "ok"; });
    rep("pixel", function () {
      var d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    });
    rep("drawImage-canvas", function () {
      var src = document.createElement("canvas");
      src.width = src.height = 4;
      src.getContext("2d").fillStyle = "#ff0000";
      src.getContext("2d").fillRect(0, 0, 4, 4);
      ctx.drawImage(src, 10, 10);
      var d = ctx.getImageData(11, 11, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    });
    console.log("PROBE drawpixels-done");
  }, 2500);
});
</script>
""", "drawImage(<img>) then getImageData returns the image's pixels"),

    # `image-crossorigin.sub.html` asserts both directions. The second origin
    # is a second port of the probe's own server — the run's `www1.` aliases do
    # not resolve here (WPT-RUN-10) and measuring that instead would be wrong.
    "canvas-taint-crossorigin": ("""
<img id=same src="media/1x1-green.png?taint=same">
<img id=cross src="__ALT_ORIGIN__/images/black-rectangle.png?taint=cross">
<img id=corsed crossorigin=anonymous
     src="__ALT_ORIGIN__/images/black-rectangle.png?taint=cors">
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
// Parser-written on purpose: a script-built image is not registered in the
// canvas bitmap store at all, so it draws nothing and the tainting question
// could not be asked through it (measured — `canvas-drawimage-parser`).
window.addEventListener("load", function () {
  setTimeout(function () {
    var ctx = document.getElementById("c").getContext("2d");
    function el(id) { return document.getElementById(id); }
    rep("crossOrigin-reflects", function () { return String(el("corsed").crossOrigin); });
    rep("same-draw", function () { ctx.drawImage(el("same"), 0, 0, 4, 4); return "ok"; });
    rep("same-read", function () {
      var d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    });
    rep("cross-draw", function () { ctx.drawImage(el("cross"), 8, 8, 4, 4); return "ok"; });
    rep("cross-read", function () {
      var d = ctx.getImageData(9, 9, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    });
    rep("cross-toDataURL", function () {
      return document.getElementById("c").toDataURL().slice(0, 22);
    });
    console.log("PROBE taint-done");
  }, 2500);
});
</script>
""", "a cross-origin draw then read must throw SecurityError"),

    "canvas-svg-foreignobject": ("""
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var plain = document.createElement("img");
  plain.src = "vrc-square.svg";
  var fo = document.createElement("img");
  fo.src = "vrc-foreignobject.svg";
  [plain, fo].forEach(function (el) { document.body.appendChild(el); });
  setTimeout(function () {
    var ctx = document.getElementById("c").getContext("2d");
    rep("svg-complete", function () { return String(plain.complete); });
    rep("svg-draw", function () { ctx.drawImage(plain, 0, 0); return "ok"; });
    rep("svg-read", function () { return ctx.getImageData(0, 0, 1, 1).data.length; });
    rep("fo-draw", function () { ctx.drawImage(fo, 5, 5); return "ok"; });
    rep("fo-read", function () { return ctx.getImageData(5, 5, 1, 1).data.length; });
    console.log("PROBE foreignobject-done");
  }, 2500);
});
</script>
""", "an SVG image draws, and a <foreignObject> in it does not taint"),

    "canvas-filter-offscreen": ("""
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var c = document.getElementById("c");
  var ctx = c.getContext("2d");
  rep("ctx-filter", function () { ctx.filter = "blur(2px)"; return String(ctx.filter); });
  rep("OffscreenCanvas", function () { return typeof OffscreenCanvas; });
  rep("offscreen-ctx", function () {
    return typeof new OffscreenCanvas(10, 10).getContext("2d");
  });
  rep("offscreen-filter", function () {
    var o = new OffscreenCanvas(10, 10).getContext("2d");
    o.filter = "blur(1px)";
    return String(o.filter);
  });
  rep("transferControlToOffscreen", function () {
    return typeof c.transferControlToOffscreen;
  });
  rep("convertToBlob", function () {
    return typeof new OffscreenCanvas(4, 4).convertToBlob;
  });
  rep("worker-transfer", function () {
    var src = "self.onmessage = function (e) {" +
              "  var o = e.data.canvas;" +
              "  var g = o.getContext('2d');" +
              "  g.fillStyle = '#00f'; g.fillRect(0, 0, 4, 4);" +
              "  self.postMessage('worker-drew ' + (g.filter === undefined ? 'nofilter' : g.filter));" +
              "};";
    var w = new Worker(URL.createObjectURL(new Blob([src], {type: "text/javascript"})));
    w.onmessage = function (e) { console.log("PROBE worker-said " + e.data); };
    w.onerror = function (e) { console.log("PROBE worker-error " + (e && e.message)); };
    var off = document.createElement("canvas").transferControlToOffscreen();
    w.postMessage({canvas: off}, [off]);
    return "posted";
  });
  setTimeout(function () { console.log("PROBE filter-done"); }, 3000);
});
</script>
""", "ctx.filter, OffscreenCanvas in a worker, convertToBlob"),

    "canvas-draw-element-image": ("""
<canvas id=c width="20" height="20"></canvas>
<div id=subject style="width:10px;height:10px;background:#0f0"></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var ctx = document.getElementById("c").getContext("2d");
  rep("drawElementImage", function () { return typeof ctx.drawElementImage; });
  rep("captureElementImage", function () { return typeof ctx.captureElementImage; });
  rep("ctx-getTransform", function () { return typeof ctx.getTransform; });
  rep("createImageBitmap", function () { return typeof createImageBitmap; });
  rep("ImageBitmap-global", function () { return typeof ImageBitmap; });
  rep("captureStream", function () {
    return typeof document.getElementById("c").captureStream;
  });
  rep("bitmaprenderer", function () {
    return typeof document.createElement("canvas").getContext("bitmaprenderer");
  });
  console.log("PROBE drawelement-done");
});
</script>
""", "the tentative draw/captureElementImage entry points and ImageBitmap"),

    # `test_datafont_same_origin.html` hangs in `document.fonts.onloadingdone`,
    # which is a FontFaceSet event rather than a font-loading question — both
    # halves are measured, against a `data:` font and an http one.
    "fontface-events": ("""
<style>
@font-face { font-family: DataFont;
  src: url(data:font/opentype;base64,AAEAAAANAIAAAwBQRkZUTQ==); }
@font-face { font-family: HttpFont; src: url(vrc-font.ttf); }
</style>
<p id=d style="font-family: DataFont">data font</p>
<p id=h style="font-family: HttpFont">http font</p>
""" + REPORT + """
<script>
rep("fonts-typeof", function () { return typeof document.fonts; });
try {
  document.fonts.onloadingdone = function (e) {
    console.log("PROBE loadingdone faces=" +
                (e && e.fontfaces ? e.fontfaces.length : "no-fontfaces"));
  };
  document.fonts.onloadingerror = function (e) { console.log("PROBE loadingerror"); };
  document.fonts.onloading = function (e) { console.log("PROBE loading"); };
  document.fonts.addEventListener("loadingdone",
    function () { console.log("PROBE loadingdone-lst"); });
} catch (e) { console.log("PROBE fonts-handlers-THREW " + e); }
window.addEventListener("load", function () {
  rep("fonts-size", function () { return String(document.fonts.size); });
  rep("fonts-status", function () { return String(document.fonts.status); });
  rep("fonts-check", function () { return String(document.fonts.check("12px HttpFont")); });
  rep("fonts-ready", function () { return typeof document.fonts.ready.then; });
  try {
    document.fonts.ready.then(function () { console.log("PROBE fonts-ready-resolved"); });
    document.fonts.load("12px HttpFont").then(
      function (f) { console.log("PROBE fonts-load-resolved n=" + (f && f.length)); },
      function (e) { console.log("PROBE fonts-load-rejected " + e); });
  } catch (e) { console.log("PROBE fonts-load-THREW " + e); }
  rep("FontFace-ctor", function () { return typeof FontFace; });
  setTimeout(function () { console.log("PROBE fonts-done"); }, 3000);
});
</script>
""", "loadingdone/loadingerror fire; server sees vrc-font.ttf"),

    # `video_crash_empty_src.html` builds its element from *script* and appends
    # it — the parser form measured by `video-empty-src` is a different path
    # through the resource selection algorithm (BUG-825 was fixed on
    # 2026-08-25, so both halves have to be re-measured rather than assumed).
    "video-script-empty-src": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  ["about:blank", ""].forEach(function (src, i) {
    var v = document.createElement("video");
    v.controls = true;
    watch("s" + i, v, ["error", "loadstart", "loadedmetadata"]);
    v.src = src;
    document.body.appendChild(v);
    setTimeout(function () {
      rep("s" + i + "-error", function () {
        return v.error ? String(v.error.code) : String(v.error);
      });
      rep("s" + i + "-networkState", function () { return String(v.networkState); });
    }, 2000);
  });
  setTimeout(function () { console.log("PROBE scriptempty-done"); }, 2500);
});
</script>
""", "an error event on both, from the script-built path"),

    # Which container the engine claims, and whether the bytes are ever asked
    # for. `fetch/range` and `fetch/orb` both hang waiting for a `<video>` to
    # reach `play`, so "no format is supported" and "the file is never
    # requested" are two different answers with the same symptom.
    "video-formats": ("""
<video id=srcel><source src="media/2x2-green.mp4" type="video/mp4"></video>
""" + REPORT + """
<script>
watch("srcel", document.getElementById("srcel"), ["loadstart", "loadedmetadata", "error"]);
window.addEventListener("load", function () {
  var v = document.createElement("video");
  rep("canPlayType-mp4", function () {
    return JSON.stringify(v.canPlayType("video/mp4"));
  });
  rep("canPlayType-mp4-codecs", function () {
    return JSON.stringify(v.canPlayType('video/mp4; codecs="avc1.42E01E"'));
  });
  rep("canPlayType-webm", function () { return JSON.stringify(v.canPlayType("video/webm")); });
  rep("canPlayType-ogg", function () { return JSON.stringify(v.canPlayType("video/ogg")); });
  var mp4 = document.createElement("video");
  watch("mp4", mp4, ["loadstart", "loadedmetadata", "canplay", "error"]);
  mp4.src = "media/2x2-green.mp4";
  document.body.appendChild(mp4);
  setTimeout(function () {
    rep("mp4-error", function () {
      return mp4.error ? String(mp4.error.code) : String(mp4.error);
    });
    rep("mp4-readyState", function () { return String(mp4.readyState); });
    rep("srcel-currentSrc", function () {
      return JSON.stringify(document.getElementById("srcel").currentSrc);
    });
    console.log("PROBE formats-done");
  }, 3000);
});
</script>
""", "canPlayType for mp4/webm/ogg; whether either container is fetched"),

    # The pixels half of `bypass-cache-revalidation.html` and
    # `image-crossorigin.sub.html`: both compare what came back out of the
    # canvas, so "drawImage did not throw" is not the measurement — the values
    # are.
    "canvas-drawimage-visible": ("""
<canvas id=c width="40" height="40"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var png = document.createElement("img");
  png.src = "images/black-rectangle.png";
  var svg = document.createElement("img");
  svg.src = "vrc-square.svg";
  [png, svg].forEach(function (el) { document.body.appendChild(el); });
  setTimeout(function () {
    var ctx = document.getElementById("c").getContext("2d");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, 40, 40);
    function sample(x, y) {
      var d = ctx.getImageData(x, y, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    }
    rep("png-naturalWidth", function () { return String(png.naturalWidth); });
    rep("png-draw", function () { ctx.drawImage(png, 0, 0, 20, 20); return "ok"; });
    rep("png-pixel", function () { return sample(5, 5); });
    rep("svg-draw", function () { ctx.drawImage(svg, 20, 20, 20, 20); return "ok"; });
    rep("svg-pixel", function () { return sample(25, 25); });
    rep("white-control", function () { return sample(38, 2); });
    console.log("PROBE visible-done");
  }, 2500);
});
</script>
""", "a drawn image changes the pixels; the white control does not"),

    # `offscreencanvas.filter.w.html` and its two neighbours open with
    # `createImageBitmap(patternCanvas).then(...)`, so the whole file depends
    # on a promise BUG-880 rejects — no test is ever registered.
    "createimagebitmap-canvas": ("""
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var c = document.getElementById("c");
  c.getContext("2d").fillStyle = "#a00";
  c.getContext("2d").fillRect(0, 0, 10, 10);
  ["canvas", "img", "blob"].forEach(function (kind) {
    var source = c;
    if (kind === "img") {
      source = document.createElement("img");
      source.src = "images/black-rectangle.png";
    } else if (kind === "blob") {
      source = new Blob([new Uint8Array([1, 2, 3])], {type: "image/png"});
    }
    try {
      createImageBitmap(source).then(
        function (b) { console.log("PROBE bitmap-" + kind + "-ok " + (b && b.width)); },
        function (e) { console.log("PROBE bitmap-" + kind + "-rejected " + e); });
    } catch (e) { console.log("PROBE bitmap-" + kind + "-THREW " + e); }
  });
  setTimeout(function () { console.log("PROBE bitmap-done"); }, 2500);
});
</script>
""", "createImageBitmap(canvas) resolves — three ids open with it"),

    # A background image is the one subresource path this cluster has not
    # measured, and `svg-image-href` raised the question by accident: the
    # script-set background of its control never reached the server.
    "css-background-fetch": ("""
<div id=parser style="background-image:url(images/apng.png);width:10px;height:10px"></div>
<style>#sheet { background-image: url(images/anim-gr.png); width: 10px; height: 10px; }</style>
<div id=sheet></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var made = document.createElement("div");
  made.style.backgroundImage = "url(images/black-rectangle.png)";
  made.style.width = made.style.height = "10px";
  document.body.appendChild(made);
  setTimeout(function () {
    rep("script-bg", function () { return made.style.backgroundImage; });
    rep("computed-bg", function () {
      return getComputedStyle(made).backgroundImage;
    });
    console.log("PROBE background-done");
  }, 2500);
});
</script>
""", "server sees all three background URLs (parser, stylesheet, script)"),

    # The ordering `video_crash_empty_src.html` and `fetch/orb/.../status.sub.html`
    # are both written in: `v.src = url` FIRST, the listener after. If the
    # engine dispatches the resource-selection error inline from the setter,
    # the listener attached on the next line can never see it — a page that is
    # entirely correct then waits forever. Measured against the same element
    # with the listener attached first, on the same page.
    "video-error-timing": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  // (a) the WPT ordering: src assigned before anything is listening.
  var late = document.createElement("video");
  late.src = "";
  late.addEventListener("error", function () { console.log("PROBE late-lst-error"); });
  late.onerror = function () { console.log("PROBE late-prop-error"); };
  document.body.appendChild(late);
  // (b) the same element with the listener armed first.
  var early = document.createElement("video");
  early.addEventListener("error", function () { console.log("PROBE early-lst-error"); });
  early.src = "";
  document.body.appendChild(early);
  // (c) an <img>, for the same question on the other replaced element.
  var img = document.createElement("img");
  img.src = "vrc-no-such-image.png?timing=1";
  img.addEventListener("error", function () { console.log("PROBE img-late-error"); });
  document.body.appendChild(img);
  setTimeout(function () {
    rep("late-error-obj", function () {
      return late.error ? String(late.error.code) : String(late.error);
    });
    rep("early-error-obj", function () {
      return early.error ? String(early.error.code) : String(early.error);
    });
    console.log("PROBE timing-done");
  }, 2000);
});
</script>
""", "early sees the error; whether late does is the whole question"),

    # The other half of `canvas-drawimage-visible`: an image the *parser* wrote,
    # which is the one form `page_pipeline.rs`'s single `register_img_bitmaps`
    # pass can see. If this one draws and the script-built one does not, the
    # defect is the one-shot registration rather than canvas or decoding.
    "canvas-drawimage-parser": ("""
<img id=p src="images/black-rectangle.png">
<canvas id=c width="40" height="40"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  setTimeout(function () {
    var ctx = document.getElementById("c").getContext("2d");
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, 40, 40);
    function sample(x, y) {
      var d = ctx.getImageData(x, y, 1, 1).data;
      return [d[0], d[1], d[2], d[3]].join(",");
    }
    var p = document.getElementById("p");
    rep("parser-draw", function () { ctx.drawImage(p, 0, 0, 20, 20); return "ok"; });
    rep("parser-pixel", function () { return sample(5, 5); });
    rep("parser-bitmap", function () {
      createImageBitmap(p).then(
        function (b) { console.log("PROBE parser-bitmap-ok " + b.width); },
        function (e) { console.log("PROBE parser-bitmap-rejected " + e); });
      return "asked";
    });
    // Same element, re-pointed from script after the pipeline's single pass.
    rep("repoint", function () { p.src = "media/1x1-green.png"; return "ok"; });
    setTimeout(function () {
      rep("repoint-draw", function () { ctx.drawImage(p, 20, 20, 20, 20); return "ok"; });
      rep("repoint-pixel", function () { return sample(25, 25); });
      console.log("PROBE parserdraw-done");
    }, 1500);
  }, 2000);
});
</script>
""", "a parser-written image draws; a script-repointed one may not"),

    # `offscreencanvas.filter.w.html` builds its bitmaps inside a worker, and
    # the only path there is `postMessage(canvas, [canvas])`. The main-thread
    # half of that transfer exists (`_lumenSerializeWithTransfers` in
    # `worker.rs` neuters the source and emits a `__lumen_sentinel__` object
    # carrying the pixels); this variant asks what the *worker* end makes of
    # it, which is the half no test in the residual can see from the page.
    "worker-offscreen-transfer": ("""
<canvas id=c width="8" height="8"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var src = "self.onmessage = function (e) {" +
            "  var o = e.data.canvas;" +
            "  self.postMessage('typeof=' + (typeof o) +" +
            "    ' ctor=' + (o && o.constructor ? o.constructor.name : 'none') +" +
            "    ' keys=' + (o ? Object.keys(o).join('/') : 'none') +" +
            "    ' sentinel=' + (o && o.__lumen_sentinel__ ? 'yes' : 'no') +" +
            "    ' getContext=' + (typeof (o && o.getContext)) +" +
            "    ' OffscreenCanvas=' + (typeof OffscreenCanvas) +" +
            "    ' native=' + (typeof _lumen_offscreen_canvas_from_image_data));" +
            "};";
  var w = new Worker(URL.createObjectURL(new Blob([src], {type: "text/javascript"})));
  w.onmessage = function (e) { console.log("PROBE worker-said " + e.data); };
  w.onerror = function (e) { console.log("PROBE worker-error " + (e && e.message)); };
  rep("transfer", function () {
    var off = document.getElementById("c").transferControlToOffscreen();
    w.postMessage({canvas: off}, [off]);
    return "posted";
  });
  // The sender's own half: the contract says the source canvas is neutered —
  // §4.12.5 makes `getContext` return null afterwards and a second
  // `transferControlToOffscreen()` throw `InvalidStateError`. Both are
  // observable from the page, so neither reading needs an engine internal.
  rep("source-getcontext-after", function () {
    return String(document.getElementById("c").getContext("2d"));
  });
  rep("source-transfer-twice", function () {
    return typeof document.getElementById("c").transferControlToOffscreen();
  });
  setTimeout(function () { console.log("PROBE wot-done"); }, 3000);
});
</script>
""", "what a worker receives when an OffscreenCanvas is transferred to it"),

    "media-recorder": ("""
<canvas id=c width="20" height="20"></canvas>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("MediaRecorder", function () { return typeof MediaRecorder; });
  rep("MediaStream", function () { return typeof MediaStream; });
  rep("mediaDevices", function () { return typeof navigator.mediaDevices; });
  rep("canvas-captureStream", function () {
    return typeof document.getElementById("c").captureStream;
  });
  rep("recorder-ctor", function () {
    var stream = document.getElementById("c").captureStream();
    return typeof new MediaRecorder(stream);
  });
  rep("isTypeSupported", function () {
    return String(MediaRecorder.isTypeSupported("video/webm"));
  });
  console.log("PROBE recorder-done");
});
</script>
""", "MediaRecorder / captureStream / mediaDevices presence"),
}

ASSETS = {
    "vrc-asset.js": "window.vrcAsset = 1;\n",

    "vrc-square.svg": """<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<rect width="20" height="20" fill="#0a0"/></svg>
""",

    # The taint question of
    # `drawimage_svg_image_with_foreign_object_does_not_taint.html`: an SVG
    # image whose content is HTML.
    "vrc-foreignobject.svg": """<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<foreignObject width="20" height="20">
<div xmlns="http://www.w3.org/1999/xhtml" style="background:#00a;width:20px;height:20px"></div>
</foreignObject></svg>
""",
}

#: The second evidence class of this slice: the *real* WPT file, run under the
#: *real* `testharness.js`, with a console reporter appended in place of
#: `testharnessreport.js`. A variant above measures one question in isolation;
#: a replay answers the different question "would this file pass if the runner
#: reached it", which is what separates an engine gap from a harness or runner
#: gap. The copy is written next to the original so every relative reference
#: (`resources/dice.png`, `support/*.html`) still resolves, and the probe's own
#: server records what the page asks for exactly as it does for a variant.
#:
#: The reporter uses `add_completion_callback`, the same hook
#: `testharnessreport.js` uses, so it sees the harness's own verdict —
#: including its `"Test timed out"` status, which is the distinction the
#: BUG-796 note in `CLAUDE.md` turns on.
#:
#: The copy must *drop* the file's own `testharnessreport.js` tag: on disk that
#: file is an un-substituted template (`%(output)s`, `%(timeout_multiplier)s`,
#: ... — `wptrunner` fills those in when it builds the static route), so a
#: plain http server hands the page a syntax error and every replay dies at
#: `Unexpected token '%'` before the test body runs. Measured the first time
#: this ran.
REPLAY_REPORTER = """
<script>
add_completion_callback(function (tests, status) {
  console.log("PROBE replay-status " + status.status + " " +
              (status.message || ""));
  tests.forEach(function (t) {
    console.log("PROBE replay-test [" + t.status + "] " + t.name + " :: " +
                (t.message || ""));
  });
  console.log("PROBE replay-done");
});
setTimeout(function () { console.log("PROBE replay-no-completion"); }, 6000);
</script>
"""

#: `name -> path of the real test, relative to the WPT root (== `HERE`)`.
REPLAYS = {
    "replay-null-image-source":
        "html/semantics/embedded-content/the-img-element/null-image-source.html",
    "replay-video-crash-empty-src":
        "html/semantics/embedded-content/the-video-element/video_crash_empty_src.html",
    "replay-canvas-with-padding":
        "html/canvas/element/manual/context-attributes/canvas-with-padding.html",
    "replay-img-fallback-baseline":
        "html/rendering/replaced-elements/images/img-fallback-baseline-alignment.html",
    "replay-content-visibility-input-image":
        "css/css-contain/content-visibility/content-visibility-input-image.html",
    "replay-datafont-same-origin":
        "css/css-fonts/test_datafont_same_origin.html",
    "replay-jpegxl-input-image": "jpegxl/html-input-image.html",
    "replay-jpegxl-svg-image": "jpegxl/svg-image-element.html",
    "replay-bypass-cache": "html/dom/elements/images/bypass-cache-revalidation.html",
    "replay-implicit-sizes":
        "html/semantics/embedded-content/the-img-element/sizes/"
        "implicit-sizes-ignores-width.html",
    "replay-video-lazy-to-eager":
        "html/semantics/embedded-content/the-video-element/"
        "video-loading-lazy-to-eager.html",
    "replay-video-lazy-autoplay":
        "html/semantics/embedded-content/the-video-element/"
        "video-loading-lazy-autoplay-when-visible.html",
    "replay-media-currentsrc":
        "html/semantics/embedded-content/media-elements/"
        "location-of-the-media-resource/currentSrc.html",
    "replay-foreignobject-taint":
        "html/canvas/element/manual/drawing-images-to-the-canvas/"
        "drawimage_svg_image_with_foreign_object_does_not_taint.html",
    "replay-offscreen-filter-worker":
        "html/canvas/offscreen/manual/filter/offscreencanvas.filter.w.html",
    "replay-selection-nested-video": "selection/selection-nested-video.html",
    "replay-mediarecorder-destroy":
        "mediacapture-record/MediaRecorder-destroy-script-execution.html",
    "replay-iframe-marginwidth":
        "html/rendering/non-replaced-elements/the-page/"
        "iframe-marginwidth-marginheight.html",
}

#: Deliberately NOT replayable, and the reason belongs next to the table: a
#: `.sub.html` file carries `{{domains[www1]}}` / `{{ports[http][0]}}`
#: placeholders that only `wptserve`'s pipe substitution fills in, so a plain
#: server hands the page the literal braces and the replay would measure the
#: probe rather than the engine. `fetch/orb/tentative/status.sub.html`,
#: `content-security-policy/img-src/img-src-full-host-wildcard-blocked.sub.html`
#: and `svg/embedded/image-crossorigin.sub.html` are in this cluster and are
#: measured through the `canvas-taint-crossorigin` variant instead, whose
#: second origin is a real second port.

#: The `testharnessreport.js` tag, in the spellings WPT files use for it.
_REPORT_SCRIPT_RE = re.compile(
    r"<script[^>]*\bsrc\s*=\s*[\"\']?[^\"\'>]*testharnessreport\.js[\"\']?[^>]*>"
    r"\s*</script>", re.I)

_MAX_MARKERS = 44
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
#: Engine-side exception lines. The probe reports its own failures through
#: `rep(...)`, so anything the browser prints on top of that is a report the
#: page itself could not make.
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages and their assets, recording every request.

    `vrc-dyn-image.png` is answered by hand with `Cache-Control: no-cache`, so
    the request count for one URL is the measurement `img-cache-revalidate`
    needs — a second element pointed at an already-fetched URL either comes
    back to the server or it does not.
    """

    protocol_version = "HTTP/1.1"

    extensions_map = dict(http.server.SimpleHTTPRequestHandler.extensions_map)
    extensions_map[".mjs"] = "text/javascript"
    extensions_map[".ttf"] = "font/ttf"
    extensions_map[".webm"] = "video/webm"
    extensions_map[".svg"] = "image/svg+xml"

    #: Set per server instance: the label the recorded path is prefixed with,
    #: so the alternate-origin server's requests are told apart from the main
    #: one's in a single `served` list.
    origin_label = ""

    def _record(self, method):
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.origin_label}{self.path}")

    def do_GET(self):  # noqa: N802 — http.server's own casing
        self._record("GET")
        if self.path.split("?")[0].endswith("/vrc-dyn-image.png"):
            self.send_response(200)
            self.send_header("Content-Type", "image/png")
            self.send_header("Content-Length", str(len(GREEN_PNG)))
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()
            self.wfile.write(GREEN_PNG)
            return
        super().do_GET()

    def do_HEAD(self):  # noqa: N802
        self._record("HEAD")
        super().do_HEAD()

    def log_message(self, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root, label=""):
    """Start a background http server on `root`, return (port, shutdown).

    `label` prefixes every path this server records, so the alternate-origin
    server's requests stay distinguishable inside the single `SERVED` list
    that `canvas-taint-crossorigin` reads.
    """
    port = _free_port()

    class _Labelled(_Quiet):
        origin_label = label

    def handler(*args, **kwargs):
        return _Labelled(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds, page=None):
    """Launch one browser on one probe page; return (ticks, markers, served).

    `page` overrides the server-relative page path, which is what a replay
    needs: its copy lives next to the original test rather than in the WPT
    root, so that the test's own relative references still resolve.
    """
    log_path = os.path.join(REPO, ".tmp", f"vrc-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    page = page or f".vrc-{name}.html"
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
    with _SERVED_LOCK:
        served = [p for p in SERVED if "/.vrc-" not in p]
    return ticks, markers, served


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=8.0,
                        help="how long each page is allowed to run")
    parser.add_argument("--variant", action="append", default=None,
                        help="run only these variants (repeatable)")
    args = parser.parse_args()

    wanted = args.variant or (list(VARIANTS) + list(REPLAYS))
    unknown = [name for name in wanted
               if name not in VARIANTS and name not in REPLAYS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    http_port, shutdown = _serve(HERE)
    alt_port, alt_shutdown = _serve(HERE, label="[alt]")
    alt_origin = f"http://127.0.0.1:{alt_port}"
    written = []
    #: `name -> server-relative page path`, for the replays whose copy does not
    #: live in the WPT root.
    replay_pages = {}
    for name in wanted:
        if name in REPLAYS:
            source = os.path.join(HERE, *REPLAYS[name].split("/"))
            directory = os.path.dirname(source)
            path = os.path.join(directory, f".vrc-{name}.html")
            with open(source, encoding="utf-8", errors="replace") as handle:
                original = _REPORT_SCRIPT_RE.sub("", handle.read())
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(original + REPLAY_REPORTER)
            written.append(path)
            replay_pages[name] = (os.path.dirname(REPLAYS[name]) +
                                  f"/.vrc-{name}.html")
            continue
        path = os.path.join(HERE, f".vrc-{name}.html")
        body = VARIANTS[name][0].replace("__ALT_ORIGIN__", alt_origin)
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)
    # A real font file, so `fontface-events` measures the FontFaceSet rather
    # than a 404: Ahem is bundled for exactly this kind of determinism.
    font_src = os.path.join(REPO, "assets", "fonts", "Ahem.ttf")
    font_dst = os.path.join(HERE, "vrc-font.ttf")
    if os.path.exists(font_src):
        with open(font_src, "rb") as src, open(font_dst, "wb") as dst:
            dst.write(src.read())
        written.append(font_dst)

    try:
        print(f"{'variant':26s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, served = _run_variant(
                args.binary, name, http_port, args.seconds,
                page=replay_pages.get(name))
            seen = ", ".join(markers) if markers else "— nothing"
            if served:
                # Counted, not de-duplicated (slice 28): "was it fetched
                # again?" is a different question from "was it fetched", and
                # it is the whole of `img-cache-revalidate`.
                counts = collections.Counter(served)
                seen += "   [server saw: " + ", ".join(
                    (f"{path} x{n}" if n > 1 else path)
                    for path, n in sorted(counts.items())) + "]"
            else:
                seen += "   [server saw: nothing]"
            expected = (f"replay of {REPLAYS[name]}" if name in REPLAYS
                        else VARIANTS[name][1])
            print(f"{name:26s} {ticks:5d}  {expected:62s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed the event it armed is "
              "the shape every id in this cluster dies of — the element was "
              "put on screen and the engine never spoke about it again, so the "
              "test cannot even reach its first assertion. `server saw` is the "
              "independent half: a source missing there was never fetched "
              "(BUG-826 means the browser's own log cannot answer that), and a "
              "path listed twice is the only evidence that a second element "
              "re-requested an already-fetched URL.")
    finally:
        shutdown()
        alt_shutdown()
        for path in written:
            try:
                os.remove(path)
            except OSError:
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
