# BUG-127

**Статус:** OPEN (feature wired — остаток = KNOWN_DEBTOR)
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
2. **Цвет view-бокса не композится** — BUG-231: анимированный `background-color`
   не применяется в живом окне (только opacity/transform compositor-offloadable),
   поэтому subject держит базовый teal вместо оранжевого.

KNOWN_DEBTORS['78'] = ('BUG-127', 10.07).
