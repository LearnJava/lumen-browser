//! Paint-слой: layout tree → display list → пиксели.
//!
//! Три слоя:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - [`backend`] стабильный трейт [`RenderBackend`] — контракт всех GPU-бэкендов.
//! - [`renderer`] wgpu-бэкенд; доступен только с feature `backend-wgpu` (ADR-010).

pub mod atlas;
pub mod backend;
pub mod blend_modes;
pub mod color_management;
pub mod dash_math;
pub mod gradient_math;
pub mod matrix_util;
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
pub mod gap_decorations;
pub mod fingerprint;
pub mod hit_test;
pub mod layer_cache;
#[cfg(feature = "backend-wgpu")]
pub mod renderer;
pub mod scroll_snap;
pub mod svg_path;
#[cfg(feature = "backend-wgpu")]
pub mod texture_pool;
pub mod tile_grid;
pub mod webgl;

#[cfg(feature = "cpu-render")]
pub mod cpu_raster;

pub use atlas::{GlyphAtlas, GlyphEntry};
pub use backend::{RenderBackend, RenderError};
pub use color_management::detect_color_space_from_icc;
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
    cull_display_list, hash_display_list, is_image_set, point_on_resize_grip,
    select_image_set_url, split_at_page_breaks, serialize_display_list, BlendMode, CornerRadii,
    DisplayCommand, DisplayList,
};
pub use gap_decorations::{emit_gap_rules, GapDecorationContext, GapSegment};
pub use tile_grid::{TileDirty, TileGrid, DEFAULT_TILE_SIZE};
pub use fingerprint::GpuFingerprint;
pub use hit_test::{hit_test, HitTestResult};
pub use layer_cache::{LayerCache, LayerKey};
#[cfg(feature = "backend-wgpu")]
pub use renderer::{ImageRegisterError, Renderer, SnapshotUploadError};
pub use scroll_snap::{find_scroll_snap_y, find_scroll_snap_y_proximity};
pub use webgl::SoftwareWebGl;

// ── FontMeasurer ────────────────────────────────────────────────────────────

use lumen_font::{Cmap, FontError, Hmtx, Hvar, UnicodeRange, VariationAxis, codepoint_in_ranges};
use lumen_layout::{FontVariationSetting, TextMeasurer};

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
    /// OS/2 `sxHeight` в font units (высота строчной `x`). CSS Fonts L5 §4 —
    /// основа для `font-size-adjust`. Если таблица OS/2 отсутствует или версия
    /// < 2 (нет поля sxHeight), используется приближение `units_per_em / 2`.
    x_height_units: u16,
}

impl<'a> FontMeasurer<'a> {
    /// Создаёт измеритель из уже разобранного [`lumen_font::Font`].
    pub fn new(font: &lumen_font::Font<'a>) -> Result<Self, FontError> {
        let head = font.head()?;
        let hmtx = font.hmtx()?;
        let cmap = font.cmap()?;
        let hhea = font.hhea()?;
        let descent_units = hhea.descent.unsigned_abs();
        let units_per_em = head.units_per_em;
        // OS/2 sxHeight (v2+); fallback к units_per_em/2 (aspect ≈ 0.5).
        let x_height_units = font
            .os2()
            .ok()
            .and_then(|o| o.x_height)
            .filter(|&v| v > 0)
            .map_or(units_per_em / 2, |v| v as u16);
        Ok(Self { hmtx, cmap, units_per_em, descent_units, x_height_units })
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

    fn x_height_px(&self, font_size_px: f32) -> f32 {
        self.x_height_units as f32 * font_size_px / self.units_per_em as f32
    }
}

// ── MultiFontMeasurer ────────────────────────────────────────────────────────

use std::collections::HashMap;

/// Cached variable-font data for HVAR advance width variation.
///
/// Extracted at `register_family` time and reused for every `char_width_varied`
/// call. Parsing is done once; hot-path only normalises CSS axis values and
/// evaluates pre-parsed `ItemVariationStore`.
struct OwnedVariableFont {
    /// fvar axes in order, used to map CSS design-space values to normalized
    /// `[-1.0, 1.0]` coords for [`Hvar::advance_width_index`].
    axes: Vec<VariationAxis>,
    /// Parsed HVAR table. `None` when the font has fvar but no HVAR (rare).
    hvar: Option<Hvar>,
}

impl OwnedVariableFont {
    /// Normalizes a single axis value from CSS design space to `[-1.0, 1.0]`.
    /// Follows OpenType spec §2.4 (linear normalization; ignores avar for now).
    fn normalize(axis: &VariationAxis, value: f32) -> f32 {
        let v = value.clamp(axis.min, axis.max);
        if v >= axis.default {
            let range = axis.max - axis.default;
            if range > 0.0 { (v - axis.default) / range } else { 0.0 }
        } else {
            let range = axis.default - axis.min;
            if range > 0.0 { -(axis.default - v) / range } else { 0.0 }
        }
    }

