# SPLIT-PT1 — paint: три файла сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-paint`. Метод — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файлы (длины на 2026-10-02)

| Файл | Строк | Как резать |
|---|---|---|
| `crates/engine/paint/src/lib.rs` | 2189 (baseline 2133) | inline `mod multi_font_tests` (1349–2190, 842 строки) → файл-модуль `src/multi_font_tests.rs` (`#[cfg(test)] mod multi_font_tests;`) |
| `crates/engine/paint/src/display_list/tests/text_and_images.rs` | 2008 | блоки с `// Тесты <img> / DrawImage` (1666) до конца → `display_list/tests/images_media.rs` |
| `crates/engine/paint/src/display_list/tests/anim_and_chrome.rs` | 2009 (baseline) | блоки с `// BoxModelOverlay` (1159) до конца → соседний файл в `display_list/tests/` |

Тесты display list регистрируются в `crates/engine/paint/src/display_list.rs:290-318` (`#[path = "display_list/tests/…"]`). Новые модули добавлять туда же.

## Правило

Чисто механический перенос, без правок логики:

- тела тестов байт в байт;
- меняются только `use` и `mod`;
- число тестов `cargo test -p lumen-paint` до и после совпадает.

## Не трогать

- Продакшн-код paint.
- `renderer.rs`, `femtovg_backend.rs`, `cpu_raster.rs` — это отдельные батчи.

## Готово, когда

- Все три файла ≤2000 строк.
- `python scripts/check_file_sizes.py 2>&1 | grep -E "paint/src/(lib|display_list)"` пусто.
- Строки `paint/src/lib.rs` и `anim_and_chrome.rs` удалены из `scripts/file-size-baseline.tsv` вручную. **Без** `--update`.
- §4 плана дополнен.

## Гейт

```
cargo clippy -p lumen-paint --all-targets --features backend-wgpu -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

Только тесты — пиксели не двигаются.

## Зависимости

Нет.
