# BUG-1631 — `CSSStyleDeclaration` не имеет `webkitMaskBoxImage*` (`-webkit-mask-box-image*`)

**Статус:** FIXED 2026-10-19 (P3)
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

## Исправление (2026-10-19)

`mask-border*` в движке нет (CSS-SPECS «mask-border» ⬜), поэтому шесть имён не добавлены в таблицу простых алиасов
(`COMPAT_CSS_ALIASES` — 51 имя, тест фиксирует число). Chrome держит `-webkit-mask-box-image*` как собственные
свойства, а не алиасы. В прокси `el.style` (`_lumen_make_style`, `_lumen_make_rule_style`) атрибуты
`webkitMaskBoxImage{,Outset,Repeat,Slice,Source,Width}` перечисляются через `_lumen_is_compat_alias_attr`
(`js/src/shim/web_api_shim_mid.js`); значение хранится как сырой текст, не валидируется и не рисуется.
В `SUPPORTED_PROPERTIES` имена **не** добавлены: `@supports (-webkit-mask-box-image: …)` должен оставаться ложным,
пока рамка-маска не рисуется, иначе страница выбирает ветку без запасного варианта. Тест —
`js/src/dom/tests/compat_css_alias.rs` (`webkit_mask_box_image_attributes_are_enumerated_but_not_supported_in_css`).
Остаток — реализация `mask-border*` и `-webkit-mask-box-image*` в раскладке/отрисовке: CSS-SPECS «mask-border».
