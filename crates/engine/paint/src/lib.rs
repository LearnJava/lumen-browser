//! Paint-слой: layout tree → display list → пиксели.
//!
//! Две стадии:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - [`renderer`] рисует через wgpu (exception #2 из §5 плана). Phase 0
//!   умеет только `FillRect`; `DrawText` будет с появлением font shaping.

pub mod display_list;
pub mod renderer;

pub use display_list::{build_display_list, DisplayCommand, DisplayList};
pub use renderer::Renderer;
