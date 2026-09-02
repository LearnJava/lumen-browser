# BUG-497: `CSSStyleDeclaration.cssText` serialization omits the trailing semicolon

**Статус:** DUPLICATE → [BUG-473](BUG-473-FIXED.md)
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:4257` — `_lumen_serialize_style`)
**Найден:** WPT-RUN-3 срез 9 (`ROADMAP.md`) — массовый прогон `css/css-backgrounds`

## Ревизия P3 2026-09-03: дубликат BUG-473, уже исправлен

**Дубликат [BUG-473](BUG-473-FIXED.md)** (тот же сериализатор
`_lumen_serialize_style`, тот же симптом — отсутствующая `;` после последней
декларации); выживает BUG-473 — оба бага датированы 2026-08-02, но BUG-473
найден в срезе 3 того же прогона WPT-RUN-3, BUG-497 в срезе 9 (позже в том же
прогоне), плюс меньший номер. BUG-473 закрыт 2026-09-01: `_lumen_serialize_style`
(`crates/js/src/shim/web_api_shim_mid.js`) переписана и теперь безусловно
закрывает последнюю декларацию `;` (`parts.join('; ') + ';'`), а геттер
`cssText` переведён на этот сериализатор вместо сырого текста атрибута.
Живая проверка (`cargo test -p lumen-shell --features v8
inline_style_serialization_collapses_shorthand_and_normalizes_text`, срез
2026-09-03) подтверждает точный репро этой заявки — `left: 10px` сериализуется
как `left: 10px;`. Новых измерений эта заявка не добавляет к BUG-473.

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
