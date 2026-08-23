#!/usr/bin/env python3
"""WPT-RUN-6 slice 29: the object model a page reads back — CSSOM, the SVG
DOM, and the interfaces a test constructs on its first line.

Slice 28 left 103 `unclassified` ids and no dominant directory, but the run's
*other* weakly-owned bucket is the same size and far better evidenced:
`script-error-swallowed` (103 ids, owner `BUG-591`) is a catch-all for "the
browser printed an exception nobody saw", and each of its lines names the
missing thing outright. Sorted by text, that bucket is not 103 unrelated
accidents — it is a handful of families:

    11  Cannot read properties of undefined (reading '<N>')   CSSOM rule lists
     7  Cannot read properties of undefined (reading 'baseVal')  SVG DOM
     6  Cannot read properties of undefined (reading 'length')   CSSOM rule lists
     4  CustomElementRegistry is not defined                  scoped registries
     3  XSLTProcessor is not defined                          XSLT
     2  StaticRange is not defined                            Highlight API
     2  DOMRect is not defined                                geometry

`BUG-591` was fixed on 2026-08-22, so that ownership is now stale twice over:
the exception is no longer swallowed, and it was never the *cause* — the cause
is the interface the page asked for. This probe measures each family directly,
so the bucket can be split into mechanisms that name their own bug, the way
slice 26 had to do for `mixed-content-blocked`'s wrong `ref`.

What the probe separates, per variant:

* CSSOM (`css/cssom/*`, `css/css-page/*`, `css/css-fonts/test_font_feature_*`,
  17 ids) — `document.styleSheets`, `<style>.sheet`, `sheet.cssRules`, the rule
  classes, `insertRule`, and the constructed-sheet path
  (`new CSSStyleSheet()` + `adoptedStyleSheets`, which
  `shadowrootadoptedstylesheets-fetched-module.html` needs). BUG-471 and
  BUG-746 both already claim `document.styleSheets`; which of them the ids
  belong to is a question this probe can answer only by measuring what is
  actually missing.
* SVG DOM (`svg/types/scripted/SVGLength-*.html`, 7 ids) — `text.x.baseVal[0]`
  is the exact expression those tests open with. Measured together with the
  neighbouring animated-value properties (`width`, `viewBox`, `transform`,
  `className`) and the `SVGLength` unit constants they assert against.
* Constructible interfaces a test needs before it can register a single
  `test()`: `StaticRange`/`Highlight`, `DOMRect`/`DOMPoint`/`DOMMatrix`,
  `CustomElementRegistry`, `XSLTProcessor`/`DOMParser`/`XMLSerializer`.
* Module scripts with a non-JS type (`css-module/*`, `text-module/*`, 6 ids) —
  `import sheet from "./x.css" with {type: "css"}` in both the static and the
  dynamic form, against a real served file, so "the attribute is rejected" is
  told from "the file was never fetched".
* The three `wasm/core/*.wast.js.html` ids, which are not an interface gap at
  all: they finish, in 16-31 s against the harness's 10 s. `wasm-decode`,
  `harness-cost`, `wasm-corpus` and `wasm-corpus-split` bisect that time from
  "the page is slow" down to one payload — a 32-byte module whose declared
  local count is unbounded (BUG-898) — by wrapping the harness's own entry
  points and `WebAssembly.validate`/`compile` and printing the bytes of any
  call over a millisecond. The isolated controls in `wasm-decode` are what
  make the attribution sound: the same API on ordinary payloads costs 0.02 ms.
* The residual's own smaller reads: the `ParentNode`/`ChildNode` mixin on the
  four node kinds (`dom/nodes/ParentNode-{append,prepend}.html`,
  `Node-insertBefore.html`), `HTMLCollection` liveness
  (`shadow-dom/leaktests/html-collection.html`), `select.add()`
  (`select-add.html`), `input.valueAsNumber`
  (`input-valueasnumber-invalidstateerr.html`) and `performance.measure`'s
  options form (`user-timing/measure*.html`).

Same harness as slices 15/17-22/24/25/26/27/28 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died", and a variant per hazard so one page that
freezes cannot hide its neighbours' measurements. Requests are recorded on the
probe's own server and printed with counts — the browser's log is not evidence
that anything was fetched (BUG-826), and de-duplicating them would erase the
difference between "fetched" and "fetched again" (slice 28).

Twenty-one variants. The four wasm ones need more than the default budget —
`--seconds 45` — because the page they replicate takes 25 s on purpose;
everything else settles inside 6 s.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_cssom_svg_interface_gaps.py
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

#: Paths the probe server was asked for, with the request method. A file that
#: never appears here was never fetched, whatever the page or the browser's
#: own log says (BUG-826).
SERVED = []
_SERVED_LOCK = threading.Lock()

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-29 probe: __NAME__</title>
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
#: rest of the variant. Without it a single missing global (`StaticRange`,
#: `DOMRect`) would hide every question after it — which is exactly the failure
#: mode the tests under measurement suffer from.
REPORT = """
<script>
function rep(label, fn) {
  try {
    var v = fn();
    console.log("PROBE " + label + " = " + v);
  } catch (e) {
    console.log("PROBE " + label + " THREW " + (e && e.message ? e.message : e));
  }
}
function has(name) { return typeof window[name] !== "undefined"; }
</script>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("vcsi-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, timeout, fetch-ok, load"),

    # `cssom-pagerule.html`, `cssom-ruleTypeAndOrder.html`,
    # `page-rule-declarations-00{0,1}.html` and
    # `test_font_feature_values_parsing.html` all open with
    # `document.styleSheets[0].cssRules[0]`, which is where the run's
    # `reading 'N'` / `reading 'length'` exceptions come from.
    "cssom-stylesheets": ("""
<style id=s1>
@page { margin: 1cm; }
@font-feature-values Foo { @styleset { nice: 1; } }
#a { color: rgb(1, 2, 3); }
@media screen { #b { color: green; } }
</style>
<link rel=stylesheet href="vcsi-sheet.css">
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("ss-typeof", function () { return typeof document.styleSheets; });
  rep("ss-length", function () { return document.styleSheets.length; });
  rep("ss-item0", function () { return String(document.styleSheets[0]); });
  rep("ss-cssRules", function () { return document.styleSheets[0].cssRules.length; });
  rep("style-sheet", function () { return String(document.getElementById("s1").sheet); });
  rep("link-sheet", function () {
    return String(document.querySelector("link[rel=stylesheet]").sheet);
  });
  rep("globals", function () {
    return ["StyleSheet", "CSSStyleSheet", "StyleSheetList", "CSSRuleList", "CSSRule",
            "CSSStyleRule", "CSSGroupingRule", "CSSMediaRule", "CSSPageRule",
            "CSSFontFaceRule", "CSSFontFeatureValuesRule", "CSSKeyframesRule",
            "MediaList", "CSSStyleDeclaration"]
      .filter(has).join(",") || "none";
  });
  rep("insertRule", function () {
    return typeof document.styleSheets[0].insertRule;
  });
  console.log("PROBE cssom-done");
});
</script>
""", "ss-typeof=object, ss-length>=2, cssRules, sheet on <style>/<link>"),

    # The constructed half of the same model. A declarative shadow root with
    # `adoptedStyleSheets` is what
    # `shadowrootadoptedstylesheets-fetched-module.html` builds.
    "cssom-constructed": ("""
<div id=host></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var sheet = null;
  rep("new-CSSStyleSheet", function () {
    sheet = new CSSStyleSheet();
    return typeof sheet;
  });
  rep("replaceSync", function () {
    sheet.replaceSync("#a { color: red }");
    return sheet.cssRules.length;
  });
  rep("doc-adopted", function () { return typeof document.adoptedStyleSheets; });
  rep("doc-adopted-set", function () {
    document.adoptedStyleSheets = [sheet];
    return document.adoptedStyleSheets.length;
  });
  var root = null;
  rep("attachShadow", function () {
    root = document.getElementById("host").attachShadow({mode: "open"});
    return typeof root;
  });
  rep("shadow-adopted", function () { return typeof root.adoptedStyleSheets; });
  rep("shadow-adopted-set", function () {
    root.adoptedStyleSheets = [sheet];
    return root.adoptedStyleSheets.length;
  });
  console.log("PROBE constructed-done");
});
</script>
""", "new CSSStyleSheet, replaceSync, adoptedStyleSheets on document+shadow"),

    # `registered-property-cssom.html` and `declared.tentative.html` read the
    # Typed OM maps; BUG-554 says the value classes exist but the maps may not.
    "typed-om": ("""
<div id=t style="width: 10px"></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var el = document.getElementById("t");
  rep("globals", function () {
    return ["CSSStyleValue", "CSSUnitValue", "CSSKeywordValue", "CSSNumericValue",
            "CSSUnparsedValue", "StylePropertyMap", "CSSTransformValue"]
      .filter(has).join(",") || "none";
  });
  rep("attributeStyleMap", function () { return typeof el.attributeStyleMap; });
  rep("computedStyleMap", function () { return typeof el.computedStyleMap; });
  rep("computed-get", function () { return String(el.computedStyleMap().get("width")); });
  rep("CSS-px", function () { return String(CSS.px(3)); });
  rep("registerProperty", function () {
    CSS.registerProperty({name: "--vcsi", syntax: "<length>",
                          inherits: false, initialValue: "2px"});
    return "ok";
  });
  rep("registered-readback", function () {
    return String(getComputedStyle(el).getPropertyValue("--vcsi")).trim();
  });
  console.log("PROBE typedom-done");
});
</script>
""", "attributeStyleMap/computedStyleMap, CSS.px, registerProperty"),

    # `svg/types/scripted/SVGLength-*.html` (7 ids) open with
    # `lh_test.x.baseVal[0]` — an SVGAnimatedLengthList on a `<text>`.
    "svg-length": ("""
<div id=ref style="font-family:initial; font-size:20px; width:10lh"></div>
<svg id=root width="100" height="50" viewBox="0 0 100 50">
  <text id=t x="10lh" style="font-family:initial; font-size:20px">x</text>
  <rect id=r x="5" y="6" width="7" height="8"/>
</svg>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var t = document.getElementById("t");
  var r = document.getElementById("r");
  var root = document.getElementById("root");
  rep("text-x", function () { return typeof t.x; });
  rep("text-x-baseVal", function () { return typeof t.x.baseVal; });
  rep("text-x-baseVal-0", function () { return String(t.x.baseVal[0].value); });
  rep("rect-x-baseVal", function () { return String(r.x.baseVal.value); });
  rep("rect-width-baseVal", function () { return String(r.width.baseVal.value); });
  rep("svg-width-baseVal", function () { return String(root.width.baseVal.value); });
  rep("viewBox-baseVal", function () { return String(root.viewBox.baseVal.width); });
  rep("globals", function () {
    return ["SVGLength", "SVGAnimatedLength", "SVGLengthList", "SVGAnimatedLengthList",
            "SVGRect", "SVGNumber", "SVGElement", "SVGSVGElement", "SVGTextElement",
            "SVGRectElement", "SVGTransform", "SVGMatrix", "SVGPoint"]
      .filter(has).join(",") || "none";
  });
  rep("unit-consts", function () {
    return String(SVGLength.SVG_LENGTHTYPE_PX) + "/" +
           String(SVGLength.SVG_LENGTHTYPE_UNKNOWN);
  });
  rep("createSVGLength", function () { return typeof root.createSVGLength(); });
  console.log("PROBE svglength-done");
});
</script>
""", "x.baseVal[0].value, width.baseVal, SVGLength constants"),

    "svg-dom": ("""
<svg id=root width="100" height="50">
  <g id=g class="a b" transform="translate(3,4)"><rect id=r width="7" height="8"/></g>
</svg>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var g = document.getElementById("g");
  var r = document.getElementById("r");
  var root = document.getElementById("root");
  rep("ownerSVGElement", function () { return String(r.ownerSVGElement === root); });
  rep("className-baseVal", function () { return String(g.className.baseVal); });
  rep("transform-baseVal", function () { return String(g.transform.baseVal.numberOfItems); });
  rep("getBBox", function () { var b = r.getBBox(); return b.width + "x" + b.height; });
  rep("getCTM", function () { return typeof r.getCTM(); });
  rep("getScreenCTM", function () { return typeof r.getScreenCTM(); });
  rep("instanceof-SVGElement", function () { return String(r instanceof SVGElement); });
  rep("root-instanceof", function () { return String(root instanceof SVGSVGElement); });
  rep("getBoundingClientRect", function () {
    var b = r.getBoundingClientRect(); return b.width + "x" + b.height;
  });
  console.log("PROBE svgdom-done");
});
</script>
""", "ownerSVGElement, className.baseVal, transform.baseVal, getBBox/getCTM"),

    # `Highlight-iteration{,-with-modifications}.html` die on `new StaticRange`
    # before registering a test. BUG-533 owns the constructor.
    "ranges-highlight": ("""
<p id=p>hello world</p>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var p = document.getElementById("p");
  rep("globals", function () {
    return ["Range", "StaticRange", "AbstractRange", "Highlight", "HighlightRegistry",
            "Selection", "CaretPosition"].filter(has).join(",") || "none";
  });
  var range = null;
  rep("createRange", function () {
    range = document.createRange();
    range.setStart(p.firstChild, 0);
    range.setEnd(p.firstChild, 5);
    return String(range);
  });
  rep("static-range", function () {
    var sr = new StaticRange({startContainer: p.firstChild, startOffset: 0,
                              endContainer: p.firstChild, endOffset: 5});
    return sr.startOffset + "-" + sr.endOffset;
  });
  rep("css-highlights", function () { return typeof CSS.highlights; });
  rep("new-Highlight", function () { return typeof new Highlight(range); });
  rep("highlight-set", function () {
    CSS.highlights.set("vcsi", new Highlight(range));
    return String(CSS.highlights.size);
  });
  rep("getSelection", function () { return typeof window.getSelection(); });
  console.log("PROBE ranges-done");
});
</script>
""", "StaticRange ctor, Highlight, CSS.highlights"),

    # `custom-elements/registries/*` (4 ids) construct a second registry:
    # `new CustomElementRegistry()`, then pass it to `createElement`
    # /`importNode`.
    "custom-registry": ("""
<div id=host></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("customElements", function () { return typeof window.customElements; });
  rep("global-ctor", function () { return typeof CustomElementRegistry; });
  rep("define", function () {
    class VcsiEl extends HTMLElement {
      connectedCallback() { console.log("PROBE ce-connected"); }
    }
    customElements.define("vcsi-el", VcsiEl);
    return "defined";
  });
  rep("upgrade-on-append", function () {
    document.getElementById("host").appendChild(document.createElement("vcsi-el"));
    return "appended";
  });
  rep("get", function () { return typeof customElements.get("vcsi-el"); });
  rep("whenDefined", function () { return typeof customElements.whenDefined; });
  rep("new-registry", function () { return typeof new CustomElementRegistry(); });
  rep("createElement-opts", function () {
    var reg = new CustomElementRegistry();
    return String(document.createElement("vcsi-el", {customElements: reg}).tagName);
  });
  rep("importNode-opts", function () {
    var reg = new CustomElementRegistry();
    return String(document.importNode(document.createElement("div"), {customElements: reg}));
  });
  console.log("PROBE registry-done");
});
</script>
""", "customElements.define/upgrade, new CustomElementRegistry()"),

    # `xml/xslt/*.window.html` (3 ids) — the whole family is one missing global.
    "xslt-xml": (REPORT + """
<script>
window.addEventListener("load", function () {
  rep("globals", function () {
    return ["XSLTProcessor", "DOMParser", "XMLSerializer", "XPathEvaluator", "XPathResult"]
      .filter(has).join(",") || "none";
  });
  rep("new-XSLTProcessor", function () { return typeof new XSLTProcessor(); });
  var doc = null;
  rep("DOMParser-xml", function () {
    doc = new DOMParser().parseFromString(
      "<root xmlns='urn:x'><kid a='1'>t</kid></root>", "text/xml");
    return doc.documentElement ? doc.documentElement.nodeName : "no-documentElement";
  });
  rep("xml-child", function () { return String(doc.documentElement.firstChild.nodeName); });
  rep("XMLSerializer", function () {
    return new XMLSerializer().serializeToString(doc).slice(0, 24);
  });
  rep("implementation", function () {
    return typeof document.implementation.createDocument("", "r", null);
  });
  rep("evaluate", function () { return typeof document.evaluate; });
  console.log("PROBE xslt-done");
});
</script>
""", "XSLTProcessor, DOMParser xml, XMLSerializer, createDocument"),

    # `DOMRect-nan.html` and friends. BUG-522 claims all four constructors.
    "geometry": (REPORT + """
<script>
window.addEventListener("load", function () {
  rep("globals", function () {
    return ["DOMRect", "DOMRectReadOnly", "DOMPoint", "DOMPointReadOnly",
            "DOMMatrix", "DOMMatrixReadOnly", "DOMQuad"].filter(has).join(",") || "none";
  });
  rep("new-DOMRect", function () { var r = new DOMRect(1, 2, 3, 4); return r.x + "," + r.width; });
  rep("DOMRect-fromRect", function () { return typeof DOMRect.fromRect({x: 1}); });
  rep("new-DOMPoint", function () { return String(new DOMPoint(1, 2).x); });
  rep("new-DOMMatrix", function () { return String(new DOMMatrix().is2D); });
  rep("gbcr-toJSON", function () {
    return typeof document.body.getBoundingClientRect().toJSON;
  });
  rep("gbcr-proto", function () {
    return Object.getPrototypeOf(document.body.getBoundingClientRect()).constructor.name;
  });
  console.log("PROBE geometry-done");
});
</script>
""", "DOMRect/DOMPoint/DOMMatrix ctors, getBoundingClientRect().toJSON"),

    # `dom/nodes/ParentNode-{append,prepend}.html` and `Node-insertBefore.html`
    # are in the `unclassified` residual, and
    # `dialog-focus-shadow-double-nested.html` printed
    # `div.shadowRoot.append is not a function`.
    "parentnode-mixin": ("""
<div id=host></div><div id=plain>text</div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var plain = document.getElementById("plain");
  var frag = document.createDocumentFragment();
  var root = document.getElementById("host").attachShadow({mode: "open"});
  rep("element-append", function () { plain.append("a", document.createElement("b")); return plain.childNodes.length; });
  rep("element-prepend", function () { plain.prepend("z"); return plain.firstChild.nodeValue; });
  rep("element-replaceChildren", function () { plain.replaceChildren("only"); return plain.textContent; });
  rep("document-append", function () { return typeof document.append; });
  rep("fragment-append", function () { frag.append(document.createElement("i")); return frag.childNodes.length; });
  rep("shadow-append", function () { root.append(document.createElement("i")); return root.childNodes.length; });
  rep("child-before", function () { return typeof plain.before; });
  rep("child-after", function () { return typeof plain.after; });
  rep("child-replaceWith", function () { return typeof plain.replaceWith; });
  rep("child-remove", function () { return typeof plain.remove; });
  rep("insertBefore-frag", function () {
    var f = document.createDocumentFragment();
    f.append(document.createElement("u"), document.createElement("u"));
    document.body.insertBefore(f, plain);
    return document.body.querySelectorAll("u").length;
  });
  rep("insertBefore-badarg", function () {
    try { document.body.insertBefore(document.createElement("u"), plain.firstChild); return "no-throw"; }
    catch (e) { return e.name; }
  });
  console.log("PROBE parentnode-done");
});
</script>
""", "append/prepend/replaceChildren on 4 node kinds, insertBefore w/ fragment"),

    # The other half of the SVG question: `svg.rs` attaches its typed classes by
    # re-pointing the prototype of an element built by `createElementNS`
    # (`crates/js/src/svg.rs:901-918`), and never runs their constructor. So the
    # instance fields those constructors declare (`x`, `width`, `viewBox`, ...)
    # exist on neither path, while the prototype methods exist on one of them.
    # Every WPT SVG test writes its markup in the file, i.e. takes the parser
    # path — which is why this variant is the control that names the mechanism.
    "svg-createns": ("""
<svg id=parsed width="100"><rect id=pr x="5" width="7" height="8"/></svg>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var NS = "http://www.w3.org/2000/svg";
  var root = document.createElementNS(NS, "svg");
  root.setAttribute("width", "100");
  root.setAttribute("viewBox", "0 0 100 50");
  var r = document.createElementNS(NS, "rect");
  r.setAttribute("x", "5"); r.setAttribute("width", "7"); r.setAttribute("height", "8");
  root.appendChild(r);
  document.body.appendChild(root);
  rep("created-instanceof", function () { return String(r instanceof SVGElement); });
  rep("created-ctor-name", function () { return Object.getPrototypeOf(r).constructor.name; });
  rep("created-getBBox", function () { var b = r.getBBox(); return b.width + "x" + b.height; });
  rep("created-getCTM", function () { return typeof r.getCTM(); });
  rep("created-x", function () { return typeof r.x; });
  rep("created-width-baseVal", function () { return String(r.width.baseVal.value); });
  rep("created-root-width", function () { return String(root.width.baseVal.value); });
  rep("created-viewBox", function () { return String(root.viewBox.baseVal.width); });
  rep("created-createSVGLength", function () { return typeof root.createSVGLength(); });
  rep("created-ownerSVGElement", function () { return String(r.ownerSVGElement === root); });
  rep("parser-ctor-name", function () {
    return Object.getPrototypeOf(document.getElementById("parsed")).constructor.name;
  });
  rep("parser-instanceof", function () {
    return String(document.getElementById("pr") instanceof SVGElement);
  });
  rep("parser-getBBox", function () {
    return typeof document.getElementById("pr").getBBox;
  });
  console.log("PROBE createns-done");
});
</script>
""", "createElementNS path: prototype yes, instance fields no"),

    # `shadow-dom/leaktests/html-collection.html` printed
    # `reading 'length'`; the live-collection accessors are the suspect.
    "collections": ("""
<form id=f><input name=i></form><img id=img src="vcsi-square.svg">
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("globals", function () {
    return ["HTMLCollection", "NodeList", "HTMLFormControlsCollection", "HTMLOptionsCollection",
            "NamedNodeMap", "DOMTokenList"].filter(has).join(",") || "none";
  });
  rep("doc-images", function () { return document.images.length; });
  rep("doc-forms", function () { return document.forms.length; });
  rep("doc-scripts", function () { return document.scripts.length; });
  rep("doc-links", function () { return typeof document.links; });
  rep("getElementsByTagName", function () { return document.getElementsByTagName("img").length; });
  rep("live-update", function () {
    var before = document.getElementsByTagName("img").length;
    document.body.appendChild(document.createElement("img"));
    return before + "->" + document.getElementsByTagName("img").length;
  });
  rep("children", function () { return document.body.children.length; });
  rep("namedItem", function () { return typeof document.forms.namedItem; });
  rep("form-elements", function () { return document.getElementById("f").elements.length; });
  console.log("PROBE collections-done");
});
</script>
""", "document.images/forms/scripts, live HTMLCollection, form.elements"),

    # `wasm/core/*.wast.js.html` (3 residual ids). The sweep of
    # `verify_layout_hangs.py` measured them at 31.2 s and 15.9 s standalone —
    # over the harness's own 10 s, hence TIMEOUT — and a stack sample taken
    # while `binary.wast.js.html` was running landed in
    # `lumen_js::wasm::parser::parse_module`. A sample is one snapshot, not a
    # profile, so this variant times the decode from the page instead: the
    # corpus is 18 KB of tiny modules, so a per-call cost in milliseconds is
    # the only way it can add up to half a minute.
    "wasm-decode": (REPORT + """
<script>
function bytes(str) {
  var a = new Uint8Array(str.length);
  for (var i = 0; i < str.length; i++) a[i] = str.charCodeAt(i);
  return a.buffer;
}
var VALID = "\x00\x61\x73\x6d\x01\x00\x00\x00";
window.addEventListener("load", function () {
  rep("has-WebAssembly", function () { return typeof WebAssembly; });
  rep("valid-x20", function () {
    var t = performance.now();
    for (var i = 0; i < 20; i++) WebAssembly.validate(bytes(VALID));
    return (performance.now() - t).toFixed(1) + "ms/20";
  });
  rep("malformed-short-x20", function () {
    var t = performance.now();
    for (var i = 0; i < 20; i++) {
      WebAssembly.validate(bytes(""));
      WebAssembly.validate(bytes("\x01"));
      WebAssembly.validate(bytes("\x00\x61\x73"));
    }
    return (performance.now() - t).toFixed(1) + "ms/60";
  });
  rep("huge-section-size", function () {
    var t = performance.now();
    WebAssembly.validate(bytes(VALID + "\x01\xff\xff\xff\xff\x0f"));
    return (performance.now() - t).toFixed(1) + "ms";
  });
  rep("compile-valid", function () {
    var t = performance.now();
    WebAssembly.compile(bytes(VALID)).then(function () {
      console.log("PROBE compile-resolved " + (performance.now() - t).toFixed(1) + "ms");
    }, function (e) { console.log("PROBE compile-rejected " + e); });
    return "started";
  });
  rep("instantiate-valid", function () {
    var t = performance.now();
    WebAssembly.instantiate(bytes(VALID)).then(function () {
      console.log("PROBE instantiate-resolved " + (performance.now() - t).toFixed(1) + "ms");
    }, function (e) { console.log("PROBE instantiate-rejected " + e); });
    return "started";
  });
  console.log("PROBE wasm-done");
});
</script>
""", "per-call decode cost: valid vs malformed vs oversized section"),

    # The same question asked of the real file. The probe's server has
    # `tests/wpt` as its root, so the corpus and its harness resolve at their
    # own absolute URLs — this is the actual page the run timed out on, minus
    # `testharnessreport.js` and wptrunner.
    "wasm-corpus": (REPORT + """
<script src="/resources/testharness.js"></script>
<script>window.__t0 = performance.now();
console.log("PROBE harness-loaded");</script>
<script src="/wasm/core/js/harness/async_index.js"></script>
<script>console.log("PROBE async-index " + (performance.now() - window.__t0).toFixed(0) + "ms");
window.__t1 = performance.now();</script>
<script src="/wasm/core/js/binary.wast.js"></script>
<script>console.log("PROBE corpus-body " + (performance.now() - window.__t1).toFixed(0) + "ms");</script>
<script>
window.addEventListener("load", function () {
  console.log("PROBE load-at " + (performance.now() - window.__t0).toFixed(0) + "ms");
});
</script>
""", "how long the real corpus page spends, phase by phase"),

    # Bisecting the 25 s of `wasm-corpus` into the two things every
    # `module()` call of `async_index.js` does besides decoding: taking a
    # stack trace (`new Error().stack`) and registering a `test()` with
    # `testharness.js`. Both are engine surfaces, and neither is
    # wasm-specific — whichever dominates here dominates any generated test
    # that registers hundreds of subtests.
    "harness-cost": ("""
<script src="/resources/testharness.js"></script>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("error-stack-x100", function () {
    var t = performance.now();
    for (var i = 0; i < 100; i++) { var s = new Error().stack.toString(); }
    return (performance.now() - t).toFixed(1) + "ms/100";
  });
  rep("test-register-x100", function () {
    var t = performance.now();
    for (var i = 0; i < 100; i++) { test(function () {}, "vcsi-" + i); }
    return (performance.now() - t).toFixed(1) + "ms/100";
  });
  rep("test-register-x100-again", function () {
    var t = performance.now();
    for (var i = 100; i < 200; i++) { test(function () {}, "vcsi-" + i); }
    return (performance.now() - t).toFixed(1) + "ms/100";
  });
  rep("promise-chain-x100", function () {
    var t = performance.now();
    var chain = Promise.resolve();
    for (var i = 0; i < 100; i++) chain = chain.then(function () {});
    return (performance.now() - t).toFixed(1) + "ms/100";
  });
  console.log("PROBE harness-cost-done");
});
</script>
""", "new Error().stack vs test() registration, 100 calls each"),

    # ...and the same page with `async_index.js`'s own entry points wrapped, so
    # the 25 s is attributed to a function rather than to the file. The corpus
    # calls only these, and each is a global function declaration of a classic
    # script, i.e. a writable property of `window`.
    "wasm-corpus-split": (REPORT + """
<script src="/resources/testharness.js"></script>
<script src="/wasm/core/js/harness/async_index.js"></script>
<script>
window.__cost = {};
["module", "instance", "assert_invalid", "assert_malformed", "assert_return",
 "assert_trap", "run", "call", "exports", "binary", "uniqueTest",
 "reinitializeRegistry"].forEach(function (name) {
  var fn = window[name];
  if (typeof fn !== "function") { window.__cost[name] = "absent"; return; }
  window.__cost[name] = [0, 0];
  window[name] = function () {
    var t = performance.now();
    try { return fn.apply(this, arguments); }
    finally {
      window.__cost[name][0] += performance.now() - t;
      window.__cost[name][1] += 1;
    }
  };
});
["validate", "compile", "instantiate"].forEach(function (name) {
  var fn = WebAssembly[name];
  window.__cost["WebAssembly." + name] = [0, 0];
  WebAssembly[name] = function () {
    var t = performance.now();
    try { return fn.apply(WebAssembly, arguments); }
    finally {
      var ms = performance.now() - t;
      window.__cost["WebAssembly." + name][0] += ms;
      window.__cost["WebAssembly." + name][1] += 1;
      // A payload that costs more than a millisecond is the finding: print
      // its length and its bytes so the bug report carries a repro rather
      // than an average.
      if (ms > 1 && name === "validate" && window.__slow_shown !== 3) {
        window.__slow_shown = (window.__slow_shown || 0) + 1;
        var v = new Uint8Array(arguments[0]), hex = "";
        for (var i = 0; i < Math.min(v.length, 24); i++)
          hex += (v[i] < 16 ? "0" : "") + v[i].toString(16) + " ";
        console.log("PROBE slow-validate " + ms.toFixed(0) + "ms len=" + v.length +
                    " bytes=" + hex);
      }
    }
  };
});
window.__cost["Error.stack"] = [0, 0];
var _origTest = window.test;
window.__t1 = performance.now();
</script>
<script src="/wasm/core/js/binary.wast.js"></script>
<script>
console.log("PROBE corpus-body " + (performance.now() - window.__t1).toFixed(0) + "ms");
(function () {
  var t = performance.now();
  for (var i = 0; i < 127; i++) { var s = new Error().stack.toString(); }
  console.log("PROBE replica-error-stack " + (performance.now() - t).toFixed(0) + "ms/127");
  t = performance.now();
  for (i = 0; i < 127; i++) { var b = new ArrayBuffer(8); }
  console.log("PROBE replica-buffer " + (performance.now() - t).toFixed(0) + "ms/127");
})();
Object.keys(window.__cost).forEach(function (k) {
  var c = window.__cost[k];
  if (c === "absent") { console.log("PROBE cost " + k + " absent"); return; }
  if (c[1]) console.log("PROBE cost " + k + " " + c[0].toFixed(0) + "ms/" + c[1]);
});
</script>
""", "which harness entry point the corpus spends its 25 s in"),

    # `select-add.html` (unclassified residual).
    "select-options": ("""
<select id=s><option value=a>A</option><option value=b>B</option></select>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var s = document.getElementById("s");
  rep("options-length", function () { return s.options.length; });
  rep("new-Option", function () { return typeof new Option("C", "c"); });
  rep("add", function () { s.add(new Option("C", "c")); return s.options.length; });
  rep("add-before", function () { s.add(new Option("D", "d"), 0); return s.options[0].value; });
  rep("remove", function () { s.remove(0); return s.options.length; });
  rep("item", function () { return typeof s.item; });
  rep("namedItem", function () { return typeof s.namedItem; });
  rep("selectedOptions", function () { return typeof s.selectedOptions; });
  rep("selectedIndex", function () { return String(s.selectedIndex); });
  rep("length-set", function () { s.length = 1; return s.options.length; });
  console.log("PROBE select-done");
});
</script>
""", "select.add/remove/item, new Option(), selectedOptions"),

    # `input-valueasnumber-invalidstateerr.html` (unclassified residual).
    "input-value": ("""
<input id=num type=number value=3>
<input id=date type=date value="2026-08-23">
<input id=text type=text value="hello">
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var num = document.getElementById("num");
  var date = document.getElementById("date");
  var text = document.getElementById("text");
  rep("num-valueAsNumber", function () { return String(num.valueAsNumber); });
  rep("date-valueAsNumber", function () { return String(date.valueAsNumber); });
  rep("date-valueAsDate", function () { return String(date.valueAsDate); });
  rep("text-valueAsNumber-throws", function () {
    try { text.valueAsNumber = 1; return "no-throw"; } catch (e) { return e.name; }
  });
  rep("text-valueAsDate-throws", function () {
    try { text.valueAsDate = new Date(); return "no-throw"; } catch (e) { return e.name; }
  });
  rep("stepUp", function () { num.stepUp(); return num.value; });
  rep("setRangeText", function () { text.setRangeText("X", 0, 1); return text.value; });
  rep("validity", function () { return typeof text.validity; });
  console.log("PROBE input-done");
});
</script>
""", "valueAsNumber/valueAsDate + InvalidStateError on text, stepUp"),

    # `user-timing/measure.html`, `measure_navigation_timing.html`,
    # `po-mark-measure.any.html` (3 unclassified residual ids).
    "user-timing": (REPORT + """
<script>
window.addEventListener("load", function () {
  rep("mark", function () { return typeof performance.mark("m1"); });
  rep("mark-detail", function () {
    var e = performance.mark("m2", {detail: {a: 1}, startTime: 12});
    return e ? (e.startTime + "/" + JSON.stringify(e.detail)) : "no-entry";
  });
  rep("measure-names", function () { return typeof performance.measure("a", "m1", "m2"); });
  rep("measure-options", function () {
    var e = performance.measure("b", {start: 0, end: 10, detail: {x: 1}});
    return e ? (e.duration + "/" + JSON.stringify(e.detail)) : "no-entry";
  });
  rep("measure-navtiming", function () {
    return typeof performance.measure("c", "navigationStart");
  });
  rep("getEntriesByType", function () {
    return performance.getEntriesByType("measure").length + "/" +
           performance.getEntriesByType("mark").length;
  });
  rep("entry-toJSON", function () {
    var e = performance.getEntriesByType("mark")[0];
    return e ? typeof e.toJSON : "no-entry";
  });
  rep("clearMarks", function () { performance.clearMarks(); return performance.getEntriesByType("mark").length; });
  rep("po-buffered", function () {
    var got = 0;
    new PerformanceObserver(function (list) { got += list.getEntries().length;
      console.log("PROBE po-fired " + got); }).observe({type: "mark", buffered: true});
    performance.mark("m3");
    return "observing";
  });
  console.log("PROBE usertiming-done");
});
</script>
""", "mark/measure options form, navigation-timing name, PO buffered"),

    # `css-module/*.html` and `text-module/*.html` (6 ids). The static form is
    # what the tests use; the dynamic one separates "attribute rejected" from
    # "module graph never started".
    "module-types": ("""
<script type=module>
console.log("PROBE module-ran");
import("./vcsi-mod.mjs").then(function (m) { console.log("PROBE dyn-js " + m.value); },
                              function (e) { console.log("PROBE dyn-js-err " + e); });
import("./vcsi-sheet.css", {with: {type: "css"}}).then(
  function (m) { console.log("PROBE dyn-css " + (m.default && m.default.cssRules ? m.default.cssRules.length : typeof m.default)); },
  function (e) { console.log("PROBE dyn-css-err " + e); });
import("./vcsi-data.json", {with: {type: "json"}}).then(
  function (m) { console.log("PROBE dyn-json " + JSON.stringify(m.default)); },
  function (e) { console.log("PROBE dyn-json-err " + e); });
</script>
<script type=module src="vcsi-static-css.mjs"></script>
""", "module-ran, dyn-js, dyn-css/json verdicts, server saw .css/.json"),
}

