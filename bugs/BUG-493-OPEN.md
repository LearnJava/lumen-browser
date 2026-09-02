# BUG-493: `getComputedStyle()` reads a stale/absent style snapshot for DOM
mutations made earlier in the same script execution — no synchronous
style/layout flush before the read

**Статус:** OPEN
**Тип:** доработка (нереализованная функциональность), не дефект — ведётся как задача [`CSSOM-4`](../ROADMAP.md) дорожки CSSOM, а не как строка очереди P3. Файл остаётся детальной записью наблюдений: «срезы» ниже — прогоны категорий WPT, упиравшиеся в эту же дыру, а не куски выполненной работы. Переклассифицировано 2026-08-28 по решению пользователя.
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
cssText]` gap — [BUG-494](../bugs/BUG-494-FIXED.md), fixed 2026-09-02) — the only variable is
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

## Срез 15 (`css/css-easing`, 2026-08-02) — a clean, animation-free minimal repro of the `offsetWidth` sub-case, plus a new WAAPI-`currentTime` variant

**`offsetWidth` sub-case, re-confirmed and isolated further**:
`step-jump-{both,end,none,start}.html` (4 files, 24 subtests) each declare 7
`.test` divs with `animation: ... paused; animation-delay:
calc(var(--at) * -1s)` and a plain top-level `<script>checkLayout(".test")
</script>` right after the markup — every `node.offsetWidth` read comes back
`0` regardless of `--at`, including the case (`--at: 3.0`) where the
animation has already finished and the expected value is simply the
element's own static `width: 10px` declaration (not even an animated
value). Re-verified with a minimal, animation-free scratch page
(`.tmp/sync_width_probe.html`: one `<div class="box">` with a plain
`width: 123px` rule, no `@keyframes`/no JS mutation at all, followed
immediately by `<script>window.__result =
document.getElementById('d1').offsetWidth;</script>`) via a headless
`--mcp-port` probe: `window.__result` (captured during the page's own
synchronous inline script) is `0`; the identical `offsetWidth` read via a
*separate*, later `eval()` call is `123`. This isolates the mechanism
completely from CSS Animations/`animation-delay`/`calc()`/`var()` — it is
purely "no DOM geometry accessor forces a synchronous layout flush before
reading", exactly as срез 12 established for `checkLayout`, now confirmed
on a page with zero animation-related CSS at all.

**New WAAPI-`currentTime` variant**: `step-timing-functions-output.html`
(13 subtests), `cubic-bezier-timing-functions-output.html` (4), and
`linear-timing-functions-output.html` (5) all follow the pattern `var anim =
target.animate([...], {...}); anim.currentTime = N;
assert_equals(getComputedStyle(target).left, "...")` — mutating a Web
Animations API `currentTime` and reading `getComputedStyle()` back in the
same synchronous script tick, no task boundary. Same architectural gap as
the DOM-mutation case already on file (`d.style.x = ...; getComputedStyle
(d).x` from срез 8), just triggered through `Animation.currentTime`'s
setter instead of `CSSStyleDeclaration.setProperty`. In the two `*-output.html`
files the empty-string result additionally crashes the test rather than
just failing an assertion: `testcommon.js::pxToNum` does
`String(str).match(/^(-?[\d.]+)px$/)[1]`, and matching an empty string
against that regex returns `null`, so `null[1]` throws `Cannot read
properties of null (reading '1')` — explaining the JS-exception-shaped
failures instead of ordinary `assert_equals` messages seen on these two
files.

46 subtests / 6 files this slice, the largest single-slice addition to this
bug to date. `.ini` under `tests/wpt/metadata/css/css-easing/` for all 6
files, `expected: FAIL` per subtest.

## Срез 16 (`css/css-forms`, 2026-08-02) — new masking facet: comparing two empty reads against each other passes falsely

`appearance-base-basic.html` (10 subtests, one per form-control element)
compares `getComputedStyle(el).X` against `getComputedStyle(document.body).X`
for five inherited properties (`color`, `font-size`, `font-family`,
`font-weight`, `font-style`), all read inside the same synchronous top-level
`<script>` right after the page's markup — no task boundary, the exact idiom
this bug already covers. Both sides of each `assert_equals` return `""`
(cache not yet populated), so all five inherited-property assertions **pass
by accident** instead of failing — only the sixth assertion in the same
`test()`, `assert_equals(style.boxSizing, "border-box", ...)`, compares
against a *literal* rather than another empty read, and that's the one that
actually surfaces as `FAIL` (`expected "border-box" but got ""`). Live
`--mcp-live-port` A/B probe against the same markup confirms: same-tick read
→ `"content-box"` for `boxSizing` (not the empty string wptrunner's log
shows) is what a *populated* cache would answer wrong-but-non-empty (`appearance:
base`'s box-sizing reset isn't implemented — a separate, un-filed gap, out of
scope here since it's masked underneath this bug); a *post-navigate* separate
`eval()` reads `"content-box"` consistently too, confirming the wptrunner-vs-
probe discrepancy is entirely explained by this bug's synchronous-read
mechanism, not a second bug. This is the same failure-masking shape already
noted for the WAAPI-`currentTime` variant in срез 15, generalized: **any**
assertion in this bug's window that happens to compare two equally-empty
reads to each other, rather than to a literal, silently passes and hides
this bug's presence — only assertions against a literal or a value obtained
outside the affected window surface it. 10 subtests / 1 file this slice.
`.ini` under `tests/wpt/metadata/css/css-forms/appearance-base-basic.html.ini`,
`expected: FAIL` per subtest.

## Срез 19 (`css/css-nesting`, 2026-08-03)

9 files, 12 subtests — plain top-level-script instances of the same gap, no
new masking shape. Each file's first assertion reads `getComputedStyle()`
(`color`/`zIndex`/a custom-property-via-`--x` combined with a standard
property) synchronously right after the `<style>`+markup that defines the
value, and gets `""` instead of the real computed value; every one of these
files would pass if the same assertion ran from a second, separate `eval()`
call (not verified per-file this slice, per the established mechanism —
see [[project_wpt_run3_css_easing_slice15_landed]] et al.). Two files worth
noting: `invalidation-003.html`/`invalidation-004.html` have a *second*
assertion later in the same `test()` that would exercise
[BUG-471](BUG-471-OPEN.md) (`document.styleSheets[0].rules[...]`) — never
reached, since the first assertion throws first; `.ini` attributes both
files to this bug only. `.ini` under `tests/wpt/metadata/css/css-nesting/`
for all 9 files.

## Срез 26 (`css/css-ruby`, 2026-08-03)

`br-clear-all-000.html`/`br-clear-all-001.html`/`br-clear-all-002.html` —
same `check-layout-th.js` idiom (`checkLayout("#container")` called
synchronously right after the static markup). Confirmed via
`lumen --dump-layout` on the same markup that `#container`'s real computed
height is non-trivial (a forced layout pass produces a real, if imperfect,
box), while the JS-side same-tick read the test relies on gets `0` — the
established same-tick-cache signature, not a float/clear layout bug. 1
subtest/file, 3 files. `.ini` under `tests/wpt/metadata/css/css-ruby/`.

