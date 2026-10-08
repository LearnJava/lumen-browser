# Задача: `<position>` с отступом от края (3/4-значная форма)

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:78` (CSS Values L4, остаток BUG-495) · **Размер:** M · **Крейты:** `lumen-layout`, `lumen-paint`

## Цель
`background-position: right 12px center`, `object-position: left 10% bottom 20px`, `calc(100% - Npx)` работают. Сейчас `ObjectPosition::parse` отвергает больше 2 токенов, а `PositionComponent` умеет только `Px`/`Percent`.

## Точка входа
- `crates/engine/layout/src/style/values/flexgrid.rs:523`, `:584`, `:637` — `PositionComponent`/`ObjectPosition::parse`.
- `crates/engine/paint/src/display_list/geometry.rs:284` — резолв позиции фона.

## Что сделать
1. Вариант «доля + px» в `PositionComponent` (`right 10px` = 100% − 10px), метод `.resolve(free)`.
2. Парсинг 3- и 4-значной формы и `calc(100% - Npx)`.
3. Все `match` по вариантам (~10 мест: geometry.rs, renderer.rs, femtovg, selector_query) перевести на `.resolve(free)`.

## Не трогать
Лонгхенды `background-position-x/-y` (уже есть); сериализацию `getComputedStyle` — только в объёме новых вариантов.

## Готово, когда
- Юнит: `right 10px bottom 20px` на области 200×100 с тайлом 20×20 даёт смещение (170, 60).
- `--dump-display-list`: в `DrawBackgroundImage` x = край − 10.

## Гейт
`cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings`; двигает пиксели → полный `python graphic_tests/run.py --continue-on-fail` и эталоны в том же коммите.
