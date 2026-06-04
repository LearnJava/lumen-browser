//! Paint-слой: layout tree → display list → пиксели.
//!
//! Три слоя:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - [`backend`] стабильный трейт [`RenderBackend`] — контракт всех GPU-бэкендов.
//! - [`renderer`] wgpu-бэкенд; доступен только с feature `backend-wgpu` (ADR-010).

pub mod atlas;
pub mod backend;
#[cfg(any(
    feature = "backend-wgpu",
    feature = "backend-femtovg",
    feature = "backend-vello",
    feature = "backend-cpu",
    feature = "compare"
))]
pub mod backends;
pub mod backdrop_cache;
pub mod glsl;
pub mod compositor;
pub mod display_list;
pub mod fallback;
pub mod fingerprint;
pub mod hit_test;
pub mod layer_cache;
#[cfg(feature = "backend-wgpu")]
pub mod renderer;
pub mod scroll_snap;
pub mod svg_path;
#[cfg(feature = "backend-wgpu")]
pub mod texture_pool;
pub mod webgl;

#[cfg(feature = "cpu-render")]
pub mod cpu_raster;

pub use atlas::{GlyphAtlas, GlyphEntry};
pub use backend::{RenderBackend, RenderError};
#[cfg(feature = "backend-wgpu")]
pub use backends::WgpuBackend;
#[cfg(feature = "backend-femtovg")]
pub use backends::FemtovgBackend;
#[cfg(feature = "backend-vello")]
pub use backends::VelloBackend;
#[cfg(feature = "backend-cpu")]
pub use backends::CpuBackend;
#[cfg(feature = "compare")]
pub use backends::CompareBackend;
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
    hash_display_list, is_image_set, point_on_resize_grip, select_image_set_url,
    split_at_page_breaks, serialize_display_list, BlendMode, CornerRadii, DisplayCommand,
    DisplayList,
};
pub use fingerprint::GpuFingerprint;
pub use hit_test::{hit_test, HitTestResult};
pub use layer_cache::{LayerCache, LayerKey};
#[cfg(feature = "backend-wgpu")]
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

// ── MultiFontMeasurer ────────────────────────────────────────────────────────

use std::collections::HashMap;

/// Owned метрики одного шрифта, извлечённые при регистрации.
///
/// Хранит `cmap_data` (байты cmap-таблицы) и `advance_widths` (из hmtx),
/// что позволяет измерять ширину символов без хранения ссылки на оригинал.
struct OwnedFontMetrics {
    /// Байты cmap-таблицы для временного создания `Cmap<'_>` при lookup'е.
    cmap_data: Vec<u8>,
    /// advance_width по glyph_id (индекс = glyph_id; из hmtx).
    advance_widths: Vec<u16>,
    units_per_em: u16,
}

impl OwnedFontMetrics {
    fn from_bytes(bytes: &[u8]) -> Result<Self, FontError> {
        let font = lumen_font::Font::parse(bytes)?;
        let head = font.head()?;
        let maxp = font.maxp()?;
        let hmtx = font.hmtx()?;
        let cmap_data = font
            .table(b"cmap")
            .ok_or(FontError::TableNotFound(*b"cmap"))?
            .to_vec();
        let num_glyphs = maxp.num_glyphs;
        let advance_widths: Vec<u16> =
            (0..num_glyphs).map(|id| hmtx.advance_width(id).unwrap_or(0)).collect();
        Ok(Self {
            cmap_data,
            advance_widths,
            units_per_em: head.units_per_em,
        })
    }

    /// Возвращает ширину символа в px. Если глиф не найден (glyph_id == 0),
    /// возвращает `None`, чтобы вызывающий код мог попробовать следующую семью.
    fn try_char_width(&self, ch: char, font_size_px: f32) -> Option<f32> {
        let cmap = Cmap::parse(&self.cmap_data).ok()?;
        let glyph_id = cmap.glyph_index(ch as u32)?;
        if glyph_id == 0 {
            return None; // .notdef — глиф не покрыт этим шрифтом
        }
        let aw = *self.advance_widths.get(glyph_id as usize)?;
        Some(aw as f32 * font_size_px / self.units_per_em as f32)
    }
}

/// Многошрифтовый измеритель: поддерживает @font-face-загруженные шрифты.
///
/// Расширяет [`FontMeasurer`]: при вызове [`TextMeasurer::char_width_with_families`]
/// перебирает CSS `font-family` список и возвращает ширину из первого шрифта,
/// в котором есть глиф для данного символа. Если ни одна семья не подходит —
/// fallback к bundled Inter через внутренний [`FontMeasurer`].
///
/// Создаётся через [`MultiFontMeasurer::new`], дополняется семьями через
/// [`MultiFontMeasurer::register_family`].
pub struct MultiFontMeasurer {
    /// Bundled Inter fallback (всегда доступен).
    fallback: FontMeasurer<'static>,
    /// Загруженные @font-face семьи: ключ = lowercase family name.
    faces: HashMap<String, OwnedFontMetrics>,
}

