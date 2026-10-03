# BUG-1257 — `align-items/align-self: baseline` / `first baseline` / `last baseline` во flex не реализованы: в `flex.rs` нет ни одного упоминания baseline, элемент

**Статус:** OPEN (ДОРАБОТКА → FLEX-BASELINE)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`)

## Симптом

`align-items/align-self: baseline` / `first baseline` / `last baseline` во flex не реализованы: в `flex.rs` нет ни одного упоминания baseline, элементы с разным кеглем остаются у верхней границы линии (проба: контейнер `align-items:baseline`, элементы с `font-size` 30 px и 10 px — оба `y=0`, ожидается выравнивание базовых линий). Семейство (baseline-синтез по контенту, первый/последний, вложенные flex/grid/multicol), а не одна строка. WPT-RUN-14-S1: 82 id (`flex-align-baseline*`, `align-items-baseline*`, `alignment/*`, `baseline-synthesis-*`; оценка по `baseline` в имени), 336 упавших сабтестов.

## Описание

Записано как ДОРАБОТКА по `docs/probe-method.md` §8: `grep -ci baseline crates/engine/layout/src/box_tree/flex.rs` = 0, а объём — модель baseline (получение baseline бокса любого вида, синтез для replaced/пустых, last baseline), а не точечная правка. Задача — `FLEX-BASELINE` в `ROADMAP.md`.

## Как найдено

WPT-RUN-14 срез 1.
