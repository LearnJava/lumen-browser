# BUG-493: `getComputedStyle()` reads a stale/absent style snapshot for DOM
mutations made earlier in the same script execution — no synchronous
style/layout flush before the read

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js/layout boundary (`crates/js/src/v8_runtime.rs::_lumen_get_computed_style`
+ `update_computed_styles`, `crates/driver/src/session.rs:379`)
**Найден:** WPT-RUN-3 срез 8 (`ROADMAP.md`) — массовый прогон `css/css-borders`

## Механизм

`_lumen_get_computed_style(nid, prop)` (`v8_runtime.rs:3146`) is a pure
cache read against `computed_styles: Arc<Mutex<HashMap<u32,
HashMap<String,String>>>>` — it never triggers layout itself. That cache is
only refreshed by an external call to `update_computed_styles(...)`
(`v8_runtime.rs:651`), fed from `lumen_layout::collect_computed_styles`
(`crates/driver/src/session.rs:379`, called after each `InProcessSession`
mutation per DEVX-9) or from equivalent call sites in the live-window shell
(`crates/shell/src/main.rs`, several `update_computed_styles` call sites tied
to frame/paint-cycle boundaries). None of these refresh points fire
*synchronously in response to a `getComputedStyle()` call* — real browsers
are spec-required to force a synchronous style + layout recalculation the
moment script reads a computed value, precisely so a script can mutate a
style and immediately observe the new resolved value in the same turn.
Lumen instead resolves `getComputedStyle()` against whatever snapshot last
happened to be pushed, which for freshly-created-and-mutated nodes can be
*no entry at all* (`HashMap::get` → `None` → `.unwrap_or_default()` → `""`
via the JS-side `|| ''` fallback in `dom.rs:12780`/`12789`), not merely an
outdated value.

Reproduced live (`--mcp-port`, minimal isolation, unrelated to any
css-borders-4-specific gap):

```js
// existing, already-styled element, mutated then read in the SAME eval call:
var d = document.getElementById('static-target'); // outline: solid 2px purple
d.style.outlineWidth = '9px';
getComputedStyle(d).outlineWidth   // → "2px" (stale — the pre-mutation value)

// freshly created element, styled+appended+read in the SAME eval call:
var d2 = document.createElement('div');
d2.style.outlineWidth = '1px'; d2.style.outlineStyle = 'solid';
document.body.appendChild(d2);
getComputedStyle(d2).outlineWidth  // → "" (no entry at all, not even stale)

// same sequence split across TWO eval calls (crosses a DEVX-9 relayout boundary):
window.__d = document.createElement('div');
__d.style.outlineWidth = '1px'; __d.style.outlineStyle = 'solid';
document.body.appendChild(__d);
// -- eval call boundary here --
getComputedStyle(window.__d).outlineWidth  // → "1px" (correct, once relayout ran)
```

`d.style.outlineWidth = '...'` itself is confirmed working in all three cases
(`_lumen_make_style`'s `setProperty` path, not the separate `[PutForwards=
cssText]` gap — [BUG-494](../bugs/BUG-494-OPEN.md)) — the only variable is
whether a relayout happened to run between the mutation and the read.

## Симптом

`border-width-rounding.tentative.html` synchronously does
`document.createElement` → `div.style = "..."` → `document.body.appendChild`
→ `test(() => assert_equals(getComputedStyle(div).outlineWidth, expected))`,
all inside one `<script>` block with no `await`/task boundary between
creation and the assertion — `outline-width` **is** present in
`computed_style_to_map` (`selector_query.rs:837`, unlike `border-width` the
shorthand — see [BUG-472](../bugs/BUG-472-OPEN.md) for that half of this same
file's failures), so its failures trace specifically to this bug: 11
`outline-width: ...` subtests, all `expected "<N>px" but got ""`. This is
the file's *only* signal for this bug in the current slice — most WPT tests
query `getComputedStyle()` against a pre-existing markup element (`#target`)
rather than a same-tick freshly created one, so this gap's true blast radius
across `css/` is unmeasured; flagged here as a first observation, the same
way BUG-488 was in slice 7.

## Масштаб находки

Not scoped beyond this slice's one file — likely affects any WPT test (in
any category) that creates+styles+appends a node and reads its computed
style back in the same synchronous script turn, but this wasn't surveyed
beyond `css/css-borders`.

## .ini

Committed `.ini` for the `outline-width: ...` subtests of
`border-width-rounding.tentative.html` (the `border-width: ...` subtests in
the same file are [BUG-472](../bugs/BUG-472-OPEN.md) instead — same `.ini`
file, single header referencing both bugs since `wptmanifest` `.ini` doesn't
support per-subtest attribution comments).

