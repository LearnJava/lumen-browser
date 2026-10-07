# BUG-1253 — Автоматический минимальный размер (`min-width/min-height: auto`) реализован только для column-контейнера в ветке `flex-basis: <length>` (BUG-158, `fle

**Статус:** FIXED 2026-10-07
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs` — автоматический минимальный размер flex-элемента, CSS Flexbox L1 §4.5)

## Симптом

Автоматический минимальный размер (`min-width/min-height: auto`) реализован только для column-контейнера в ветке `flex-basis: <length>` (BUG-158, `flex.rs:515`). Не работает: (а) row-контейнер — `flex:1 1 0` элемент с содержимым 90 px в контейнере 100 px при двух таких элементах получает 50/50, ожидается 90/90 (overflow); (б) column с определённой `height` — два элемента с содержимым 60 px в контейнере `height:80` сжимаются до 40/40, ожидается 60/60. WPT-RUN-14-S1: кластер «automatic minimum size» — 73 id (оценка по именам `flex-minimum-*`, `flexbox-min-*`, `flex-container-min-content-*`; причина подтверждена пробой на двух представителях), 131 упавший сабтест.

## Описание

Проба `--dump-layout`:

```html
<div style="display:flex;width:100px"><div style="flex:1 1 0;background:green"><div style="width:90px;height:10px"></div></div>
<div style="flex:1 1 0;background:blue"><div style="width:90px;height:10px"></div></div></div>
```

элементы `50`+`50`, ожидается `90`+`90`. Column: `display:flex;flex-direction:column;height:80px` с двумя элементами по `60px` содержимого — высоты 40/40 вместо 60/60.

## Как найдено

WPT-RUN-14 срез 1. Кластер определён регулярным выражением по имени файла и проверен пробами; 73 — оценка сверху, часть id может принадлежать другому кластеру (BUG-1255).

## Как проверить

`run_report.py --root css/css-flexbox --recursive`, файлы `flex-minimum-height-flex-items-*`, `flex-minimum-width-flex-items-*`.

## Исправление

`build_line_inits` (`flex.rs`): автоматический минимум (§4.5) теперь ограничивает и распределение `flex-grow` (цикл заморозки клампит цель по `[min, max]`; раньше учитывал только `max`), и сжатие column-контейнера (раньше минимум колонки был 0): `col_auto_mins` — высота содержимого элемента с `height:auto`, `min-height:auto`, `overflow-y:visible`. Тесты — `box_tree/tests/flex_auto_min_size.rs`. Остаток: column-элемент с определённой `height` (минимум `min(content, specified)`) и `total_grow == 0` не зажимаются по минимуму.
