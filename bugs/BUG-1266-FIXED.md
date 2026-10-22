# BUG-1266 — `deep_grid_chain_lays_out_without_overflowing_the_stack` переполняет стек

**Статус:** FIXED 2026-10-22 (chat)
**Заведён:** 2026-10-04 (P4, найден в гейте задачи `p4-gap-rule-pixdiff-28`, не относится к ней)
**Область:** layout (`crates/engine/layout/src/box_tree/tests/grid_trampoline.rs`, `box_tree/grid_trampoline.rs`)

## Симптом

`cargo test -p lumen-layout deep_grid_chain` — `thread '...deep_grid_chain_lays_out_without_overflowing_the_stack' has overflowed its stack`, процесс тестов завершается `STATUS_STACK_OVERFLOW` (0xc00000fd), поэтому весь `lumen-layout --lib` в `scripts/scoped-test.sh` считается упавшим. Остальные 4511 тестов крейта при `--skip deep_grid_chain` зелёные.

## Как проверить

`git stash` (или чистый `origin/main`) → `cargo test -p lumen-layout deep_grid_chain`: тот же отказ, то есть дефект не от правок multicol. Соседние `deep_subgrid_chain_*`, `deep_flex_*`, `deep_multicol_*` зелёные.

## Дополнение (P1, 2026-10-19)

Та же симптоматика у `table_trampoline::deep_table_chain_lays_out_without_overflowing_the_stack` — переполнение стека на чистом `origin/main` (проверено с `multicol_flow.rs` из main; гейт задачи GRID-FRAG, не от её правок). Остальные `deep_*` проходят.

## Исправление

Сетка: итеративный `impl Clone for LayoutBox` (ветка chat-fix-grid-depth, 30cd37385). Таблица: уровень table-in-cell стоил ~9,7 КБ нативного стека (2 МиБ тестового потока — ~200 уровней при DEPTH = 500), потому что `block_flow_trampoline::step_child` (заинлайнен в `run_pass`, ~5 КБ кадра) и `table_trampoline::step_cell` оставались на цепочке, а оба трамплина держали кадры (целый `LayoutBox`) по значению. Теперь `step_child`/`step_cell` возвращают чужой dispatch в цикл (`StepOutcome::Foreign`), трамплин другого вида запускается из `run_foreign_child`/`run_cell_content`, кадры — `Box<Frame>`, ветка balance у `block_flow_trampoline::run` вынесена в `run_balanced`: ~2,2 КБ на уровень (~900 уровней). Проверка: `cargo test -p lumen-layout --lib -- table_trampoline grid_trampoline`.