#: Files the probe pages reference. Kept as real served files on purpose: for
#: the module variants the only sound way to tell "the import attribute was
#: rejected" from "the module graph never started" is whether the server was
#: asked for the file at all.
ASSETS = {
    "vcsi-asset.js": "window.vcsiAsset = 1;\n",

    "vcsi-sheet.css": "#linked { color: rgb(4, 5, 6); }\n",

    "vcsi-data.json": '{"vcsi": 42}\n',

    "vcsi-mod.mjs": 'export const value = "plain-module";\n',

    # The static form the css-module tests use. A failure here is reported by
    # the browser as a `module error:` line, which the probe picks up like any
    # other stderr text.
    "vcsi-static-css.mjs": """import sheet from "./vcsi-sheet.css" with { type: "css" };
console.log("PROBE static-css " + (sheet && sheet.cssRules ? sheet.cssRules.length : typeof sheet));
""",

    "vcsi-square.svg": """<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
<rect width="20" height="20" fill="#0a0"/></svg>
""",
}

_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
#: Engine-side exception lines. The probe reports its own failures through
#: `rep(...)`, so anything the browser prints on top of that is a report the
#: page itself could not make — the very class of evidence this slice is
#: splitting out of `script-error-swallowed`.
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages and their assets, recording every request."""

    protocol_version = "HTTP/1.1"

    #: A `.css`/`.json` module must arrive with its real MIME type or the
    #: engine would be entitled to reject it for a reason that has nothing to
    #: do with the import attribute under measurement.
    extensions_map = dict(http.server.SimpleHTTPRequestHandler.extensions_map)
    extensions_map[".mjs"] = "text/javascript"
    extensions_map[".css"] = "text/css"
    extensions_map[".json"] = "application/json"

    def _record(self, method):
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.path}")

    def do_GET(self):  # noqa: N802 — http.server's own casing
        self._record("GET")
        super().do_GET()

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
    log_path = os.path.join(REPO, ".tmp", f"vcsi-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    page = f".vcsi-{name}.html"
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
        served = [p for p in SERVED if "/.vcsi-" not in p]
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
        path = os.path.join(HERE, f".vcsi-{name}.html")
        body = VARIANTS[name][0].replace("__ORIGIN__", origin)
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
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
                # Counted, not de-duplicated (slice 28): "was it fetched
                # again?" is a different question from "was it fetched".
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
              "live page (ticks > 0) whose `rep(...)` line says THREW or "
              "`undefined` is missing the interface the test under measurement "
              "opens with, and a test built on it can only TIMEOUT — it dies "
              "before registering a single `test()`, so `testharness.js` never "
              "publishes a status. `server saw` is the independent half: a "
              "module file missing there was never fetched, so the import "
              "attribute was rejected before the graph started (BUG-826 means "
              "the browser's own log cannot answer that).")
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
