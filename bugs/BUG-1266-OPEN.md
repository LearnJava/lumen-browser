# BUG-1266 — `deep_grid_chain_lays_out_without_overflowing_the_stack` переполняет стек

**Статус:** OPEN
**Заведён:** 2026-10-04 (P4, найден в гейте задачи `p4-gap-rule-pixdiff-28`, не относится к ней)
**Область:** layout (`crates/engine/layout/src/box_tree/tests/grid_trampoline.rs`, `box_tree/grid_trampoline.rs`)

## Симптом

`cargo test -p lumen-layout deep_grid_chain` — `thread '...deep_grid_chain_lays_out_without_overflowing_the_stack' has overflowed its stack`, процесс тестов завершается `STATUS_STACK_OVERFLOW` (0xc00000fd), поэтому весь `lumen-layout --lib` в `scripts/scoped-test.sh` считается упавшим. Остальные 4511 тестов крейта при `--skip deep_grid_chain` зелёные.

## Как проверить

`git stash` (или чистый `origin/main`) → `cargo test -p lumen-layout deep_grid_chain`: тот же отказ, то есть дефект не от правок multicol. Соседние `deep_subgrid_chain_*`, `deep_flex_*`, `deep_multicol_*` зелёные.
