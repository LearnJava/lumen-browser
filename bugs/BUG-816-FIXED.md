# BUG-816

**Статус:** FIXED 2026-08-22
**Компонент:** test/paint
**Файл:** `crates/engine/paint/tests/snapshots/img_with_background_and_border.snap`

## Описание

`cargo test -p lumen-paint` красный на `main` с 2026-08-21 16:07
(коммит `29e7bcf1d`, [BUG-431](BUG-431-FIXED.md)): текстовый golden-снапшот
display list'а `img_with_background_and_border` продолжает ожидать
`DrawImage` в **border-box**, тогда как фикс BUG-431 намеренно перевёл
содержимое `<img>`/`<video>`/`<iframe>` в **content-box**.

```
---- cases::snapshot_tests::img_with_background_and_border stdout ----
Snapshot 'img_with_background_and_border' mismatch.
--- expected ---
DrawImage (0.00, 0.00, 54.00, 54.00) src="x.png" alt=""
--- actual ---
DrawImage (2.00, 2.00, 50.00, 50.00) src="x.png" alt=""
```

Прав **actual**: `<img width="50" height="50">` с `border: 2px solid` при
дефолтном `box-sizing: content-box` даёт border-box 54×54 (фон и рамка) и
content-box 50×50 со смещением (2,2), куда и обязана лечь картинка
(CSS Box L3 §1, CSS Images L3 §5). То есть дефект — в ожидании теста, а не
в движке.

## Почему это важно

Падение блокирует шаг 1 протокола завершения задачи (`cargo test -p
lumen-paint`) и `scripts/scoped-test.sh` **для любой роли**, которая
трогает paint, — независимо от содержания её правки. Ровно тот же класс,
что [BUG-118](BUG-118-FIXED.md) / [BUG-149](BUG-149-FIXED.md) /
[BUG-297](BUG-297-FIXED.md) / [BUG-316](BUG-316-FIXED.md) (протухшие
эталоны после правки покраски), но здесь протух не PNG-снапшот CPU-пути, а
текстовый golden display list'а — его в чеклисте «регенерировать в том же
коммите» (CLAUDE.md §Graphic tests, п. 5) нет вовсе, поэтому автор BUG-431
и не мог о нём вспомнить: коммит `29e7bcf1d` регенерировал графические
результаты и проверил `snapshot_cpu` (там изменений действительно нет — ни
одна страница `PAGES` не сочетает рамку/padding прямо на `<img>`), но
`crates/engine/paint/tests/snapshots/` не тронул.

## Область

Единственный протухший эталон: полный `cargo test -p lumen-paint` даёт
1014 + 28 зелёных и ровно одно это падение. Ссылки на `dump-golden`
не затронуты — `grep DrawImage graphic_tests/dump-golden/` пуст, ни одна
из 6 страниц гейта не содержит замещённых элементов.

## Фикс

Эталон перегенерирован (`UPDATE_SNAPSHOTS=1 cargo test -p lumen-paint
--test all img_with_background_and_border`) — одна строка. В комментарий
теста добавлено, ПОЧЕМУ у фона и картинки разные прямоугольники
(border-box против content-box, ссылка на BUG-431), чтобы следующее
расхождение не читалось как регрессия движка.

## Происхождение

Найден P3 2026-08-22 при разборе очереди `STATUS-P3.md`.
