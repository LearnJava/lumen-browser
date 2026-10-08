# GAP-ANCHORCSSOM-S1 — грамматика `anchor()` / `anchor-size()` в парсере layout

Срез 1 из [GAP-ANCHORCSSOM](../../ROADMAP.md) (BUG-563). Владелец — P1. Крейт — `lumen-layout`.

## Цель

Привести разбор к грамматике CSS Anchor Positioning L1 §3.1, §4.1:

1. `anchor()`: `<anchor-name>` и `<anchor-side>` комбинируются через `&&`, порядок любой.
   `anchor(top --a)` должно разбираться так же, как `anchor(--a top)`.
2. `<anchor-side>`: добавить `inside`, `outside`, `self-start`, `self-end`.
   Это новые варианты `AnchorSide` и их резолв:
   - `inside` — та же сторона, что у инсет-свойства;
   - `outside` — противоположная;
   - `self-start` / `self-end` — по writing-mode и direction самого позиционируемого элемента; при horizontal-tb + ltr совпадают с `start` / `end`.
3. `anchor-size()`: имя и размер разделяются **пробелом**: `anchor-size(--a width)`. Запятая отделяет fallback: `anchor-size(--a width, 10px)`.
   Сейчас код ждёт `anchor-size(--a, width)`, поэтому валидный CSS не разбирается и на пути layout. Это видимый дефект.

## Точки входа

- [box_sides.rs:385](../../crates/engine/layout/src/style/parse/box_sides.rs#L385) `parse_anchor_func`.
- `:406` `parse_anchor_side`.
- `:351` `parse_anchor_size_func`.
- [anchor.rs:47](../../crates/engine/layout/src/anchor.rs#L47) `enum AnchorSide`.
- `anchor.rs:349` `anchor_side_value`. У него `is_horizontal`; для `inside`/`outside` нужен ещё признак стороны инсета. `resolve_anchor_func` (`:393`) уже получает `is_end_edge`, его и протянуть.

## Не трогать

- `CalcNode` (`style/calc.rs`), то есть `anchor()` внутри `calc()`. Это S3, проектный вопрос.
- `position-area` / `InsetAreaKeyword` — S4.
- JS-шим и `element.style` — S2.

## Готово, когда

Юнит-тесты в `lumen-layout` зелёные:

- `anchor(top --a)` == `anchor(--a top)`.
- Четыре новые стороны разбираются и резолвятся. Для `inside`/`outside` проверить `top` и `bottom`.
- `anchor-size(--a width)` и `anchor-size(--a width, 10px)` разбираются.
- Повтор компонента (`anchor(--a --b top)`, `anchor(top left)`) даёт `None`.
- Старая форма с запятой `anchor-size(--a, width)` даёт `None`, как по спецификации.
- Существующие anchor-тесты и графтесты без дельт.

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/run.py --continue-on-fail
```

Нужен полный графический прогон, потому что меняется резолв layout.

## Зависимости

Нет. S2 зависит от этого среза.
