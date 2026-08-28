//! Крупные ветки `match event` из `Lumen::window_event` (SPLIT-SH2).
//!
//! Четыре ветки из тринадцати занимали 3 087 строк из 3 306 — по файлу на
//! ветку. Остальные девять коротки и остались на месте, в самом `match`.
//!
//! Тело ветки стало `pub(crate) fn on_<ветка>` в `impl Lumen`, а сама ветка —
//! вызовом в одну строку. Ранний `return` внутри ветки при этом означает ровно
//! то же самое, что и раньше: `match` — последний оператор `window_event`, за
//! ним ничего не выполняется, поэтому выход из метода и выход из ветки
//! неразличимы.

pub(crate) mod cursor_moved;
pub(crate) mod mouse_input;
pub(crate) mod mouse_wheel;
pub(crate) mod redraw_requested;
