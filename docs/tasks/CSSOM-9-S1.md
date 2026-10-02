# CSSOM-9-S1 — resolved value процентных инсетов `position: sticky`

Остаток BUG-472 после CSSOM-9. Владелец — P1. Крейт — `lumen-layout`.

## Цель

У `position: sticky` `getComputedStyle().top` (и остальные стороны) в `%` резолвится в px от **ближайшего scrollport**: content box ближайшего предка с `overflow` ≠ visible, иначе вьюпорт. Основание: CSSOM §resolved values, css-position §sticky-pos, csswg #3115.

Значение `auto` остаётся `auto`.

## Точки входа

- [resolved_geometry.rs:263](../../crates/engine/layout/src/resolved_geometry.rs#L263): ветка `Position::Static | Position::Sticky => {}`.
- `GeomCtx` (`:65`) — добавить поле scrollport.
- `child_ctx` (`:167`) — выставлять поле на боксе со скроллируемым overflow.

## Не трогать

- Ветки relative/absolute/fixed.
- Восстановление margin.
- Сам алгоритм sticky в layout.

## Готово, когда

- `tests/wpt/css/cssom/getComputedStyle-sticky-pos-percent.html`: FAIL → PASS, ожидается `top: 250px`.
- `run_report.py --all --root css/cssom --check` даёт exit 0, baseline обновлён (`--update-expected`).
- Юнит-тест в `resolved_geometry.rs`: тот же DOM, `top: 50%` у sticky под `overflow:hidden` высотой 500px даёт `250px`.

`getComputedStyle-insets-sticky.html` сейчас целиком `TIMEOUT` (module-скрипт) — это не критерий этого среза.

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет.
