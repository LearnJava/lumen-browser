# SPLIT-LT1 — layout: тестовые файлы сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-layout`, только тесты. Метод — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файлы (длины на 2026-10-02)

| Файл | Строк | Точка разреза (баннер) → новый файл |
|---|---|---|
| `crates/engine/layout/src/tests/filter_transform_snap_mask.rs` | 2042 | с `// CSS Scroll Snap L1` (927) до конца → `tests/scroll_snap_masking.rs` |
| `crates/engine/layout/src/tests/layout_generation_misc.rs` | 2137 | с `// Half-leading` (1152) до конца → `tests/half_leading_multicol.rs` (имя — по содержимому хвоста) |
| `crates/engine/layout/src/tests/scroll_interaction_misc.rs` | 2553 | с `// Scroll container tests` (1144) до конца → `tests/scroll_container.rs` |
| `crates/engine/layout/src/style/tests/values.rs` | 2049 | последние группы с `// overflow-anchor` (1934) или `// scrollbar-gutter` (1902) → соседний файл в `style/tests/` |

Регистрация новых модулей:

- для `tests/*.rs` — в `crates/engine/layout/src/lib.rs` блоком `#[cfg(test)] #[path = "tests/<name>.rs"] mod <name>;` рядом с исходным (около `lib.rs:2696-2716`);
- для `values.rs` — в `crates/engine/layout/src/style/tests/mod.rs`.

`lib.rs` (2724) уже выше своего baseline (2378), это чужой рост. Плюс 12 строк регистрации — допустимый побочный эффект этого среза. Число в baseline для `lib.rs` не трогать.

## Правило

Чисто механический перенос, без правок логики:

- тела `#[test]` и хелперов переезжают байт в байт;
- общий хелпер, нужный обоим файлам, — `pub(super)` в исходном, без копирования;
- число тестов `cargo test -p lumen-layout` до и после совпадает.

## Не трогать

Продакшн-код layout. Особенно `box_tree/layout_dispatch.rs` — это SPLIT-LB11.

## Готово, когда

- Все четыре файла ≤2000 строк.
- `python scripts/check_file_sizes.py 2>&1 | grep -E "layout/src/(tests|style/tests)"` пусто.
- Строки `scroll_interaction_misc.rs` и `style/tests/values.rs` удалены из `scripts/file-size-baseline.tsv` вручную. **Без** `--update`.
- §4 плана дополнен.

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

Только тесты — пиксели не двигаются, полный графический прогон не нужен.

## Зависимости

Нет.
