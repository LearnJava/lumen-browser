# BUG-1831 — sticky оба инсета: нет приоритета «top/left wins» (overconstrained-семья WPT) + флак `left-and-right-overconstrained`

**Статус:** OPEN
**Открыт:** 2026-11-19 (P6, при BUG-1825 срез 1)
**Область:** layout/paint (`sticky_correction` в `sticky_snap.rs`, зеркала `sticky_dy`/`sticky_dx` в paint; дельта-форма накапливает `top`- и `bottom`-поправки последовательно)

## Симптом 1 — приоритет при обоих инсетах

CSS Position L3 (sticky-алгоритм): при одновременном срабатывании обоих инсетов оси выигрывает `top` (по горизонтали — `left`). Дельта-форма (все пять зеркал) применяет `bottom`-поправку от **незастрявшей** позиции (`actual_screen_y = flow.y − scroll`, без уже набранной `top`-поправки) и затирает её. `compute_sticky_offset` (page-level, кламп-форма) — «top wins», но до пяти зеркал дельта-формы это не доведено.

- `css/css-position/sticky/position-sticky-top-and-bottom-overconstrained.html` — 0/4 стабильно.
- `position-sticky-left-and-right-overconstrained.html` — стабильно соло 0/4: «right before» 160 (эталон 180), «right after» 220 (эталон 240) — ровно на инсет 20 меньше; «start of scroll container» и «end» тоже мимо.
- `position-sticky-horizontal-rtl-overconstrained.html` — 0/4.

## Симптом 2 — недетерминизм в батче

Один и тот же бинарник: `left-and-right-overconstrained` в батче из 5 файлов — 3/4 (2 неожиданных PASS), соло трижды — 0/4. Похоже на гонку чтения `offsetLeft` сразу после программного `scrollLeft`: незастявшее (flow) значение совпадает с ожиданием двух подтестов, и неприменившийся скролл даёт ложный PASS. Замеры 2026-11-19: `.tmp` не сохранялись, логи прогона — `run_smoke.py` stdout; повторяемость: соло 3/3 прогонов FAIL, батч 1/1 прогон PASS.

## Что делать

1. Приоритет: привести все пять зеркал к кламп-форме `compute_sticky_offset` (clamp в `[lo, hi]`, lo выигрывает) либо честно учитывать уже набранную поправку в `bottom`/`right`-ветках.
2. Флак: проверить, публикует ли запись `scrollLeft` скролл-стейт синхронно с чтением `offsetLeft` (какая цепочка scroll_states → correction → getBoundingClientRect); закрепить WPT-подтест (или юнит) на детерминизм.

## Как проверить

Три overconstrained-файла зелёные в ≥ 3 соло-прогонах подряд; `.ini` перевёрнуты; la-ri не зависит от состава батча.
