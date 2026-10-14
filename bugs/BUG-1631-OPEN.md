# BUG-1631 — `CSSStyleDeclaration` не имеет `webkitMaskBoxImage*` (`-webkit-mask-box-image*`)

**Статус:** OPEN
**Заведён:** 2026-10-14 (P2, прогон `compat/` — COMPAT-CSS-WPT)
**Область:** css-parser/layout (перечень свойств `CSSStyleDeclaration`, Compat Standard)

## Симптом

`tests/wpt/compat/webkit-mask-box-enumeration.html` — 0/6. Для каждого из шести имён
`webkitMaskBoxImage`, `webkitMaskBoxImageOutset`, `webkitMaskBoxImageRepeat`, `webkitMaskBoxImageSlice`,
`webkitMaskBoxImageSource`, `webkitMaskBoxImageWidth` подтест «found on CSSStyleDeclaration» падает:
`assert_true: expected true got false`.

Compat Standard (compat.spec.whatwg.org) требует `-webkit-mask-box-image*` как алиасы `mask-border*`.
Строки в `CSS-SPECS.md` для них нет (CSS-SPECS.md:781 — общие `-webkit-*` алиасы, `-webkit-mask-box-image*` не
перечислены).

## Что нужно

1. Добавить строку в CSS-SPECS «Compat»: `-webkit-mask-box-image*` ↔ `mask-border*` (сначала нужны сами `mask-border*`).
2. Реализовать алиасы и имена в перечне `CSSStyleDeclaration`.
3. Проверка: `python tests/wpt/run_corpus.py --prefixes compat` — `webkit-mask-box-enumeration` 6/6.
