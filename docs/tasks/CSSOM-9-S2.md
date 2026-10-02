# CSSOM-9-S2 — resolved insets `position: fixed` под предком с `transform`

Остаток BUG-472. Владелец — P1. Крейт — `lumen-layout`.

## Цель

У `position: fixed` containing block — padding box ближайшего предка с `transform` (а также `filter` / `contain: paint|layout`, если эти поля уже есть в `ComputedStyle`). Только если такого предка нет, CB — вьюпорт (css-transforms-1 §2, css-position §containing block).

Для fixed с таким CB `auto`-инсеты отдаются как used px из геометрии — как уже сделано для absolute (`auto_geom`).

## Точки входа

- [resolved_geometry.rs:160](../../crates/engine/layout/src/resolved_geometry.rs#L160): `Position::Fixed => вьюпорт`.
- `:244` `let auto_geom = s.position == Position::Absolute`.
- `GeomCtx` (`:65`) — добавить `fixed_cb: Option<Rect>`.
- `child_ctx` (`:167`) — выставлять `fixed_cb` на боксе с непустым `transform` (padding box).

## Не трогать

Случай «CB = вьюпорт с прокруткой» вне среза: нужна прокрутка документа на момент layout, а `LayoutBox.scroll_x/y` — это прокрутка самого бокса.

## Готово, когда

Юнит-тест в `resolved_geometry.rs` повторяет `#container-for-fixed` из `tests/wpt/css/cssom/support/getComputedStyle-insets.js:35-44`:

- `transform: scale(1)`;
- padding 64/128;
- border 128/256;
- margin 256/512.

Для fixed-ребёнка с `top: 10%` и `left: auto` ожидаются px от padding box контейнера (300×600), а не от вьюпорта. `auto` даёт used-значение.

`getComputedStyle-insets-fixed.html` сейчас целиком `TIMEOUT` (module-скрипт). Это не критерий; если после среза он перестанет висеть, обновить baseline (`run_report.py --all --root css/cssom --update-expected`, затем `--check`).

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет. С CSSOM-9-S1 правит один файл — брать последовательно.
