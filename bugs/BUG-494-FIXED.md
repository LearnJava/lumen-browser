# BUG-494: `element.style = "css text"` (whole-string assignment) is a
silent no-op — missing Web IDL `[PutForwards=cssText]` forwarding

**Статус:** FIXED 2026-09-02
**Дата:** 2026-08-02
**Компонент:** js shim (`crates/js/src/shim/web_api_shim_mid.js` —
`_LUMEN_WRAPPER_MEMBERS`; the file/line pointers below predate SPLIT-JS3,
2026-08-28, which moved the shim text out of `dom.rs`)
**Найден:** WPT-RUN-3 срез 8 (`ROADMAP.md`) — массовый прогон `css/css-borders`

## Фикс (2026-09-02, P3)

Added `set style(v) { this.style.cssText = String(v); }` next to the existing
`get style()` in `_LUMEN_WRAPPER_MEMBERS` (`web_api_shim_mid.js`) — Web IDL
`[PutForwards=cssText]`: a bare-string assignment now forwards to the
already-correct `cssText` setter inside `_lumen_make_style`'s Proxy handler.
Fixes both the sloppy-mode silent no-op and the strict-mode `TypeError`
(same root cause: a getter-only accessor) in one change, including the
`numeric-testcommon.js` reset pattern (`inline-style-assign` mechanism) that
was TIMEOUT-ing 23 vendored files. Live-verified via `--dump-layout`:
`d.style = '...'` now round-trips through `getAttribute('style')` and
`style.cssText`, in both sloppy and strict mode. BUG-493 (same test page,
`getComputedStyle()` same-tick staleness) is unaffected and stays open.
Gates: `cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
warnings` clean, `cargo test -p lumen-js --features v8-backend` 3414/3414
lib + 83/83 `cases`.

## Механизм

`Element.style` is defined in the JS shim's `_obj` element wrapper
(`dom.rs:5613`) as a plain getter-only accessor: `get style() { return
_style; }`, with no matching `set style(v)`. Per Web IDL, the `style`
attribute on `HTMLElement`/`SVGElement` carries `[PutForwards=cssText]` —
assigning a bare string to `element.style` is spec-required to forward to
`element.style.cssText = value` (the setter `_lumen_make_style` **does**
implement correctly, `dom.rs:4288`, reachable only via `.style.cssText = v`
or `.style.setProperty(...)`/bracket-indexed assignment through the Proxy's
own `set` trap). Because the plain-object literal defines only a getter for
`style`, `element.style = "..."` hits a property with no setter: in
non-strict script (the classic `<script>` tags this WPT harness uses, not
ES modules) this is a **silent no-op** — no exception, no effect, and no
diagnostic.

Reproduced live (`--mcp-port`):

```js
var d = document.createElement('div');
d.style = 'border: solid 1px blue; outline: solid 1px purple;';
document.body.appendChild(d);
d.getAttribute('style')   // → null (nothing was ever written)
```

## Симптом

`border-width-rounding.tentative.html` uses exactly this pattern
(`div.style = \`border: solid ${input} blue; outline: solid ${input}
purple; margin-bottom: 20px;\``, one `<div>` per test case) — every one of
its 11 `{input, expected}` pairs never actually applies any border/outline
styling at all, so the div keeps its default (unstyled) appearance
regardless of `input`. This overlaps with — but is mechanistically distinct
from — [BUG-493](../bugs/BUG-493-OPEN.md) (same file, same failing
subtests): even if this bug were fixed today, BUG-493's same-tick
`getComputedStyle()` staleness would still make the assertions fail, so no
new `.ini` beyond what's already committed for BUG-472/493 on this file is
needed — this bug is filed for its own sake (a real, previously undocumented
IDL-forwarding gap affecting *any* future `element.style = "..."` usage, not
just this test), not because it changes this slice's `.ini` coverage.

## Масштаб находки

Not scoped beyond this file — no other file in `css/css-borders` uses the
`element.style = "string"` pattern (WPT convention strongly favors
`.style.cssText = ...` or per-property `.style.prop = ...`, both of which
already work), so the practical blast radius across the wider `css/` corpus
is expected to be small, but unverified.

## .ini

No dedicated `.ini` — see Симптом above; the affected subtests are already
covered under BUG-472/BUG-493's `.ini` entry for
`border-width-rounding.tentative.html`.

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02)** found the first
file where this bug is the sole, complete cause rather than a secondary
factor: `parsing/background-shorthand-serialization.html` (11/11
subtests) uses exactly the `element.style = 'background: ...;'` pattern
for every single test case, then reads `element.style.background` back.
Because the assignment is a silent no-op, every readback sees the
element's untouched default (empty) style — the test title claims to be
about shorthand *serialization*, but none of these subtests ever reach
serialization logic at all. Committed `.ini` under
`tests/wpt/metadata/css/css-backgrounds/parsing/
background-shorthand-serialization.html.ini`, the bug's first dedicated
`.ini` file.

## Расширение: WPT-RUN-6 срез 14 (2026-08-21) — в strict-mode это не no-op, а TypeError

Formulation above ("a **silent no-op**, no exception, no effect") holds only
for sloppy-mode script. In a strict-mode file the same assignment is a hard
`TypeError`, and it kills the script that made it — every statement after it
in that `<script>` block never runs. Probe (`--dump-layout`, one page, four
blocks):

```
[JS] SLOPPY-OK cssText=[]                                   # no-op, as filed
[JS] STRICT-THROW TypeError: Cannot set property style of #<_ctor> which has only a getter
[JS] STRICT-SCRIPT-END
script error: JS runtime error: Cannot set property style of #<_ctor> which has only a getter
[JS] LATER-SCRIPT-RAN                                       # a *separate* block still runs
```

The unguarded strict block printed nothing after the assignment: `UNGUARDED-
CONTINUED` never appeared.

This matters far beyond the one `css/css-borders` file the bug was filed on,
because WPT's shared numeric helper `css/support/numeric-testcommon.js` opens
with `'use strict'` and resets the target element with `testEl.style = ""`
(line 104, again at 167) *before* registering any test. The helper throws
there, the test file's inline script dies with it, not a single `test()` is
ever registered — and because [BUG-591](BUG-591-FIXED.md) never dispatches the
window `error` event, `testharness.js`'s `error_handler` (which would set the
harness status to ERROR and call `done()`, `resources/testharness.js:5048`)
never runs either. The file therefore reports **TIMEOUT**, not FAIL.

Scale, measured on the WPT-RUN-5 snapshot: exactly **23** vendored files
include `numeric-testcommon.js`, and all 23 are TIMEOUT with this error line
as their only evidence — a 1:1 correspondence, no other consumer and no
consumer that survives. Categories: `css/css-values` 19,
`css/css-color`, `css/css-transforms`, `css/filter-effects`,
`css/css-viewport/zoom` — one file each.

Reproduced live 2026-08-21 (`run_report.py --root css/css-values --limit 12
--processes 3`): `/css/css-values/acos-asin-atan-atan2-computed.html` →
TIMEOUT, browser output ends with `Загружен скрипт: …/css/support/
numeric-testcommon.js` followed by `script error: JS runtime error: Cannot
set property style of #<_ctor> which has only a getter`. Neighbouring tests
in the same run that do not use the helper finish normally.

Classifier support: mechanism `inline-style-assign` in
`tests/wpt/timeout_audit.py`.
