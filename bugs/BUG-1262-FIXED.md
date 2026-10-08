# BUG-1262 — `margin-top` первого flex-/grid-элемента схлопывается с полями предков контейнера

**Статус:** FIXED 2026-10-08
**Заведён:** 2026-10-04 (P1, FLEX-BASELINE)
**Область:** layout (`crates/engine/layout/src/box_tree/bfc.rs` — `establishes_bfc`, `collapsed_top_margin`)

## Симптом

Поле `margin-top` первого flex-элемента двигает вниз сам flex-контейнер, а потом элемент отступает на то же поле ещё раз внутри контейнера — суммарно поле считается дважды.
`display: grid` ведёт себя так же. Проба `--dump-layout`, `body{margin:0}`:

```html
<div style="display:flex;width:300px"><div style="height:10px;margin-top:20px"></div></div>
```

Контейнер — `rect.y = 20`, элемент — `rect.y = 40`; ожидается (Chrome) контейнер `y = 0`, элемент `y = 20`. С `display:grid` — то же: контейнер 20, элемент 40.

## Описание

CSS 2.1 §8.3.1: поля контейнера и его первого ребёнка схлопываются, только если контейнер не создаёт контекст форматирования; flex-/grid-контейнер создаёт независимый
контекст форматирования (Flexbox §3, Grid §3), поэтому поля его элементов с контейнером и его предками не схлопываются. `establishes_bfc` (`bfc.rs:11`) знает только
`BoxKind::FlowRoot`, `overflow` ≠ `visible`, `float` и `position: absolute|fixed`, и цепочка первых детей в `collapsed_top_margin` проходит через `display: flex|grid` вниз.
Вероятно, тот же пропуск у `inline-block`, `table-cell`, `contain: layout|paint` — проверить пробой до правки.

## Как найдено

FLEX-BASELINE: юнит-тест с `margin-top` у первого элемента падал на абсолютном `y` (ожидалось 20, получено 40); проба `--dump-layout` показала, что контейнер сам стоит на `y = 20`.

## Как проверить

Проба выше; тест — абсолютный `y` контейнера и элемента в `crates/engine/layout/src/box_tree/tests/flex_baseline.rs` (сейчас позиции считаются относительно контейнера из-за этого дефекта).
Правка двигает пиксели у любой страницы, где у первого flex-/grid-элемента есть `margin-top`: полный графтест + регенерация эталонов в том же коммите.

## Исправление

`establishes_bfc` (`bfc.rs`) учитывает `display: flex|inline-flex|grid|inline-grid` — цепочка первых/последних детей в `collapsed_top_margin`/`collapsed_bottom_margin` обрывается на таком контейнере. Тесты: `flex_baseline.rs` (абсолютный `y` контейнера и элемента для flex и grid). A/B `--dump-layout` по graphic_tests и samples (186 страниц) — без отличий, эталоны не менялись. `inline-block`, `table-cell`, `contain` не проверялись — отдельная проба при появлении жалобы.
