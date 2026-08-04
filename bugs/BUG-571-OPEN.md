# BUG-571: dynamically-inserted `<script>` elements never execute

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs::collect_scripts_ordered` +
`run_scripts_with_dom`), js (`crates/js/src/dom.rs` — `HTMLScriptElement`
has IDL reflection only, no execution hook)
**Найден:** P2, WPT-VENDOR-html-semantics-scripting-1, 2026-08-04

## Симптом

`document.createElement('script')` (or `createElementNS` for SVG), followed
by setting `type`/`textContent`/`src` and `appendChild`-ing the element into
a live document, never runs the script — no exception, no side effect,
`window.ran` (or any variable the script would set) simply stays at its
pre-insertion value forever. Canonical repro from
`html/semantics/scripting-1/the-script-element/resources/script-type-and-language-js.js`:

```js
let script = document.createElement("script");
script.setAttribute("type", "text/javascript");
script.textContent = "window.ran = true;";
document.querySelector('#script-placeholder').appendChild(script);
assert_equals(window.ran, true);   // FAIL: got false
```

This single mechanism explains 218 of the 575 `FAIL` lines in
`script-type-and-language-js.html`/`.svg`/`.xhtml` alone (every "Script
should run with type=/language=..." case across the full legacy JavaScript
MIME-type and `language=` matrices — `is_classic_script_type()` at
`main.rs:6625` already implements the correct spec whitelist, but that
function is never consulted for a dynamically-created element because
nothing calls it after initial parse). The same root cause also accounts for
the category's `scheduler:`/ordering tests ("dynamically created external
script executes asynchronously", "Async script element execution delays the
window's load event", etc.) — anything that builds a `<script>` via the DOM
API rather than relying on the parser.

## Причина

Classic-script execution in Lumen is a **one-shot walk**, not a live
mechanism. `run_scripts_with_dom` (`main.rs:6936`) is called exactly once
per navigation (`main.rs:5228`), and internally calls
`collect_scripts_ordered` (`main.rs:6661`) which recursively walks the
already-parsed DOM tree exactly once, classifying every `<script>` node it
finds at that instant into `classic`/`modules` lists and then executing
them. There is no equivalent of the HTML "prepare the script element"
algorithm (HTML LS §8.1.3.1) hooked to node insertion — no
`MutationObserver`-style callback, no native binding fired from
`appendChild`/`insertBefore`/`replaceChild`. `HTMLScriptElement` in the JS
shim (`crates/js/src/dom.rs`, reflection installed at `dom.rs:13838`) is a
plain reflected-attribute wrapper with zero execution semantics attached to
insertion.

Consequently **every dynamically-created `<script>` element on any page is
inert**, regardless of `type`, `src`, `async`/`defer`, or insertion method.
This is one of the most common real-world script-loading patterns (lazy
analytics/ads loaders, dynamic polyfill loading, most bundler runtime
chunk-loading shims that don't go through `<script type=module>`), so the
practical impact reaches far beyond WPT.

## Масштаб

At least 60 files in `html/semantics/scripting-1/the-script-element/` alone
use `createElement("script")` + insertion (`grep -rl
'createElement("script")\|createElement(\'script\')'`). Within just the one
`script-type-and-language-js` fixture (shared by 3 test files —
`.html`/`.svg`/`.xhtml`): 218 subtests. Very likely the dominant cause of
the `scheduler:`/async-ordering failure cluster observed across the rest of
the category (not separately quantified here — those tests mix this defect
with legitimate ordering-semantics gaps).

Distinguish from [BUG-446](BUG-446-OPEN.md) (network-loaded *module* import
graph) and [BUG-568](BUG-568-OPEN.md) (`document.write()`) — both are about
different script-loading paths; this one is specifically "script created via
DOM API, whether classic or module, whether inline or `src`, is never
executed at all, in a page that has already finished its initial parse".
