//! Paint-слой: layout tree → display list → пиксели.
//!
//! Две стадии:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - [`renderer`] рисует через wgpu (exception #2 из §5 плана).
//!
//! Экспортирует [`FontMeasurer`] — реализацию [`lumen_layout::TextMeasurer`]
//! на основе TTF-данных; используется в shell для line wrapping при layout.

pub mod atlas;
pub mod compositor;
pub mod display_list;
pub mod fallback;
pub mod hit_test;
pub mod renderer;

pub use atlas::{GlyphAtlas, GlyphEntry};
pub use fallback::CURATED_FALLBACK_FAMILIES;
pub use compositor::{
    BasicLayer, BasicLayerTree, Compositor, InProcessCompositor, Layer, LayerTree,
};
pub use display_list::{
    build_display_list, build_display_list_ordered, serialize_display_list, BlendMode,
    DisplayCommand, DisplayList,
};
pub use hit_test::{hit_test, HitTestResult};
pub use renderer::{ImageRegisterError, Renderer};

// ── FontMeasurer ────────────────────────────────────────────────────────────

use lumen_font::{Cmap, FontError, Hmtx};
use lumen_layout::TextMeasurer;

/// Реализация [`TextMeasurer`] на основе TTF-данных шрифта.
///
/// Используется в shell для передачи в [`lumen_layout::layout_measured`],
/// чтобы layout мог корректно рассчитывать ширину слов при line wrapping.
///
/// Хранит слайсы таблиц hmtx/cmap с временем жизни `'a`, привязанным к
/// байтам шрифта. Для bundled Inter (`include_bytes!`, `'static`) используй
/// `FontMeasurer::new(&font)` где `font: Font<'static>`.
pub struct FontMeasurer<'a> {
    hmtx: Hmtx<'a>,
    cmap: Cmap<'a>,
    units_per_em: u16,
}

impl<'a> FontMeasurer<'a> {
    /// Создаёт измеритель из уже разобранного [`lumen_font::Font`].
    pub fn new(font: &lumen_font::Font<'a>) -> Result<Self, FontError> {
        let head = font.head()?;
        let hmtx = font.hmtx()?;
        let cmap = font.cmap()?;
        Ok(Self { hmtx, cmap, units_per_em: head.units_per_em })
    }
}

impl<'a> TextMeasurer for FontMeasurer<'a> {
    fn char_width(&self, ch: char, font_size_px: f32) -> f32 {
        let glyph_id = self.cmap.glyph_index(ch as u32).unwrap_or(0);
        match self.hmtx.advance_width(glyph_id) {
            Some(aw) => aw as f32 * font_size_px / self.units_per_em as f32,
            // Fallback для неизвестных глифов: ~0.5em
            None => font_size_px * 0.5,
        }
    }
}
