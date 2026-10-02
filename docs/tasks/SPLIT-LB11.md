# SPLIT-LB11 — `box_tree/layout_dispatch.rs` сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-layout`, продакшн. Метод — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файл (длина на 2026-10-02)

`crates/engine/layout/src/box_tree/layout_dispatch.rs` — 2050 строк. Census (`python scripts/split_census.py`):

| Item | Строки | Размер |
|---|---|---|
| `fn dispatch_box` | 582–1986 | 1405 |
| `fn lay_out_cache_checked` | 60–245 | 186 |
| `fn finalize_block_height` | 305–403 | 99 |
| `fn lay_out_inner_impl` | 485–552 | 68 |
| `fn finish_after_match` | 1987–2051 | 65 |

План: вынести кэш-обвязку (`lay_out_cache_checked` + её приватные хелперы) и `finalize_block_height` в новый `box_tree/layout_cache.rs`. Получится около 300 строк, `layout_dispatch.rs` станет около 1750.

`dispatch_box` не дробить: это один `match`, его тело — отдельная задача.

## Правило

Чисто механический перенос, без правок логики:

- `fn` в `impl` переезжают на ту же глубину, без дедента (приём SH-2);
- меняются только `use`, видимость и `mod`;
- pub-API не меняется;
- число тестов не уменьшается.

## Не трогать

- Тела функций.
- `dispatch_box`.
- Другие файлы `box_tree/`.

## Готово, когда

- `layout_dispatch.rs` ≤2000 строк, новый файл ≤2000.
- `python scripts/check_file_sizes.py 2>&1 | grep layout_dispatch` пусто.
- Полный графический прогон без пиксельных дельт.
- §4 плана дополнен.

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/run.py --continue-on-fail
```

Прогон обязателен по §5 для layout, ожидается «без дельт».

## Зависимости

Нет. С SPLIT-LT1 файлы не пересекаются.
