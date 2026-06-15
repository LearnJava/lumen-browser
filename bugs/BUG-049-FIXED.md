# BUG-049

**Статус:** FIXED 2026-05-30
**Компонент:** shell
**Файл:** `shell/src/main.rs:4219,4272`

## Описание

lumen-shell не компилируется: non-exhaustive match по DisplayCommand в content_height_of/content_width_of — новый вариант PageBreak (p2 print-pages merge) не обработан; маркер пагинации печати, не контент, без rect → ветка continue (как BUG-048)
