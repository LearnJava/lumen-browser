# BUG-1521 — `text-emphasis`: метки равномерно размазаны по ширине фрагмента, в вертикальном `writing-mode` не рисуются, `left`/`right` не учитываются, строка не растёт

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** paint (`crates/engine/paint/src/display_list/text_run.rs::emit_text_emphasis_marks`)

## Симптом

`emit_text_emphasis_marks` (Phase 0, комментарий в коде): шаг метки — `ширина фрагмента / число символов`, размер — `font_size * 0.5`, положение — над/под строкой по `is_over()`; при `writing-mode: vertical-*` меток нет вовсе (для вертикали нужны `left`/`right`, а код знает только `over`/`under`) (в снимке столько же тёмных пикселей, сколько без `text-emphasis`); высота строки с `text-emphasis` равна высоте без неё (метки не раздвигают строку).

## Проба

Проба (`--screenshot`, `font:40px/1 Arial`, текст `AAAA`, число тёмных пикселей / bbox):

| случай | пикселей, bbox по `y` | ожидается |
|---|---|---|
| без `text-emphasis` | 972, `43…71` | |
| `text-emphasis:circle; position:over right` | 1 676, `17…71` (метки над строкой) | метки есть |
| `position:under right` | 1 676, `43…95` | метки под строкой |
| `writing-mode:vertical-rl` + `text-emphasis:circle` | **972**, `x 108…136` — как без меток | метки справа от символов |
| `div` `line-height:1` (40 px) с `text-emphasis:circle` — высота блока | 40 | строка раздвигается под метку (`text-emphasis-line-height-*`) |

## Как найдено

WPT-RUN-14 срез 23: `text-emphasis-*` — 131 reftest `thick` (по имени: `text-emphasis-position-property` 40, `-style-property` 38, `-line-height` 13, `-style` 7, `-property` 6, `-ruby` 6, `-color-property` 4, остальное). 55 из 131 — вертикальный `writing-mode`. До исправления BUG-1520 часть падений не отделить.

## Что делать

Расставлять метки по границам глифов (кластерам графем) вместо равномерного шага, поддержать вертикальный поток (`left`/`right`), раздвигать строку (CSS Text Decoration 3 §4.2 «Emphasis Mark Position»: метки занимают место в строке, если `line-height` мал).

## Как проверить

Таблица выше; `css/css-text-decor/text-emphasis-line-height-001a.html`, `text-emphasis-position-property-003b.html`.
