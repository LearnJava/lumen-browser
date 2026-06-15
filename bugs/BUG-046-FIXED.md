# BUG-046

**Статус:** FIXED 2026-05-30
**Компонент:** layout
**Файл:** `layout/src/lib.rs:12253,12269,979`

## Описание

3 устаревших теста lumen-layout --lib: webp теперь декодируется (в supported_mime_types) → picture-тесты обновлены (avif для fallback, webp для supported); non_cell_col_row_span: `lay` возвращает body-box напрямую, убран лишний first_element_child
