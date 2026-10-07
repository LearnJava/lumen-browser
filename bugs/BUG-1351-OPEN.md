# BUG-1351 — Ширина по содержимому (shrink-to-fit) не учитывает горизонтальные `margin` блочных детей: внешний бокс получается уже, чем ребёнок вместе с его полями

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** layout (`crates/engine/layout/src/box_tree/intrinsic.rs`, `layout_dispatch.rs` — shrink-to-fit ширина `position:absolute`/`float`/`inline-block`)

## Симптом

`--dump-layout`: `<div style="position:absolute"><div style="margin:0 40px;width:10px;height:10px"></div></div>` → внешний `rect` ширины **10**, ребёнок на `x=40…50` (вылез). Ожидается 90 (10 + 2×40). Тот же результат при `float:left`. Для `padding`/`border` ребёнка (`padding:0 40px` → 102,2 px) всё верно — не учитываются именно `margin`. Ребёнок без `width` и с текстом: `margin:0 40px` → внешняя ширина 22,2 (только текст).

## Как найдено

WPT-RUN-14 срез 12: `margin-padding-clear/margin-{001…009,collapse-102…104}`, `margin-{bottom,right}-applies-to-012`, `normal-flow/{blocks-025,block-non-replaced-width-001,inline-block-zorder-*}` — 29 id (контейнер `position:absolute|float|inline-block`, у ребёнка `margin`). Проба на `margin-001.xht`: абсолютный `#div1` (рамка 10 + ребёнок 288+2×10 рамки + поля 2×96 + рамка 10) должен быть 520 px и совпасть с красным `#reference` 500+2×10; по дампу у Lumen `rect=(8, 25.72, 328, 328)` — поля ребёнка не вошли. A/B не делался (правка в движке), число — верхняя граница.

## Что делать

В max-content/min-content ширине блока добавлять горизонтальные `margin` детей в потоке (CSS 2.1 §10.3.5, §10.3.7; CSS Sizing 3 §5).

## Как проверить

`css/CSS2/margin-padding-clear/margin-001.xht`.
