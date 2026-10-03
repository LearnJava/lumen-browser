# BUG-1258 — Flex-контейнер с вертикальным `writing-mode` (`vertical-rl`/`vertical-lr`/`sideways-*`) раскладывается как горизонтальный: главная ось `row` остаётся 

**Статус:** OPEN (ДОРАБОТКА → FLEX-VWM)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs`, `vertical_trampoline.rs`)

## Симптом

Flex-контейнер с вертикальным `writing-mode` (`vertical-rl`/`vertical-lr`/`sideways-*`) раскладывается как горизонтальный: главная ось `row` остаётся горизонтальной (проба: `writing-mode:vertical-rl;display:flex`, два ребёнка 30×40 — оба `y=0`, `x=70` и `x=40`, то есть справа налево по горизонтали; ожидается сверху вниз по вертикали, `x=70` у обоих), а флаги осей не пересчитываются через writing-mode/direction. `flex.rs` не упоминает `writing_mode` вовсе. Семейство — физические/логические оси, `flex-direction`, auto-margins, abspos static position, baseline во всех 8 сочетаниях writing-mode×direction. WPT-RUN-14-S1: 116 id по имени/исходнику (`writing-mode: vertical|sideways`, `-vert`, `-vlr`, `-vrl`, `wmvert`, `flexbox-writing-mode-*`), 963 упавших сабтеста — крупнейший кластер.

## Описание

ДОРАБОТКА по `docs/probe-method.md` §8: `grep -ci "writing_mode" crates/engine/layout/src/box_tree/flex.rs` = 0, объём — модель осей. Задача — `FLEX-VWM` в `ROADMAP.md`.

## Как найдено

WPT-RUN-14 срез 1.
