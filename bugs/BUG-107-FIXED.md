# BUG-107

**Статус:** FIXED 2026-06-09
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:5254`

## Описание

flex align-content: default (`normal`→`stretch`) did not distribute free cross-space — outer `.__f` rows packed at top instead of stretched. Fix: `Auto`/`Normal` align-content behaves as `stretch` for flex; `Stretch` branch now shifts later lines down by cumulative growth of preceding lines (was computed but never applied). TEST-65 17.34%→row geometry matches Edge (pitch 181.5 vs 182).
