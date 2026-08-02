# BUG-494: `element.style = "css text"` (whole-string assignment) is a
silent no-op — missing Web IDL `[PutForwards=cssText]` forwarding

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js shim (`crates/js/src/dom.rs:5613`, `_lumen_make_style`
Proxy at `dom.rs:4264`)
**Найден:** WPT-RUN-3 срез 8 (`ROADMAP.md`) — массовый прогон `css/css-borders`

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
