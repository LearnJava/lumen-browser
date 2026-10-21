# BUG-1663 — юнит-тест `template_of_an_empty_grid_container_is_resolved_to_used_tracks` падает: used-значение `grid-template-columns` включает неявные имена area-линий

**Статус:** OPEN
**Заведён:** 2026-10-21 (P6, замечен при гейте BUG-1396; к `contain` не относится)
**Область:** layout (`crates/engine/layout/src/tests/grid_computed_style.rs:84`, сериализация used-значения `grid-template-columns`; вероятно след BUG-1394 — area-линии в subgrid)

## Симптом

`cargo test -p lumen-layout --lib -- template_of_an_empty_grid` (`RUST_MIN_STACK=33554432`):
`grid-template-columns` с `grid-template-areas:'a b' 'c d'` даёт `[a-start c-start] 100px [a-end b-start c-end d-start] 150px [x b-end d-end] 50px`,
тест ждёт `100px 150px [x] 50px`. Неявные имена `<area>-start`/`<area>-end` попадают в resolved value.

## Что делать

Решить по спецификации (CSS Grid L2 §7.2.3 / CSSOM: resolved value содержит явные имена линий) и либо не сериализовать неявные имена, либо поправить ожидание теста.
