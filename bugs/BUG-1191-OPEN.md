# BUG-1191 — `getComputedStyle` пуст у элемента без бокса

**Статус:** OPEN
**Заведён:** 2026-09-26 (P1, при закрытии GAP-UASHADOWSLOT).
**Область:** layout — [`collect_computed_styles`](../crates/engine/layout/src/lib.rs)
(снимок строится обходом `LayoutBox`-дерева), js — шим `getComputedStyle`.

## Симптом

`getComputedStyle(el).length === 0` и `getComputedStyle(el).display === ''` для элемента,
который стоит в flat tree, но не породил собственного `LayoutBox`:

- пустой inline-элемент — `div.appendChild(document.createElement('span'))`;
- потомок `content-visibility: hidden` (в том числе содержимое закрытого `<details>`);
- потомок `display: none`.

Chrome отдаёт полный набор вычисленных значений для всех трёх. Пустой `length` верен только
для узла вне flat tree (потомок `<video>`/`<audio>`, у которых UA shadow tree без слота).

## Механизм

`collect_computed_styles` публикует стиль бокса, а для узлов без бокса — только `display:
contents` из кэша каскада (`CounterMap::style_arc`). Inline-элемент без содержимого бокса не
получает, в поддерево `content-visibility: hidden`/`display: none` каскад не спускается.

Поднять планку просто так нельзя: шим `innerText` (`_lumen_rt_is_rendered` и соседи в
`web_api_shim_mid.js`) и `checkVisibility` читают «есть запись» как «узел отрисован».
Починка должна развести эти два значения.

## Что требуется

Полные вычисленные значения для любого элемента flat tree; `innerText`/`checkVisibility` не
регрессируют. Критерий: в WPT `html/rendering/widgets/shadow-dom.html` проверка
`assert_not_equals(childStyle.length, 0)` проходит для `<select>`/`<details>` (последний шаг —
`all: inherit`, см. GAP-CSSALL в [ROADMAP](../ROADMAP.md)).
