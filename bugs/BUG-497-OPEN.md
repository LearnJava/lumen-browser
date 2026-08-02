# BUG-497: `CSSStyleDeclaration.cssText` serialization omits the trailing semicolon

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:4257` — `_lumen_serialize_style`)
**Найден:** WPT-RUN-3 срез 9 (`ROADMAP.md`) — массовый прогон `css/css-backgrounds`

## Механизм

Per the CSSOM `serialize a CSS declaration block` algorithm, each
declaration in `cssText` is followed by `"; "` (or `";"` before the closing
of the last one) — a single-declaration block serializes as
`"prop: value;"`, semicolon included. `_lumen_serialize_style`
(`dom.rs:4257`) joins `prop: value` pairs without appending this trailing
separator.

## Симптом

```js
var d = document.createElement('div');
d.style.borderRadius = '10px';
d.style.cssText;   // -> "border-radius: 10px"  (spec: "border-radius: 10px;")
```

`border-radius-css-text.html`'s one subtest ("Setting border-radius does
not expand to longhand properties in cssText") actually asserts on the
semicolon, not longhand expansion (the title is misleading — the shorthand
*is* correctly preserved as `border-radius`, not exploded into four
longhands, which is itself correct per spec since `cssText` only expands
implicitly-set shorthands when the underlying `[[declarations]]` entries
require it):

```
assert_equals: expected "border-radius: 10px;" but got "border-radius: 10px"
```

## Масштаб находки

Confirmed on a single subtest this slice — `cssText` is read relatively
rarely compared to per-property getters in WPT tests, so the true blast
radius is unmeasured, but the defect is unconditional (every `cssText`
read of a non-empty declaration block is missing its final `;`), not
input-dependent.

## .ini

Committed `.ini` for `border-radius-css-text.html` under
`tests/wpt/metadata/css/css-backgrounds/`.
