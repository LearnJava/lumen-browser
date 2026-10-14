# BUG-1625 — Невалидный background/background-image затирает прежнее значение

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** layout (`crates/engine/layout/src/style/apply/paint.rs:147`, `:357`, `:582`)

## Симптом и место в коде

Найдено при проверке градиентов: невалидное значение не отбрасывается целиком (CSS Syntax), а затирает предыдущую валидную декларацию. Шортхенд `background` безусловно очищает слои и background-color до разбора (style/apply/paint.rs:147-148), parse_single_bg_layer (style/parse/image.rs:374) не сообщает об ошибке и пропускает неизвестные токены; `background-image` и `mask-image` превращают неизвестную функцию в BackgroundImage::None (paint.rs:357, :582). Каскад вызывает apply_declaration без отдельной валидации (cascade.rs:1328). Итог по коду: цепочка с `-o-`/`-ms-`/`-moz-` градиентом после рабочей строки даёт прозрачный фон. Живой пробой не подтверждено; тестов на префиксные градиенты в crates/ и graphic_tests нет. Ранее не заведён (поиск по BUGS.md/BUGS-FIXED.md). По ролям — P3; в выборе owner P3 нет.

## Где смотреть

- crates/engine/layout/src/style/apply/paint.rs:147
- crates/engine/layout/src/style/apply/paint.rs:357
- crates/engine/layout/src/style/apply/paint.rs:582
- crates/engine/layout/src/style/parse/image.rs:374
- crates/engine/layout/src/style/cascade.rs:1328

## Объём и приёмка

Невалидное значение не отбрасывается, а затирает предыдущее: `background` очищает слои и цвет до разбора (style/apply/paint.rs:147–148), неизвестная функция в `background-image`/`mask-image` даёт `BackgroundImage::None` (paint.rs:357, :582). Цепочка `background:#1e5799; background:-webkit-gradient(…); background:-o-linear-gradient(…)` в Chrome даёт градиент, в Lumen — прозрачный фон (по коду, пробой не подтверждено). Без правки -webkit-градиенты не помогут старым сайтам: следующая -o-/-ms- строка всё сотрёт. Приёмка: `background:red; background:foo(1)` → красный, как в Edge; юнит-тесты на три свойства. Источник: report.md §3 п.8.

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
