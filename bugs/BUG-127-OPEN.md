# BUG-127

**Статус:** OPEN (DEBTOR) — feature wired, остаток = font-parity (закроется с FP-1)
**Компонент:** shell/layout
**Файл:** `crates/shell/src/animation_scheduler.rs`, `crates/engine/layout/src/scroll_timeline.rs`

## Описание

CSS Scroll-Driven Animations L1 (scroll-timeline/view-timeline/animation-timeline) — TEST-78.

## Состояние (F2-2, 2026-06-22)

Фича подключена end-to-end: `animation-timeline: scroll() | view() | <custom-ident>`
теперь драйвит прогресс анимации от позиции скролла/вьюпорта, а не от часов `@keyframes`.

- `shell/animation_scheduler.rs`: `ScrollCtx::progress_for` резолвит timeline →
  прогресс `[0,1]` через `resolve_scroll_progress` / `resolve_view_progress`
  (`lumen-layout/scroll_timeline.rs`). Named-timeline матчатся против
  `collect_named_scroll/view_timelines(root)`. Для `auto` — обычный clock-путь.
- `parse_keyframe_style` (`layout/animation.rs`): добавлен разбор шортхенда
  `background: <color>` (раньше только `background-color`).

TEST-78: 12.02% → 10.07%. `scroll()` и named-боксы садятся в from-state (как у Edge);
view()-бокс едет вправо по view-прогрессу (позиция совпадает с Edge).

## Остаток (почему всё ещё > 0.5%) → KNOWN_DEBTOR

1. **Font-parity** (доминирует): страница текстоёмкая (заголовок, метки, 5-строчный
   `.info` моноблок) — Inter fallback vs шрифты Edge даёт ghosting по всему тексту
   (класс BUG-128, rule 3 «текст не трекаем»).
2. ~~**Цвет view-бокса не композится** — BUG-231~~ — **FIXED** (D2-2): анимированный
   `background-color` теперь композится, subject рисуется интерполированным
   оранжевым. Пункт закрыт.

## Ревизия P3 (2026-07-04)

- Свежая сборка main, gdigrab: **4.62%** (стабильно: full-run 2026-07-01 = 4.64%).
- Diff-декомпозиция: остаток = **font-parity текста** (заголовок, метки карточек,
  `.info` моноблок — Inter vs Edge, rule 3, класс BUG-128) + субпиксельные сдвиги
  анимируемых subject-боксов. From-state рамки всех 5 карточек совпадают
  пиксель-в-пиксель (в diff чёрные).
- Точечного P3-дефекта нет. Закроется только с FP-1 (font-parity, домен P1).

KNOWN_DEBTORS['78'] = ('BUG-127', 4.64) — ратчет вниз 10.07 → 5.76 (--ipc) → 4.64.