## Срез 33 (`css/css-sizing`, 2026-08-03) — a whole feature test suite
(`contain-intrinsic-size/auto-*.html`, "Last Remembered Size") turned out to
be this bug wearing a costume, not a missing feature

Largest single-slice addition yet: 578 subtests. Two file families, both
confirmed by inspecting the actual assertion code rather than the failure
message alone (important — the naive read is misleading here):

**`contain-intrinsic-size/auto-{006,008,009,011,014,015,016,017,018}.html`**
("Last Remembered Size" — CSS Sizing 4's `contain-intrinsic-size: auto
<fallback>` + `ResizeObserver`-driven size ratchet) produce messages like
`"Sizing normally - clientWidth expected 100 but got 0"`, which reads like
"the Last Remembered Size feature isn't implemented". It **is not that** —
`auto-006.html`'s `promise_test` does
`target.classList.remove("skip-contents"); checkSize(100, 50, "Sizing
normally");` with the assertion **synchronously immediately after** the
class mutation, no `await`/task boundary — exactly this bug's dominant
symptom (`checkSize` calls `target.clientWidth`/`clientHeight`, both
confirmed by срез 12 to share the same unflushed-cache mechanism as
`getComputedStyle`). Before attributing this cluster, a plain-code repro was
built and probed live (`--mcp-live-port`, `.tmp/repro_maxcontent.html`): a
`div#target{width:max-content}` containing `div#contents`, mutate
`#contents`'s size synchronously, read `#target.clientWidth` in the same
eval call — stays at the pre-mutation value, confirming the mechanism
without any Last-Remembered-Size-specific CSS at all. **Do not file a
separate "Last Remembered Size unimplemented" bug for this shape in a future
slice** — verify whether the failing assertion has a task boundary
(`await`/`requestAnimationFrame`/promise resolution) between the mutation
and the read first; only a genuinely async-scheduled read that still gets a
stale value would indicate a real Last-Remembered-Size gap.

**`aspect-ratio/abspos-aspect-ratio-border.html` +
`aspect-ratio-automatic-minimum.html`** (9 subtests) — same mechanism, the
other established idiom (срез 10): static markup + a plain top-level
`<script>` that calls `test(() => { var d = dims(id); assert_equals(d.w,
400, ...) })` synchronously at parse time, reading `offsetWidth`/
`offsetHeight` with no mutation involved at all — every one of the nine
`aspect-ratio`+abspos/border/padding combinations resolves to `0` for
exactly this reason, not because `aspect-ratio` is unhonored on absolutely-
positioned boxes (which would be a real, separate layout bug worth its own
report — ruled out only by checking `dims()`'s call site has zero task
boundary before it, same discipline as above).

`.ini` under `tests/wpt/metadata/css/css-sizing/contain-intrinsic-size/` and
`tests/wpt/metadata/css/css-sizing/aspect-ratio/`. Five files/~10 subtests
this slice did NOT fit this bug (`auto-004.html`'s `expected 1 but got 50`,
`replaced-element-transferred-size-flex.html`'s three subtests,
`replaced-element-028.html`, `quirks-mode-003.html`) — left as unclustered
residual, genuinely distinct numeric mismatches rather than empty/zero
reads.

## Срез CSS2 normal-flow (absorbed from [BUG-468](BUG-468-DUPLICATE.md), P3 2026-09-01)

BUG-468 (`css/CSS2/normal-flow/containing-block-percent-{padding,margin}-{left,right,top,bottom}.html`,
8 files, filed WPT-RUN-3 срез 2 — earlier in the same run than this bug's
срез 8, but merged here rather than the reverse: see BUG-468's own file for
why) is the same mechanism wearing a percentage-resolution costume. Pattern:

```html
<div id="container" style="width:123px;">
  <div id="child"></div>  <!-- CSS: padding-left:10%; width:50px; height:100px -->
</div>
<script>
  document.body.offsetTop;
  container.style.width = "500px";
  checkLayout("#container");   // check-layout-th.js reads child.offsetWidth
</script>
```

Live-probed (`--dump-layout` with `console.log` either side of the mutation,
independent of any WPT harness): `child.offsetWidth` reads `62.3` (correct
pre-mutation value: `50 + 10% of 123`) both *before and after* the
`style.width` mutation — the percentage is not "measuring 0" as BUG-468's
original filing guessed, it is the same stale pre-mutation snapshot срез 12
already documented for `offsetWidth`/`clientWidth`, just observed through a
percentage-dependent property this time.

**New confirmation this slice: the staleness is not percentage-specific at
all.** A percent-free control (`<div style="width:50px">` mutated to
`width:300px`, read back same-tick) shows the identical symptom —
`offsetWidth` still reports `50`. This rules out any percentage-resolution
logic as a contributing factor and reinforces срез 12's conclusion at full
strength: **no DOM geometry/style accessor in this engine forces a
synchronous layout flush before reading**, full stop, regardless of what
CSS feature produced the value being read. 8 files / 8 subtests this slice.
`.ini` already correctly `expected: FAIL` under
`tests/wpt/metadata/css/CSS2/normal-flow/` (unmodified, filed against
BUG-468 originally, mechanism identical).
