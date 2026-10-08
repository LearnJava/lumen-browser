# BUG-1257 — `align-items/align-self: baseline` / `first baseline` / `last baseline` во flex не реализованы: в `flex.rs` нет ни одного упоминания baseline, элемент

**Статус:** FIXED 2026-10-04 (P1, FLEX-BASELINE; остаток — `FLEX-BASELINE-2`)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`)

## Симптом

`align-items/align-self: baseline` / `first baseline` / `last baseline` во flex не реализованы: в `flex.rs` нет ни одного упоминания baseline, элементы с разным кеглем остаются у верхней границы линии (проба: контейнер `align-items:baseline`, элементы с `font-size` 30 px и 10 px — оба `y=0`, ожидается выравнивание базовых линий). Семейство (baseline-синтез по контенту, первый/последний, вложенные flex/grid/multicol), а не одна строка. WPT-RUN-14-S1: 82 id (`flex-align-baseline*`, `align-items-baseline*`, `alignment/*`, `baseline-synthesis-*`; оценка по `baseline` в имени), 336 упавших сабтестов.

## Описание

Записано как ДОРАБОТКА по `docs/probe-method.md` §8: `grep -ci baseline crates/engine/layout/src/box_tree/flex.rs` = 0, а объём — модель baseline (получение baseline бокса любого вида, синтез для replaced/пустых, last baseline), а не точечная правка. Задача — `FLEX-BASELINE` в `ROADMAP.md`.

## Как найдено

WPT-RUN-14 срез 1.

## Исправление (2026-10-04)

`align-items/align-self: baseline | first baseline | last baseline` реализованы: `AlignValue::LastBaseline` (раньше `last baseline` парсился в тот же `Baseline`), модель
базовой линии бокса в `crates/engine/layout/src/box_tree/baseline.rs`, раскладка ряда по baseline-группам в `flex_trampoline::finish_line`, fallback start/end в колонке.
Подробности — `subsystems/layout.md` (запись FLEX-BASELINE) и `CAPABILITIES.md`.

Замеры (`run_smoke.py`, `css/css-flexbox`, 368 testharness-id): сабтесты 1690 → 1746; baseline-набор из 50 id — 85 → 130 из 421; `css/css-align` +17, регрессий 0;
reftest `css-flexbox` (937 id, попиксельно) identical 263 → 280 (большая часть — побочная правка размера flex-линии с полями items). Минус: `alignment/flex-align-baseline-005`
(1/3 → 0/3) — вертикальный `writing-mode` контейнера, где прежний «проход» был случайным совпадением start-выравнивания.

## Остаток

См. задачу `FLEX-BASELINE-2` в `ROADMAP.md`: baseline через grid/table/multicol/fieldset/`-webkit-line-clamp`, вертикальные и ортогональные `writing-mode`
(после `FLEX-VWM`), `wrap-reverse` у колонки.