## Срез 10 (`css/css-variables`, 2026-08-02) — massively broader blast radius, confirmed decisively

This slice's mass run against `css/css-variables` (a category whose ~50-year-old
Microsoft-authored test files overwhelmingly use the pattern "static markup +
a plain top-level `<script>` immediately after it that calls
`getComputedStyle()`, no `createElement`, no explicit relayout trick") showed
the *same* empty-string symptom on dozens of files — proving this bug's
mechanism is not limited to same-tick create+mutate+read (the original
`border-width-rounding.tentative.html` finding), but applies equally to the
much more common "static element, script runs during initial page parse"
case. Root-caused decisively via a live `--mcp-port` A/B probe:

```js
// probe-cv2.html: <div id="t1" style="--x: 20px; width: var(--x);">, followed
// immediately by an inline <script> (i.e. exactly the WPT idiom above):
<script>
  window.__capturedWidth = getComputedStyle(document.getElementById('t1')).getPropertyValue('width');
</script>
```

```
window.__capturedWidth (read via a LATER, separate eval() call)     → ""      (captured DURING the page's own synchronous inline script)
getComputedStyle(...).getPropertyValue("width") (separate eval call, AFTER navigate() returned) → "20px"  (correct — var() substitution genuinely works once the cache is populated)
```

This proves the `computed_styles` cache is populated only as a **post-navigation**
step in the `InProcessSession`/`--mcp-port` path (i.e., *after* `navigate()`
— and therefore after all of the page's own inline `<script>` tags have
already run) — never synchronously in response to layout completing during
the page's own script execution, and *not* by `document.documentElement.offsetHeight`
either (`variable-animation-from-to.html` explicitly forces this before its
assertions and still reads `""` for a standard, map-present property).
Consequently, **any** WPT test — in `css/css-variables` or any other
category — that reads `getComputedStyle()` synchronously from a `<script>`
block executed at initial parse time, without an explicit macrotask/event
boundary (e.g. `load`, a later separate script, `setTimeout`), will observe
this gap. This is now confirmed as the dominant root cause across most of
`css/css-variables`'s failures this slice (compounding with
[BUG-472](BUG-472-OPEN.md) for properties additionally missing from the map,
and with [BUG-499](BUG-499-OPEN.md) for custom-property reads specifically) —
see files listed in that category's `.ini` headers for the full list; too
numerous to enumerate here (roughly two dozen files).

## Срез 12 (`css/css-logical`, 2026-08-02) — extends to `offsetWidth`/`clientWidth`, not just `getComputedStyle()`

`logicalprops-{block,inline}-size{,-vlr}.html` (4 files, 32 subtests) call
`checkLayout(".block"/".override"/".tablecell", false)` synchronously in a
plain top-level `<script>` right after the page's own markup — the
`check-layout-th.js` helper reads `node.offsetWidth`/`.clientWidth` (not
`getComputedStyle()`) for each element. Every reading comes back `0`
instead of the correct, already-laid-out value (e.g. `width expected 600
but got 0`). Root-caused by reading the accessor chain directly:
`offsetWidth`/`clientWidth`/`clientHeight` (`crates/js/src/dom.rs:6190-6195`)
all resolve through `_lumen_get_bounding_rect(nid)` →
`v8_runtime.rs:3021`, a lookup into `layout_rects: Arc<Mutex<HashMap<u32,
[f32;4]>>>` — the exact same "externally-pushed snapshot, never
synchronously refreshed on read" shape as `computed_styles`, just a
different cache instance backing a different accessor family. Confirms
this bug's mechanism is not specific to `getComputedStyle()`'s cache, but
architectural: **no DOM geometry/style accessor in this engine forces a
synchronous layout flush before reading**, matching this bug's title
exactly. `logicalprops-with-variables-revert.html` (6 subtests) is a fifth
file for the same reason: `getComputedStyle(el).getPropertyValue('padding-top'
/'margin-left'/...)` — all **physical**, already-mapped properties (rules
out [BUG-472](BUG-472-OPEN.md), which only explains missing *logical*
property names) — read `""` instead of the value set by a stylesheet rule
(`@layer`+`revert-layer`+`var()`) evaluated at page-parse time, the
generic "static markup, script runs during initial parse" idiom already
established as this bug's dominant symptom in срез 10. `.ini` under
`tests/wpt/metadata/css/css-logical/` for all 5 files, `expected: FAIL`
per subtest.
