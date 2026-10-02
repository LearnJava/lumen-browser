# BUG-1244 — `skipped_computed_style_entries_equal_the_rebuilt_ones` падает в полном прогоне lumen-js

**Статус:** FIXED 2026-10-02 (P1)
**Тип:** флаки теста (или зависимость от порядка/нагрузки).
**Заведён:** 2026-10-02 (P4, найден в гейте задачи `overscroll-behavior`, не относится к ней)
**Область:** js (`crates/js/src/dom/tests/v8_bug935_s59_style_skip.rs:176`).

## Симптом

В `scripts/scoped-test.sh` (`-p lumen-js ... --lib`, ~4700 тестов параллельно) тест упал:
`append_remove_on_body: no entry was left published — the skip never engaged`.
В узком запуске (`cargo test -p lumen-js -p lumen-shell --lib -- v8_bug935_s59 ...`) проходит.
Правки P4 (`resolve_scroll_chain_target`, shell/scrolling.rs) к этому пути не относятся.

## Как проверить

Полный `lumen-js --lib` прогон; затем `-- v8_bug935_s59` в одиночку.

## Причина и исправление

Не нагрузка, а общее состояние: `v8_bug935_s55_content_journal::journal_driven_flush_…` переключает
процессный `CONTENT_JOURNAL_DISABLED` (половина его времени журнал выключен), и параллельный S59-тест,
считающий оставленные записи, шёл с `ContentDirty::Untracked` — `ChangedNodes` не строится, пропуск не
срабатывает. Воспроизведение: `-- v8_bug935` (6 тестов) падал в ~4 из 6 запусков, `--test-threads=1` — 0.
Исправление (BUG-935 срез 60): `JOURNAL_SWITCH` — мьютекс, который держат переключатель и S59-тест.
