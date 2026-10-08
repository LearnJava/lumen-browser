# BUG-1352 — `margin` и `padding` на внутренних табличных боксах (`table-row-group`/`header-group`/`footer-group`/`row`) применяются при раскладке, а CSS 2.1 §17 их не применяет

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 12, `css/CSS2` (normal-flow + margin-padding-clear))
**Область:** layout (`crates/engine/layout/src/box_tree/table.rs` — внутренние табличные боксы)

## Симптом

`--dump-layout`: `display:table` > `table-row-group` (`margin:7px; padding:9px`) > `table-row` (то же) > `table-cell` высотой 10 px. Получено: `TableRowGroup m=(7,7,7,7) p=(9,9,9,9)`, `TableRow` с теми же полями; ячейка на `y=31.72` вместо `17.72` (поля 7 + 7 применены), высота таблицы 74 вместо 10 (в неё вошли и `padding` 9×4). Таблица без полей у групп — `y=17.72`, высота 10. Снимок `margin-bottom-applies-to-001.xht`: между оранжевой нижней рамкой ячейки и синей рамкой обёртки — зазор 50 px (поле `table-row-group`), в эталоне зазора нет.

## Как найдено

WPT-RUN-14 срез 12: 51 id `*-applies-to-*.xht` (`margin-*`, `padding-*`, `min-*`, `max-*`, `width`) с `display: table-row-group|header-group|footer-group|row|column-group|column|caption` в `#test`. Пересекается с BUG-1334/1335/1336 (в тех же файлах таблица ломается тремя способами сразу), поэтому 51 — верхняя граница; `table-caption` (по §17.4 поля на подписи применяются) — 3 из них, отнесены сюда по правилу «первое совпавшее» и могут быть другим дефектом. A/B не делался.

## Что делать

Не применять `margin`/`padding` к внутренним табличным боксам (кроме `table-caption`, `table`, `table-cell`) при раскладке таблицы; для `table-column(-group)` — ещё и `min/max-width`.

## Как проверить

`css/CSS2/margin-padding-clear/margin-bottom-applies-to-001.xht`, `padding-applies-to-004.xht`.
