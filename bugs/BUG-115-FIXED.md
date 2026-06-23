# BUG-115

**Статус:** OPEN
**Компонент:** css-parser
**Файл:** `crates/engine/layout/src/style.rs:15243`

## Описание

percent `background-size` (e.g. `40% 60%`, `20px 100%`) not supported — resolve_box_length returns None for `%`, so BackgroundSize falls back to Auto and the layer fills the whole positioning area instead of a percent-sized tile. TEST-45 `.no-repeat-demo`/`.repeated` residual. Needs deferred percent resolution against positioning area at paint time (like border-radius %).
