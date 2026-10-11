# BUG-1607 — shell: `bug341_s20_keystroke_moves_subtrees_without_dispatching_workers` красный на `main`

**Статус:** OPEN
**Заведён:** 2026-10-11 (замечен при гейте BUG-1605; воспроизведён на чистом `main` `d1bf86fb9`, не из-за правки)
**Область:** shell/chrome (`crates/shell/src/tests/chrome_incremental.rs:549`)

## Симптом

`cargo test -p lumen-shell --bin lumen bug341_s20` падает: нажатие клавиши в омнибоксе запускает 1 rayon fan-out при 11 переиспользованных поддеревьях. Census: `built: 39, reused: 11, prev_index_visited: 19, fanouts: 1, display_probes: 14, style_misses: 21`. Порог «≥8 детей» считает, судя по сообщению теста, переносимых, а не строящихся детей — вероятно, `chrome.html` получил контейнер с ≥8 перестраиваемыми детьми после срезов UX-CHROME-PANELS. Там же `bug341_s21_...` падает только при параллельном прогоне (общий счётчик индекса) и проходит с `--test-threads=1`.
