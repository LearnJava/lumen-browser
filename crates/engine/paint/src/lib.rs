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
pub mod backdrop_cache;
pub mod glsl;
pub mod compositor;
pub mod display_list;
pub mod fallback;
pub mod fingerprint;
pub mod hit_test;
pub mod layer_cache;
pub mod renderer;
pub mod scroll_snap;
pub mod svg_path;
pub mod texture_pool;
pub mod webgl;

#[cfg(feature = "cpu-render")]
pub mod cpu_raster;

pub use atlas::{GlyphAtlas, GlyphEntry};
pub use backdrop_cache::BackdropCache;
pub use fallback::CURATED_FALLBACK_FAMILIES;
pub use compositor::{
    BasicLayer, BasicLayerTree, Compositor, CompositorThread, InProcessCompositor, Layer,
    LayerTree, ThreadedCompositor, ThreadedCompositorHandle,
};
pub use display_list::{
    build_display_list, build_display_list_ordered, build_display_list_ordered_dpr,
    build_display_list_ordered_with_anim, build_display_list_ordered_with_anim_dpr,
    build_display_list_with_anim, build_print_display_list, contains_backdrop_filter,
    hash_display_list, is_image_set, select_image_set_url, split_at_page_breaks,
    serialize_display_list, BlendMode, CornerRadii, DisplayCommand, DisplayList,
};
pub use fingerprint::GpuFingerprint;
pub use hit_test::{hit_test, HitTestResult};
pub use layer_cache::{LayerCache, LayerKey};
pub use renderer::{ImageRegisterError, Renderer, SnapshotUploadError};
pub use scroll_snap::{find_scroll_snap_y, find_scroll_snap_y_proximity};
pub use webgl::SoftwareWebGl;

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
    /// Абсолютное значение hhea.descent (descent < 0 по конвенции OpenType).
    descent_units: u16,
}

impl<'a> FontMeasurer<'a> {
    /// Создаёт измеритель из уже разобранного [`lumen_font::Font`].
    pub fn new(font: &lumen_font::Font<'a>) -> Result<Self, FontError> {
        let head = font.head()?;
        let hmtx = font.hmtx()?;
        let cmap = font.cmap()?;
        let hhea = font.hhea()?;
        let descent_units = hhea.descent.unsigned_abs();
        Ok(Self { hmtx, cmap, units_per_em: head.units_per_em, descent_units })
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

    fn descent_px(&self, font_size_px: f32) -> f32 {
        self.descent_units as f32 * font_size_px / self.units_per_em as f32
    }
}
