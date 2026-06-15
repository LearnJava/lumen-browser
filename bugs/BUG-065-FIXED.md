# BUG-065

**Статус:** FIXED 2026-06-04
**Компонент:** shell
**Файл:** `crates/shell/src/main.rs`

## Описание

Клик по ссылке `<a href>` не срабатывал: hit-test вычислял page_y = y_css + scroll_y, не вычитая TAB_BAR_HEIGHT=36px, на которую страница сдвигается через PushTransform при рендере. Исправлено в page_point, handle_click_at, dispatch_mouse_move, update_cursor_icon
