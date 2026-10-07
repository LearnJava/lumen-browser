# BUG-1407 — `getComputedStyle()` не отдаёт `font`, `font-feature-settings`, `font-variation-settings`, `font-size-adjust`, `font-optical-sizing`, `font-palette`, `clip-path`, `clip-rule`, `mask-*`

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 18, `css/css-fonts` + `css/css-masking` + `css/WOFF2`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

`<div id=t>`, `getComputedStyle(t)`:

| свойство | `k in cs` | `getPropertyValue(k)` |
|---|---|---|
| `font-stretch`, `font-variant-numeric`, `font-variant-caps` | `true` | `100%`, `normal`, `normal` |
| `font`, `font-feature-settings`, `font-variation-settings`, `font-size-adjust`, `font-optical-sizing`, `font-palette`, `font-width` | `false` | `""` |
| `clip-path`, `clip`, `clip-rule`, `mask`, `mask-image`, `mask-mode`, `mask-type`, `mask-size`, `mask-repeat`, `mask-composite`, `mask-border-source` | `false` | `""` |

Свойства `font-feature-settings`/`-variation-settings`/`-size-adjust`/`-optical-sizing`/`-palette`, `clip-path`,
`clip-rule` и `mask-*` разбираются и применяются (`CSS-SPECS.md:245…249`, `455…462`), в карту
`computed_style_to_map` их нет. `font-kerning`, `font-synthesis`, `font-variant-alternates`,
`font-variant-east-asian`, `font-language-override` не разбираются вовсе — это задача P4, см. `CSS-SPECS.md`.

Первая проверка общего хелпера `computed-testcommon.js` — `property in getComputedStyle(el)` — роняет весь файл:
`assert_true: font-feature-settings doesn't seem to be supported in the computed style`.

## Как найдено

WPT-RUN-14 срез 18: `css-fonts/parsing/{font-computed,font-feature-settings-computed,font-variation-settings-computed,
font-size-adjust-computed,font-optical-sizing-computed,font-palette-computed,font-width-computed,
font-kerning-computed,font-synthesis-computed,font-variant-east-asian-computed,font-language-override-computed}.html`,
`palette-mix-computed.html`, `inheritance.html`, `font-shorthand-subproperties-reset.html`;
`css-masking/parsing/{clip,clip-path,clip-rule,mask,mask-image,mask-composite,mask-repeat,mask-size,mask-type}-computed.html`,
`inheritance.sub.html`, `mask-shorthand-subproperties-reset.html`.

## Что делать

Добавить строки в `computed_style_to_map` (`selector_query.rs`) для перечисленных свойств; сериализация — по
спецификации каждого. Для `font`-шорткода — `""`, если лонгхенды нельзя выразить шорткодом (CSSOM §6.7.2).

## Как проверить

`css/css-fonts/parsing/font-feature-settings-computed.html`, `font-palette-computed.html`,
`css/css-masking/parsing/clip-path-computed.html`, `mask-image-computed.html`.
