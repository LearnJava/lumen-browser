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
