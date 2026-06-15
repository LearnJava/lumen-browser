# BUG-048

**Статус:** FIXED 2026-05-30
**Компонент:** shell
**Файл:** `shell/src/main.rs:4219,4271`

## Описание

lumen-shell не компилируется: non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новый вариант DrawScrollbar (p2-scrollbar-rendering merge) не обработан; скроллбар — UI, не контент → ветка continue (как BUG-044)
