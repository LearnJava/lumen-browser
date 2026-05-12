//! Paint-слой: layout tree → display list → пиксели.
//!
//! Phase 0 — две стадии:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - Растеризатор (renderer) появится в следующем шаге — будет рисовать
//!   через wgpu (exception #2 из §5 плана).

pub mod display_list;

pub use display_list::{build_display_list, DisplayCommand, DisplayList};
