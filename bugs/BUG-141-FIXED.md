# BUG-141

**Статус:** FIXED 2026-06-13
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:6326`

## Описание

TEST-71 17.83%: misdiagnosed as @starting-style leak — actual cause was flex align-items:center in a non-wrapping container ignoring the container's cross size. dump-layout/dump-display-list confirmed @starting-style rules do NOT leak (opacity=1, no transform); the two boxes rendered at y=1 instead of y=260 because align-items used line_cross (tallest item=200px) instead of the explicit container height (718px). Fix: non-wrap flex line cross size = explicit_cross when definite (CSS Flexbox §9.5); align-items:stretch no longer grows items that have an explicit height.