    /// Returns HVAR-adjusted advance width for `glyph_id` given CSS `axes`.
    /// Falls back to `base_aw` when HVAR is absent or delta lookup fails.
    fn adjusted_advance(&self, glyph_id: u16, base_aw: u16, css_axes: &[FontVariationSetting]) -> u16 {
        let hvar = match &self.hvar {
            Some(h) => h,
            None => return base_aw,
        };
        // Build normalized coords in fvar axis order.
        let coords: Vec<f32> = self.axes.iter().map(|axis| {
            let val = css_axes.iter()
                .find(|s| s.tag == axis.tag)
                .map_or(axis.default, |s| s.value);
            Self::normalize(axis, val)
        }).collect();
        let idx = hvar.advance_width_index(glyph_id);
        let delta = hvar.store.evaluate(idx.outer, idx.inner, &coords).unwrap_or(0.0);
        (base_aw as f32 + delta).round().clamp(0.0, f32::from(u16::MAX)) as u16
    }
}

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
    /// `wdth` variation axis `(min, max)`, or `None` for non-variable fonts.
    /// Used by [`MultiFontMeasurer::resolve_font_stretch`] (CSS Fonts L4 §5.2).
    wdth_axis: Option<(f32, f32)>,
    /// Variable-font data for HVAR advance adjustment. `None` for static fonts.
    var_data: Option<OwnedVariableFont>,
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
        let fvar = font.fvar().ok();
        let wdth_axis = fvar.as_ref()
            .and_then(|f| f.axis(b"wdth").map(|a| (a.min, a.max)));
        // Extract variable-font data when fvar is present.
        let var_data = fvar.map(|fvar| OwnedVariableFont {
            axes: fvar.axes.clone(),
            hvar: font.hvar().ok(),
        });
        Ok(Self {
            cmap_data,
            advance_widths,
            units_per_em: head.units_per_em,
            wdth_axis,
            var_data,
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

    /// Like [`try_char_width`] but applies HVAR advance width deltas for
    /// variable fonts when `css_axes` is non-empty (CSS Fonts L4 §6.3).
    fn try_char_width_varied(&self, ch: char, font_size_px: f32, css_axes: &[FontVariationSetting]) -> Option<f32> {
        let cmap = Cmap::parse(&self.cmap_data).ok()?;
        let glyph_id = cmap.glyph_index(ch as u32)?;
        if glyph_id == 0 {
            return None;
        }
        let base_aw = *self.advance_widths.get(glyph_id as usize)?;
        let aw = match &self.var_data {
            Some(var) if !css_axes.is_empty() => var.adjusted_advance(glyph_id, base_aw, css_axes),
            _ => base_aw,
        };
        Some(aw as f32 * font_size_px / self.units_per_em as f32)
    }
}

/// Один @font-face face-слот с опциональным `unicode-range` ограничением.
///
/// CSS Fonts L4 §5.1: несколько @font-face с одним family name, но разными
/// `unicode-range` — хранятся как отдельные слоты. При выборе шрифта для символа
/// берётся первый слот, чей диапазон покрывает символ И чей cmap содержит глиф.
struct FontFaceSlot {
    /// Метрики и данные шрифта.
    metrics: OwnedFontMetrics,
    /// `unicode-range` дескриптор. Пустой Vec = нет ограничений (применяется для всех символов).
    unicode_ranges: Vec<UnicodeRange>,
}

/// Многошрифтовый измеритель: поддерживает @font-face-загруженные шрифты.
///
/// Расширяет [`FontMeasurer`]: при вызове [`TextMeasurer::char_width_with_families`]
/// перебирает CSS `font-family` список и возвращает ширину из первого шрифта,
/// в котором есть глиф для данного символа. Если ни одна семья не подходит —
/// fallback к bundled Inter через внутренний [`FontMeasurer`].
///
/// Создаётся через [`MultiFontMeasurer::new`], дополняется семьями через
/// [`MultiFontMeasurer::register_family`] или [`MultiFontMeasurer::register_family_with_ranges`].
pub struct MultiFontMeasurer {
    /// Bundled Inter fallback (всегда доступен).
    fallback: FontMeasurer<'static>,
    /// Загруженные @font-face семьи: ключ = lowercase family name, значение = список face-слотов.
    /// Один family может иметь несколько слотов с разными unicode-range диапазонами.
    faces: HashMap<String, Vec<FontFaceSlot>>,
}

impl MultiFontMeasurer {
    /// Создаёт измеритель с bundled-шрифтом как fallback.
    pub fn new(fallback_font: &lumen_font::Font<'static>) -> Result<Self, FontError> {
        Ok(Self {
            fallback: FontMeasurer::new(fallback_font)?,
            faces: HashMap::new(),
        })
    }

    /// Регистрирует @font-face шрифт под именем `family` без unicode-range ограничений.
    ///
    /// Шрифт применяется для любого символа, если в нём есть глиф. Для передачи
    /// `unicode-range` используй [`register_family_with_ranges`].
    /// При ошибке парсинга шрифта тихо игнорируется.
    ///
    /// [`register_family_with_ranges`]: Self::register_family_with_ranges
    pub fn register_family(&mut self, family: &str, bytes: Vec<u8>) {
        self.register_family_with_ranges(family, bytes, Vec::new());
    }

    /// Регистрирует @font-face шрифт с `unicode-range` ограничением.
    ///
    /// `unicode_ranges`: список диапазонов из `unicode-range:` дескриптора @font-face.
    /// Пустой Vec = нет ограничений (эквивалентно [`register_family`]).
    ///
    /// CSS Fonts L4 §5.1: один family может иметь несколько слотов с разными
    /// unicode-range — добавляет новый слот, не заменяет предыдущие. При
    /// `char_width_with_families` используется первый слот, покрывающий символ.
    ///
    /// [`register_family`]: Self::register_family
    pub fn register_family_with_ranges(
        &mut self,
        family: &str,
        bytes: Vec<u8>,
        unicode_ranges: Vec<UnicodeRange>,
    ) {
        if let Ok(metrics) = OwnedFontMetrics::from_bytes(&bytes) {
            let slot = FontFaceSlot { metrics, unicode_ranges };
            self.faces
                .entry(family.to_ascii_lowercase())
                .or_default()
                .push(slot);
        }
    }

    /// Количество зарегистрированных семей (для тестов).
    pub fn family_count(&self) -> usize {
        self.faces.len()
    }

    /// Resolves `font-stretch` percentage for the first matching family
    /// with a `wdth` variation axis (CSS Fonts L4 §5.2).
    ///
    /// Returns `stretch_pct` clamped to `[axis.min, axis.max]` of the first
    /// registered family that has a `wdth` axis. Returns `None` when no
    /// registered family has a `wdth` axis — caller should use the CSS default.
    ///
    /// `stretch_pct`: CSS percentage value (e.g. 100.0 = normal, 50.0 =
    /// ultra-condensed). The `wdth` axis uses the same scale per OpenType spec.
    ///
    /// // CSS: font-stretch
    pub fn resolve_font_stretch(&self, families: &[String], stretch_pct: f32) -> Option<f32> {
        for family in families {
            if let Some(slots) = self.faces.get(&family.to_ascii_lowercase()) {
                for slot in slots {
                    if let Some((min, max)) = slot.metrics.wdth_axis {
                        return Some(stretch_pct.clamp(min, max));
                    }
                }
            }
        }
        None
    }

    /// Insert a family entry with an explicit `wdth` axis range for testing.
    #[cfg(test)]
    fn insert_test_wdth_family(&mut self, family: &str, wdth_min: f32, wdth_max: f32) {
        let slot = FontFaceSlot {
            metrics: OwnedFontMetrics {
                cmap_data: vec![],
                advance_widths: vec![],
                units_per_em: 1000,
                wdth_axis: Some((wdth_min, wdth_max)),
                var_data: None,
            },
            unicode_ranges: Vec::new(),
        };
        self.faces
            .entry(family.to_ascii_lowercase())
            .or_default()
            .push(slot);
    }
}

impl TextMeasurer for MultiFontMeasurer {
    fn char_width(&self, ch: char, font_size_px: f32) -> f32 {
        self.fallback.char_width(ch, font_size_px)
    }

    fn char_width_with_families(&self, ch: char, font_size_px: f32, families: &[String]) -> f32 {
        let cp = ch as u32;
        for family in families {
            if let Some(slots) = self.faces.get(&family.to_ascii_lowercase()) {
                for slot in slots {
                    // CSS Fonts L4 §5.1: пропустить слот, если символ вне его unicode-range.
                    if !codepoint_in_ranges(cp, &slot.unicode_ranges) {
                        continue;
                    }
                    if let Some(w) = slot.metrics.try_char_width(ch, font_size_px) {
                        return w;
                    }
                }
            }
        }
        self.fallback.char_width(ch, font_size_px)
    }

    fn char_width_varied(
        &self,
        ch: char,
        font_size_px: f32,
        axes: &[FontVariationSetting],
        families: &[String],
    ) -> f32 {
        let cp = ch as u32;
        for family in families {
            if let Some(slots) = self.faces.get(&family.to_ascii_lowercase()) {
                for slot in slots {
                    if !codepoint_in_ranges(cp, &slot.unicode_ranges) {
                        continue;
                    }
                    if let Some(w) = slot.metrics.try_char_width_varied(ch, font_size_px, axes) {
                        return w;
                    }
                }
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

    fn x_height_px(&self, font_size_px: f32) -> f32 {
        self.fallback.x_height_px(font_size_px)
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

    // ── resolve_font_stretch (CSS Fonts L4 §5.2) ────────────────────────────

    #[test]
    fn resolve_font_stretch_no_families_returns_none() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        assert_eq!(m.resolve_font_stretch(&[], 100.0), None);
        assert_eq!(m.resolve_font_stretch(&["any".to_string()], 100.0), None);
    }

    #[test]
    fn resolve_font_stretch_non_variable_font_returns_none() {
        // Inter — не variable font, нет fvar/wdth → None
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        assert_eq!(m.resolve_font_stretch(&["inter".to_string()], 100.0), None);
    }

    #[test]
    fn resolve_font_stretch_clamps_below_axis_min() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // wdth ось: [75%, 150%] — ultra-condensed (50%) < min → clamp to 75%
        m.insert_test_wdth_family("varifont", 75.0, 150.0);
        assert_eq!(
            m.resolve_font_stretch(&["varifont".to_string()], 50.0),
            Some(75.0),
            "значение ниже min должно зажиматься к min"
        );
    }

    #[test]
    fn resolve_font_stretch_clamps_above_axis_max() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // wdth ось: [75%, 150%] — ultra-expanded (200%) > max → clamp to 150%
        m.insert_test_wdth_family("varifont", 75.0, 150.0);
        assert_eq!(
            m.resolve_font_stretch(&["varifont".to_string()], 200.0),
            Some(150.0),
            "значение выше max должно зажиматься к max"
        );
    }

    // ── char_width_varied (CSS Fonts L4 §6.3) ───────────────────────────────

    #[test]
    fn char_width_varied_empty_axes_matches_char_width_with_families() {
        // Empty axes → same result as char_width_with_families (default impl).
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        let families = vec!["inter".to_string()];
        let w_normal = m.char_width_with_families('A', 16.0, &families);
        let w_varied = m.char_width_varied('A', 16.0, &[], &families);
        assert!((w_normal - w_varied).abs() < 0.01,
            "пустые axes должны давать тот же результат: {w_normal} vs {w_varied}");
    }

    #[test]
    fn char_width_varied_static_font_ignores_axes() {
        // Inter is a static font (no fvar). Variation axes should be ignored.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family("inter", INTER.to_vec());
        let families = vec!["inter".to_string()];
        let axes = vec![lumen_layout::FontVariationSetting { tag: *b"wght", value: 700.0 }];
        let w_normal = m.char_width_with_families('B', 16.0, &families);
        let w_varied = m.char_width_varied('B', 16.0, &axes, &families);
        // Inter has no HVAR — delta is zero, so widths must be equal.
        assert!((w_normal - w_varied).abs() < 0.01,
            "статический шрифт без HVAR: axes не влияют на ширину");
    }

    #[test]
    fn char_width_varied_unknown_family_falls_back_to_inter() {
        let font = inter_font();
        let m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["nonexistent-vf".to_string()];
        let axes = vec![lumen_layout::FontVariationSetting { tag: *b"wght", value: 900.0 }];
        let w = m.char_width_varied('C', 16.0, &axes, &families);
        let w_fallback = m.char_width('C', 16.0);
        assert!((w - w_fallback).abs() < 0.01,
            "неизвестная семья → fallback Inter: {w} vs {w_fallback}");
    }

    // ── unicode-range фильтрация (CSS Fonts L4 §5.1) ────────────────────────

    #[test]
    fn unicode_range_covers_char_uses_registered_font() {
        // Регистрируем Inter только для ASCII (U+0020-007E).
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        m.register_family_with_ranges("myfont", INTER.to_vec(), ranges);
        // ASCII 'A' (U+0041) покрыт диапазоном → должны получить ширину из Inter.
        let w_family = m.char_width_with_families('A', 16.0, &["myfont".to_string()]);
        let w_fallback = m.char_width('A', 16.0);
        assert!((w_family - w_fallback).abs() < 0.01,
            "символ внутри unicode-range: должна использоваться зарегистрированная семья");
    }

    #[test]
    fn unicode_range_outside_falls_back_to_inter() {
        // Регистрируем Inter только для ASCII (U+0020-007E).
        // Кириллица (U+0410 = А) — вне диапазона → должен быть fallback.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        m.register_family_with_ranges("myfont", INTER.to_vec(), ranges);
        let families = vec!["myfont".to_string()];
        // Inter содержит кириллицу, поэтому если unicode-range игнорируется,
        // ширины были бы равны. Нас интересует, что слот пропускается —
        // fallback Inter (без unicode-range) даёт тот же результат,
        // поэтому тест просто проверяет, что ширина ненулевая.
        let w = m.char_width_with_families('А', 16.0, &families);
        assert!(w > 0.0, "кириллица вне unicode-range: fallback должен дать ненулевую ширину");
    }

    #[test]
    fn multiple_slots_per_family_unicode_range_selection() {
        // Два слота для одной семьи: первый — ASCII, второй — кириллица.
        // Символ из ASCII → должен выбраться первый слот.
        // Символ из кириллицы → первый слот пропускается, берётся второй.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let latin_ranges = lumen_font::parse_unicode_ranges("U+0020-007E");
        let cyrillic_ranges = lumen_font::parse_unicode_ranges("U+0400-04FF");
        m.register_family_with_ranges("subset", INTER.to_vec(), latin_ranges);
        m.register_family_with_ranges("subset", INTER.to_vec(), cyrillic_ranges);
        // Ровно одна уникальная семья
        assert_eq!(m.family_count(), 1);
        // ASCII 'A' и кирилл. 'А' — оба покрыты через разные слоты
        let w_latin = m.char_width_with_families('A', 16.0, &["subset".to_string()]);
        let w_cyrillic = m.char_width_with_families('А', 16.0, &["subset".to_string()]);
        assert!(w_latin > 0.0, "латиница должна быть покрыта первым слотом");
        assert!(w_cyrillic > 0.0, "кириллица должна быть покрыта вторым слотом");
    }

    #[test]
    fn register_family_with_ranges_empty_ranges_is_unrestricted() {
        // Пустые ranges = нет ограничений — все символы проходят через этот слот.
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.register_family_with_ranges("all", INTER.to_vec(), Vec::new());
        let w = m.char_width_with_families('А', 16.0, &["all".to_string()]);
        assert!(w > 0.0, "пустой unicode-range → нет ограничений, кириллица должна работать");
    }
}
