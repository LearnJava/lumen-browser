# BUG-1100 — 8 файлов перевалили потолок 2000 строк (§5.1) мимо дорожки SPLIT

**Статус:** OPEN
**Заведён:** 2026-09-23 (P6, побочная находка при закрытии [BUG-1002](BUG-1002-FIXED.md))
**Область:** дорожка SPLIT (`docs/tasks/p1-monolith-split-queue.md`), владелец — P1
**Владелец:** не назначен

## Симптом

При разборе BUG-1002 (`check_file_sizes.py` красный на `main`) обнаружились
8 файлов, перешагнувших потолок 2000 строк (§5.1) без единой строки в
`scripts/file-size-baseline.tsv` — то есть выросших ПОСЛЕ 2026-08-26 (когда
дорожка SPLIT в последний раз пересчитывала список) и никем не замеченных,
потому что `file-size` — только CI job, не входит в 7-шаговый чек-лист
`/lumen-task-finish`:

```
crates/engine/css-parser/src/parser/tests/at_rules.rs   2185
crates/engine/layout/src/style/tests/values.rs          2039
crates/js/src/canvas2d.rs                               2002
crates/shell/src/app/window_event/redraw_requested.rs   2010
crates/shell/src/chrome_ui.rs                           2058
crates/shell/src/page_load.rs                           2653
crates/shell/src/tests/chrome_incremental.rs            2080
crates/shell/src/tests/scripts_and_frames.rs            2066
```

BUG-1002 закрыт добавлением этих 8 в baseline (`--update`) — это восстанавливает
зелёный гейт, но не режет ни одного файла; правило §5.1 «новый монолит заводить
нельзя» на них уже не действует (задним числом), они просто встали в очередь
на разрез наравне со старыми гигантами.

## Почему отдельный баг, а не расширение BUG-1002

BUG-1002 — про красный CI-гейт, чинится обновлением baseline за минуты. Разрез
8 файлов — механический перенос кода между модулями по методу дорожки SPLIT
(`python scripts/split_census.py <file>`, батч = отдельная сессия P1), не
входит в скоуп полосы P6 («не расширяет выданный пункт на соседнюю работу»).

## Первый шаг

P1: добавить 8 файлов в перепись `docs/tasks/p1-monolith-split-queue.md` §1
(из них 4 — тестовые файлы: `at_rules.rs`, `values.rs`, `chrome_incremental.rs`,
`scripts_and_frames.rs` — резать их дешевле, чем продакшн-монолиты, обычно
достаточно разбить по `#[cfg(test)] mod`), завести строки в `ROADMAP.md`
(`SPLIT-*`) и указатели в `STATUS-P1.md` по образцу уже существующих батчей.
