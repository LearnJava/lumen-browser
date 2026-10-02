# BUG-1243 — `perf9_code_cache_hit_preserves_semantics` флакует под нагрузкой полного прогона

**Статус:** OPEN
**Тип:** флаки теста.
**Заведён:** 2026-10-02 (P4, найден в гейте задачи `scroll-snap-inline-axis`, не относится к ней)
**Область:** js (`crates/js/src/v8_runtime/tests/mod.rs:98`).

## Симптом

В `scripts/scoped-test.sh` (`-p lumen-js ... --lib`, ~4700 тестов параллельно) тест упал с
`a miss above the threshold must populate the cache`; в одиночном запуске
(`cargo test -p lumen-js -p lumen-driver -p lumen-shell --lib perf9_code_cache`) проходит.
Причина не разобрана: вероятно, процессный `CODE_CACHE` вытесняется/не заполняется под конкуренцией потоков.

## Как проверить

Полный `lumen-js --lib` прогон; в одиночку — зелёный.
