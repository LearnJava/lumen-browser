# SPLIT-ST19 — shell: тестовые файлы сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-shell`, только `src/tests/`. Метод — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файлы (длины на 2026-10-02)

| Файл | Строк (baseline) | Точка разреза → новый файл |
|---|---|---|
| `crates/shell/src/tests/scripts_and_frames.rs` | 2220 (2066) | блоки BUG-480 срезы 16–18 и FRAME-3, с `// pointer_target` (762) до конца → `tests/subdocument_frames.rs` |
| `crates/shell/src/tests/chrome_incremental.rs` | 2122 (2080) | с `// BUG-405 slice 49` (1222) до конца → `tests/chrome_incremental_dl.rs` |
| `crates/shell/src/tests/bug341_census.rs` | 2104 (2036) | 24 функции без баннеров. По `split_census.py` вынести вторую половину функций → `tests/bug341_census_b.rs`, общие хелперы `pub(super)` |

Регистрация — в `crates/shell/src/tests/mod.rs` рядом с исходными.

## Правило

Чисто механический перенос, без правок логики:

- тела тестов байт в байт;
- меняются только `use`, видимость хелперов и `mod`;
- число тестов `cargo test -p lumen-shell` до и после совпадает.

## Не трогать

- Продакшн-код shell: `relayout.rs`, `about_to_wait.rs` и прочее. Shell-продакшн идёт после BUG-935.
- `page_load.rs`, `frames.rs`, `forms.rs`.

## Готово, когда

- Все три файла ≤2000 строк.
- `python scripts/check_file_sizes.py 2>&1 | grep shell/src/tests` пусто.
- Три строки удалены из `scripts/file-size-baseline.tsv` вручную. **Без** `--update`.
- §4 плана дополнен.

## Гейт

```
cargo clippy -p lumen-shell --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет. Файлы тестов не пересекаются с BUG-935 (`relayout.rs`), но перед merge сверить, не дописал ли BUG-935 тесты в эти файлы.
