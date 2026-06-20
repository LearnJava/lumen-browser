# BUG-224

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`)
**Тест:** `crates/driver/tests/test_33.rs::test_33_multi_column`

## Описание

Регрессия высоты multi-column контейнера с `column-span: all` (auto-height).
Юнит-тест `test_33_multi_column` падает на чистом main (HEAD `7b242e60`):

```
.mc[4] should be 660x88, got 660x64
```

`mc[4]` — `column-count:3; gap:12; column-span:all`, высота должна выводиться из
контента (ground-truth 88px по `--dump-layout`), сейчас 64px. Появилось после
влития ветки `p3-bug198-objectfit-svg` (`7b242e60`), которая переписала
`box_tree.rs` (~204 строки, inline SVG object-fit) — побочно затронула расчёт
auto-высоты multi-column с `column-span: all`.

## Воспроизведение

```bash
cargo test -p lumen-driver --test test_33
# .mc[4] should be 660x88, got 660x64
```

## Влияние

`cargo test --workspace` красный (1 падение). Пиксельный паритет TEST-33 при этом
в норме (`run.py --ipc` TEST-33 ≈ 0.10%), поэтому визуально малозаметно — но
расчётная высота контейнера расходится с ground-truth. Не блокирует другие
crate-тесты (изолировано в lumen-driver test_33).

## Как чинить

Сверить изменения `box_tree.rs` из `p3-bug198` против ветки расчёта высоты
блока с `column-span: all` (auto-height из суммарной высоты колонок + span-ряда).
Регрессионный гард уже есть — `test_33_multi_column`.
