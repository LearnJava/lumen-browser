# BUG-555: `getComputedStyle()` returns an empty string for every property when queried from a synchronous inline `<script>` that runs during the initial HTML parse (before the page's first layout snapshot is published) — residual case of BUG-382

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** shell (`crates/shell/src/main.rs::apply_loaded_page` and the
`update_layout_rects`/`update_computed_styles` publish call inside it), js
(`crates/js/src/dom.rs:12769` `window.getComputedStyle`) — same subsystem as
[BUG-382](bugs/BUG-382-FIXED.md)
**Найден:** P2, WPT-RUN-3 срез 38 (`css/css-layout-api`), 2026-08-04 — прямая
проба через `--mcp-live-port` (`.tmp/probe3.html`, `.tmp/probe4.html`), после
того как реальный прогон `at-supports-rule.https.html`/
`computed-style-layout-function.https.html` (недостижимый прогону из-за
HTTPS-порт-гэпа — см. `.ini` в этой же категории) был воспроизведён вручную
поверх обычного HTTP и дал `assert_equals: expected "..." but got ""` для
`getComputedStyle(...)` на статичном элементе со статичным правилом стиля.

## Mechanism

BUG-382 was fixed by publishing the `update_layout_rects`/
`update_computed_styles` snapshot unconditionally right after the page's
`layout_box` is set in `apply_loaded_page`, verified with 6/6 clean live
windows reading `getComputedStyle` **after `wait{document_ready}`**. That
verification protocol only exercises scripts that run on/after `load` — it
does not cover a `<script>` that executes **during** the initial HTML parse
(the normal execution model for any inline `<script>` not deferred/async: it
runs synchronously the moment the parser reaches it, before the rest of the
document — and therefore before the page's first layout pass can possibly
have completed). Lumen's `getComputedStyle` does not force a synchronous
style/layout recalculation on access (the CSSOM spec's "flush pending
changes" requirement); it only reads whatever snapshot the engine thread has
already pushed via `update_computed_styles`. For a script executing mid-parse
that snapshot does not exist yet, so every property reads back as `""`
instead of either the specified value or the property's initial value.

This is deterministic, not the flaky ~3/4 race BUG-382 described for
onload-timed reads — confirmed by two independent reproductions:

```html
<style>#x{color:red}</style>
<div id="x"></div>
<script>
window.__inline_color = getComputedStyle(document.getElementById('x')).color;  // ""
window.addEventListener('load', function() {
  window.__load_color = getComputedStyle(document.getElementById('x')).color;  // "rgb(255, 0, 0)"
});
</script>
```

`window.__inline_color` and `window.__load_color` above differ every single
run: `["", "rgb(255, 0, 0)"]`. Same element, same property, same script —
only the timing of the read (mid-parse vs. `load`) changes the result.

This exactly explains two of the `css/css-layout-api` subtest failures found
this slice when the same test files were re-run manually over plain HTTP
(bypassing the unrelated HTTPS-port gap that hides them from the real
`wptrunner` run):

* `at-supports-rule.https.html`: `assert_equals: expected "\"pass\"" but got
  ""` — the test's `test(function(){...})` callback runs synchronously as
  part of the page's own `<script>`, reading `getComputedStyle(element)
  .content` before any layout snapshot exists.
* `computed-style-layout-function.https.html`: five `assert_equals` failures,
  every single one `expected "<real value>" but got ""` (`"layout(test1)"`,
  `"block"`, `"layout(test4)"`) — same synchronous-read pattern, unrelated to
  the `display: layout()` value under test (any property would fail the same
  way; `display`/`content` are just what this test happens to check).

## Impact

Any WPT (or real-world) page whose `<script>` runs `getComputedStyle` before
`load`/`DOMContentLoaded` — i.e. the ordinary "script right after the styled
markup" idiom used throughout `testharness.js`'s simple synchronous `test()`
pattern — gets silent empty-string results instead of real values or a
thrown error. `parseFloat("") === NaN` and `"" !== expected` fail quietly
further down the stack. Given how common this idiom is across the vendored
`css/` corpus, this likely explains a nontrivial slice of already-filed
per-property/per-category bugs in other WPT-RUN-3 slices that were
attributed to a missing/broken individual CSS feature rather than to this
shared, systemic timing gap — worth a targeted re-triage pass (grep prior
wptreport JSONs for `expected "..." but got ""` for various properties) once
this is fixed, rather than assuming each such case is unrelated.

## Secondary, minor finding in the same slice (not filed separately)

`inline-style-layout-function.https.html` shows `element.style.display`
accepting syntactically invalid `layout()` values without any grammar
validation (`layout()` with no name, `layout(test3, invalid)` with an extra
argument) — the setter stores whatever string looks like a `layout(...)`
call rather than validating it against the (Houdini, post-Phase-2, per
`CSS-SPECS.md:137`) custom-layout grammar. Low priority since CSS Layout API
itself is not planned for this phase; noted here in case whoever eventually
implements it wants the pointer.

## How to fix

1. Either force a synchronous style/layout recompute inside
   `window.getComputedStyle` when no snapshot is available yet for the
   queried node (spec-correct, but touches the engine-thread/JS boundary
   contract from ADR-016/ADR-023), or publish an initial snapshot
   synchronously as soon as the first layout pass for a fresh navigation
   completes — before resuming/continuing script execution for that same
   navigation — rather than only after full-document `apply_loaded_page`.
2. Regression gate per BUG-382's own advice: read `getComputedStyle` from an
   inline `<script>` positioned immediately after a styled element (not from
   a `load`/`DOMContentLoaded` handler) and require a non-empty value,
   several fresh runs in a row — a single green run proves nothing given the
   race BUG-382 already documented for the onload-timed case.
