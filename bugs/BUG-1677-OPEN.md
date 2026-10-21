# BUG-1677 — flaky-тесты `lumen-js` в полном параллельном прогоне: буфер обмена и code cache

**Статус:** OPEN
**Заведён:** 2026-10-21 (P6, выделено из [BUG-1666](BUG-1666-FIXED.md))
**Область:** js (`crates/js/src/dom/tests/v8_idle_message_clipboard.rs`, `crates/js/src/v8_runtime/tests`: `perf9_code_cache_*`)

## Симптом

Три теста падают в полном параллельном прогоне `scripts/scoped-test.sh` и проходят по одному — и на `main`, и на ветке:

- `dom::tests::v8_idle_message_clipboard::navigator_clipboard_read_asks_and_returns_items`
- `v8_runtime::tests::perf9_code_cache_full_still_stores_new_entry`
- `v8_runtime::tests::perf9_code_cache_hit_preserves_semantics`

Гипотеза (не проверена): общее состояние между параллельными тестами — каталог code cache на диске и/или буфер обмена процесса.

## Проба

```
bash scripts/scoped-test.sh        # падают три теста выше
cargo test -p lumen-js --lib -- navigator_clipboard_read_asks_and_returns_items   # по одному проходит
```

## Что делать

Найти разделяемый ресурс (путь code cache, статический буфер обмена), развести его по тестам (временный каталог на тест / сериализация через `serial`-мьютекс) и добавить проверку, что параллельный прогон стабилен.

## Как проверить

Пять подряд полных `bash scripts/scoped-test.sh` без падений этих трёх тестов.