impl MultiFontMeasurer {
    /// Создаёт измеритель с bundled-шрифтом как fallback.
    pub fn new(fallback_font: &lumen_font::Font<'static>) -> Result<Self, FontError> {
        Ok(Self {
            fallback: FontMeasurer::new(fallback_font)?,
            faces: HashMap::new(),
        })
    }

    /// Регистрирует @font-face шрифт под именем `family`.
    ///
    /// Если для `family` уже есть запись — она заменяется (последнее правило
    /// CSS @font-face побеждает, как в каскаде).
    /// При ошибке парсинга шрифта тихо игнорируется (не заменяет старую запись).
    pub fn register_family(&mut self, family: &str, bytes: Vec<u8>) {
        if let Ok(metrics) = OwnedFontMetrics::from_bytes(&bytes) {
            self.faces.insert(family.to_ascii_lowercase(), metrics);
        }
    }

    /// Количество зарегистрированных семей (для тестов).
    pub fn family_count(&self) -> usize {
        self.faces.len()
    }
}

impl TextMeasurer for MultiFontMeasurer {
    fn char_width(&self, ch: char, font_size_px: f32) -> f32 {
        self.fallback.char_width(ch, font_size_px)
    }

    fn char_width_with_families(&self, ch: char, font_size_px: f32, families: &[String]) -> f32 {
        for family in families {
            if let Some(metrics) = self.faces.get(&family.to_ascii_lowercase())
                && let Some(w) = metrics.try_char_width(ch, font_size_px)
            {
                return w;
            }
        }
        self.fallback.char_width(ch, font_size_px)
    }

    fn descent_px(&self, font_size_px: f32) -> f32 {
        self.fallback.descent_px(font_size_px)
    }

    fn ascent_px(&self, font_size_px: f32) -> f32 {
        self.fallback.ascent_px(font_size_px)
    }
}

#[cfg(test)]
mod multi_font_tests {
    use super::*;
    use lumen_layout::TextMeasurer;

    static INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

    fn inter_font() -> lumen_font::Font<'static> {
        lumen_font::Font::parse(INTER).expect("Inter TTF должен парситься")
    }

    #[test]
    fn new_creates_measurer_with_fallback() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        assert_eq!(m.family_count(), 0);
        // Fallback (Inter) должен давать ненулевую ширину для ASCII
        let w = m.char_width('A', 16.0);
        assert!(w > 0.0, "Inter должен дать ненулевую ширину для 'A'");
    }

    #[test]
    fn char_width_with_families_falls_back_to_inter_when_no_family_registered() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let w_direct = m.char_width('A', 16.0);
        let w_families = m.char_width_with_families('A', 16.0, &["nonexistent".to_string()]);
        assert_eq!(w_direct, w_families, "без зарегистрированных семей должен использоваться fallback");
    }

    #[test]
    fn char_width_with_empty_families_uses_fallback() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let w_direct = m.char_width('B', 20.0);
        let w_families = m.char_width_with_families('B', 20.0, &[]);
        assert_eq!(w_direct, w_families);
    }

    #[test]
    fn register_family_increases_count() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("testfont", INTER.to_vec());
        assert_eq!(m.family_count(), 1);
    }

    #[test]
    fn register_family_with_bad_bytes_is_ignored() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("broken", vec![0u8; 16]); // явно не шрифт
        assert_eq!(m.family_count(), 0, "сломанный шрифт должен тихо игнорироваться");
    }

    #[test]
    fn char_width_with_registered_family_uses_that_font() {
        // Регистрируем Inter под новым именем — должна быть та же ширина, что и от fallback
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter-copy", INTER.to_vec());
        let w_fallback = m.char_width('H', 16.0);
        let w_family = m.char_width_with_families('H', 16.0, &["inter-copy".to_string()]);
        // Inter registered → Inter fallback: должны совпадать
        assert!((w_fallback - w_family).abs() < 0.01, "ширины должны совпадать: {w_fallback} vs {w_family}");
    }

    #[test]
    fn family_lookup_is_case_insensitive() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("MyFont", INTER.to_vec());
        // Запрашиваем под разными регистрами
        let w1 = m.char_width_with_families('X', 16.0, &["myfont".to_string()]);
        let w2 = m.char_width_with_families('X', 16.0, &["MYFONT".to_string()]);
        let w3 = m.char_width_with_families('X', 16.0, &["MyFont".to_string()]);
        assert!(w1 > 0.0 && w1 == w2 && w2 == w3, "lookup должен быть case-insensitive");
    }
}
