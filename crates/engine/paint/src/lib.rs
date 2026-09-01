//! Paint-слой: layout tree → display list → пиксели.
//!
//! Три слоя:
//! - [`display_list`] чистая логика: обход дерева layout, генерация
//!   независимых от backend команд.
//! - [`backend`] стабильный трейт [`RenderBackend`] — контракт всех GPU-бэкендов.
//! - [`renderer`] wgpu-бэкенд; доступен только с feature `backend-wgpu` (ADR-010).

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

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
#[cfg(any(feature = "backend-wgpu", feature = "backend-femtovg"))]
pub mod chrome_fonts;
pub mod backdrop_cache;
pub mod display_list_cache;
pub mod glsl;
pub mod compositor;
pub mod display_list;
pub mod fallback;
pub mod gap_decorations;
pub mod fingerprint;
pub mod hit_test;
mod invariants;
pub mod layer_cache;
pub mod overlay_partition;
#[cfg(feature = "backend-wgpu")]
pub mod backend_probe;
#[cfg(feature = "backend-wgpu")]
pub mod renderer;
pub mod scroll_cache;
pub mod scroll_snap;
pub mod svg_path;
pub mod varied_text;
#[cfg(feature = "backend-wgpu")]
pub mod texture_pool;
pub mod tile_grid;
#[cfg(feature = "backend-wgpu")]
pub mod webgpu_compute;
pub mod webgl;

#[cfg(feature = "cpu-render")]
pub mod cpu_raster;

pub use atlas::{GlyphAtlas, GlyphEntry};
pub use backend::{RenderBackend, RenderError};

/// Уровень покадрового лога производительности (`LUMEN_FRAME_LOG`).
///
/// Диагностический инструмент: бэкенды печатают в stderr строки `[frame] …`
/// с временем paint-фазы и размером display list на каждый кадр.
/// `0` — выключен (по умолчанию), `1` — сводка по кадру, `2` — дополнительно
/// разбивка времени по типам DisplayCommand (top-8 за кадр). Значение читается
/// из окружения один раз за процесс (нулевая стоимость в горячем цикле).
pub fn frame_log_level() -> u8 {
    use std::sync::OnceLock;
    static LEVEL: OnceLock<u8> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("LUMEN_FRAME_LOG")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0)
    })
}

/// `true`, если включён покадровый лог производительности (`LUMEN_FRAME_LOG>=1`).
pub fn frame_log_enabled() -> bool {
    frame_log_level() >= 1
}

/// `true`, если включён scroll-blit путь рендера (ADR-016 M3.2.1).
///
/// Когда включён, бэкенды рисуют content не прямо в экран, а в удерживаемую
/// offscreen-поверхность и блитают её по дельте скролла — сокращая работу
/// GPU на in-band прокрутке (M3.2.1b). По умолчанию **включён** с M3.2.1c-7;
/// kill-switch: `LUMEN_SCROLL_BLIT=0` (или `false`). Явный `LUMEN_SCROLL_BLIT=1`
/// тоже работает (совместимость с тестовыми скриптами). Значение читается из
/// окружения один раз за процесс (нулевая стоимость в горячем цикле).
#[must_use]
pub fn scroll_blit_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        match std::env::var("LUMEN_SCROLL_BLIT").as_deref() {
            Ok("0") | Ok("false") | Ok("False") | Ok("FALSE") => false,
            Ok("1") | Ok("true") | Ok("True") | Ok("TRUE") => true,
            _ => true, // default on (M3.2.1c-7)
        }
    })
}

/// Аккумулятор времён кадров для сессионной сводки (`LUMEN_FRAME_LOG`).
///
/// Собирает миллисекунды кадра (полное время цикла redraw) и по запросу считает
/// перцентили p50/p95/p99, min/max и число кадров. Прослойка M0.1 плана
/// многопоточного рендера (ADR-016): каждая последующая стадия ссылается на
/// before/after числа этой сводки, поэтому сводка живёт в `lumen-paint`, а не в
/// шелле, и покрыта юнит-тестами перцентильной арифметики.
///
/// [`record`][FrameStats::record] дёшев (push в `Vec`); сортировка происходит
/// только в [`summary`][FrameStats::summary], который дёргается по кадансу
/// `LUMEN_MEM_REPORT` и один раз на выходе.
#[derive(Debug, Default)]
pub struct FrameStats {
    /// Времена всех учтённых кадров, мс, в порядке поступления.
    samples: Vec<f32>,
}

/// Перцентильная сводка по временам кадров за сессию (миллисекунды).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSummary {
    /// Количество учтённых кадров.
    pub count: usize,
    /// Минимальное время кадра, мс.
    pub min_ms: f32,
    /// Медиана (p50), мс.
    pub p50_ms: f32,
    /// 95-й перцентиль, мс.
    pub p95_ms: f32,
    /// 99-й перцентиль, мс.
    pub p99_ms: f32,
    /// Максимальное время кадра, мс.
    pub max_ms: f32,
}

impl FrameStats {
    /// Создаёт пустой аккумулятор.
    pub fn new() -> Self {
        Self::default()
    }

    /// Учитывает время одного кадра (мс). Значения NaN/inf/отрицательные
    /// игнорируются, чтобы не портить перцентили.
    pub fn record(&mut self, ms: f32) {
        if ms.is_finite() && ms >= 0.0 {
            self.samples.push(ms);
        }
    }

    /// Количество учтённых кадров.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// `true`, если ни одного кадра ещё не учтено.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Считает перцентильную сводку. Возвращает `None` для пустой выборки.
    ///
    /// Перцентиль берётся методом «ближайшего ранга» (nearest-rank) по
    /// отсортированной копии выборки; исходный `samples` не мутируется, чтобы
    /// [`record`][FrameStats::record] оставался дешёвым.
    pub fn summary(&self) -> Option<FrameSummary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len();
        // nearest-rank: индекс = ceil(p/100 * N) - 1, зажатый в [0, N-1].
        let pct = |p: f32| -> f32 {
            let rank = ((p / 100.0) * n as f32).ceil() as usize;
            sorted[rank.saturating_sub(1).min(n - 1)]
        };
        Some(FrameSummary {
            count: n,
            min_ms: sorted[0],
            p50_ms: pct(50.0),
            p95_ms: pct(95.0),
            p99_ms: pct(99.0),
            max_ms: sorted[n - 1],
        })
    }
}

impl FrameSummary {
    /// Оборачивает сводку в [`Display`] с произвольным префиксом-меткой вместо
    /// жёсткого `FRAME_SUMMARY`.
    ///
    /// Разные подсистемы MT-рендера (ADR-016) печатают одну и ту же
    /// перцентильную сводку под своей меткой: кадры — `FRAME_SUMMARY`
    /// (см. [`Display`] ниже), время relayout на UI-потоке — `ENGINE_SUMMARY`
    /// (M2.0). Формат и арифметика едины, различается только префикс.
    pub fn display_with<'a>(&'a self, label: &'a str) -> LabeledSummary<'a> {
        LabeledSummary { summary: self, label }
    }
}

/// [`Display`]-обёртка над [`FrameSummary`] с произвольной меткой-префиксом
/// (см. [`FrameSummary::display_with`]).
pub struct LabeledSummary<'a> {
    /// Сводка, чьи перцентили печатаются.
    summary: &'a FrameSummary,
    /// Префикс строки (напр. `FRAME_SUMMARY` / `ENGINE_SUMMARY`).
    label: &'a str,
}

impl std::fmt::Display for LabeledSummary<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.summary;
        write!(
            f,
            "{} count={} min={:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms max={:.2}ms",
            self.label, s.count, s.min_ms, s.p50_ms, s.p95_ms, s.p99_ms, s.max_ms
        )
    }
}

impl std::fmt::Display for FrameSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.display_with("FRAME_SUMMARY"), f)
    }
}

#[cfg(test)]
mod frame_stats_tests {
    use super::FrameStats;

    #[test]
    fn empty_has_no_summary() {
        let s = FrameStats::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert!(s.summary().is_none());
    }

    #[test]
    fn ignores_non_finite_and_negative() {
        let mut s = FrameStats::new();
        s.record(f32::NAN);
        s.record(f32::INFINITY);
        s.record(-1.0);
        assert!(s.is_empty());
        s.record(5.0);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn single_sample_all_percentiles_equal() {
        let mut s = FrameStats::new();
        s.record(12.5);
        let sum = s.summary().expect("one sample");
        assert_eq!(sum.count, 1);
        assert_eq!(sum.min_ms, 12.5);
        assert_eq!(sum.p50_ms, 12.5);
        assert_eq!(sum.p95_ms, 12.5);
        assert_eq!(sum.p99_ms, 12.5);
        assert_eq!(sum.max_ms, 12.5);
    }

    #[test]
    fn percentiles_nearest_rank_1_to_100() {
        // Выборка 1..=100; nearest-rank: p50→50, p95→95, p99→99, max→100.
        let mut s = FrameStats::new();
        for i in 1..=100 {
            s.record(i as f32);
        }
        let sum = s.summary().expect("100 samples");
        assert_eq!(sum.count, 100);
        assert_eq!(sum.min_ms, 1.0);
        assert_eq!(sum.p50_ms, 50.0);
        assert_eq!(sum.p95_ms, 95.0);
        assert_eq!(sum.p99_ms, 99.0);
        assert_eq!(sum.max_ms, 100.0);
    }

    #[test]
    fn display_with_uses_custom_label_same_numbers() {
        let mut s = FrameStats::new();
        s.record(12.5);
        let sum = s.summary().expect("one sample");
        // Default Display keeps the FRAME_SUMMARY prefix.
        assert!(format!("{sum}").starts_with("FRAME_SUMMARY count=1"));
        // A custom label swaps only the prefix; the percentile tail is identical.
        let engine = format!("{}", sum.display_with("ENGINE_SUMMARY"));
        assert!(engine.starts_with("ENGINE_SUMMARY count=1"));
        assert_eq!(
            engine.trim_start_matches("ENGINE_SUMMARY"),
            format!("{sum}").trim_start_matches("FRAME_SUMMARY"),
        );
    }

    #[test]
    fn summary_does_not_mutate_insertion_order() {
        let mut s = FrameStats::new();
        for v in [30.0, 10.0, 20.0] {
            s.record(v);
        }
        let _ = s.summary();
        // Повторный вызов даёт тот же результат (samples не отсортированы на месте).
        let a = s.summary().unwrap();
        let b = s.summary().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.min_ms, 10.0);
        assert_eq!(a.max_ms, 30.0);
    }
}
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
pub use display_list_cache::{CachedDisplayLayer, DisplayListCache, hash_commands};
pub use fallback::CURATED_FALLBACK_FAMILIES;
pub use compositor::{
    BasicLayer, BasicLayerTree, Compositor, CompositorThread, InProcessCompositor, Layer,
    LayerTree, ThreadedCompositor, ThreadedCompositorHandle,
};
pub use display_list::{
    build_display_list, build_display_list_ordered, build_display_list_ordered_dpr,
    build_display_list_ordered_with_anim, build_display_list_ordered_with_anim_dpr,
    build_display_list_ordered_with_anim_split,
    build_display_list_with_anim, build_print_display_list, contains_backdrop_filter,
    cull_display_list, hash_content, hash_display_list, is_image_set, patch_scroll_layer,
    point_on_resize_grip, select_image_set_url, split_at_page_breaks, serialize_display_list,
    strip_background_graphics,
    BlendMode, CornerRadii,
    DisplayCommand, DisplayList, FrameDelta, FrameFingerprint, ProvenanceIndex, ProvenanceSpan,
};
pub use gap_decorations::{emit_gap_rules, GapDecorationContext, GapSegment};
pub use tile_grid::{TileDirty, TileGrid, DEFAULT_TILE_SIZE};
pub use scroll_cache::{ScrollCache, ScrollFramePlan, DEFAULT_OVERSCAN};
pub use overlay_partition::{
    has_overlay, is_compositing_layer_open, is_spatial_layer_open, overlay_ranges, plan_overlays,
    plan_overlays_nested, spatial_layer_close, NestedOverlayPlan, OverlayPlan, OverlaySpan,
};
pub use fingerprint::GpuFingerprint;
pub use invariants::{count_paint_violations, PaintViolationCounts};
pub use hit_test::{hit_test, hit_test_all, HitTestResult};
pub use layer_cache::{LayerCache, LayerKey};
#[cfg(feature = "backend-wgpu")]
pub use renderer::{
    last_compose, load_counter, ComposeOutcome, ImageRegisterError, Renderer,
    SnapshotUploadError, DL_EPOCH_MISMATCHES, DL_FOLD_REUSED, FRAMES_RENDERED, FRAMES_SKIPPED,
    FRAME_LOG_NANOS, FRAME_PHASE_NANOS, POST_CACHE_NANOS, PRE_MARKS_NANOS, TAIL_NANOS,
};
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
    ascent_units: u16,
    descent_units: u16,
    x_height_units: u16,
}

impl<'a> FontMeasurer<'a> {
    pub fn new(font: &lumen_font::Font<'a>) -> Result<Self, FontError> {
        let head = font.head()?;
        let hmtx = font.hmtx()?;
        let cmap = font.cmap()?;
        let hhea = font.hhea()?;
        let units_per_em = head.units_per_em;
        let (ascent_units, descent_units) = match font.os2() {
            Ok(os2) => (os2.typo_ascender.unsigned_abs(), os2.typo_descender.unsigned_abs()),
            Err(_) => (hhea.ascent.unsigned_abs(), hhea.descent.unsigned_abs()),
        };
        let x_height_units = font
            .os2()
            .ok()
            .and_then(|o| o.x_height)
            .filter(|&v| v > 0)
            .map_or(units_per_em / 2, |v| v as u16);
        Ok(Self {
            hmtx, cmap, units_per_em,
            ascent_units, descent_units, x_height_units,
        })
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

    fn ascent_px(&self, font_size_px: f32) -> f32 {
        let total = self.ascent_units as f32 + self.descent_units as f32;
        if total > 0.0 {
            (self.ascent_units as f32 / total) * font_size_px
        } else {
            font_size_px * 0.8
        }
    }

    fn x_height_px(&self, font_size_px: f32) -> f32 {
        self.x_height_units as f32 * font_size_px / self.units_per_em as f32
    }
}

// ── MultiFontMeasurer ────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Arc;

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

/// Сколько РЕЗОЛВЛЕННЫХ системных семейств [`SystemFaceSet`] держит в
/// ленивом кэше. Каждая запись — cmap + таблица advance-ов шрифта (сотни
/// килобайт), а набор живёт всё время процесса, поэтому страница со списком
/// из тысячи выдуманных имён не должна раздувать память.
///
/// По достижении предела новые имена кэшируются как «не резолвится» и
/// меряются bundled Inter-ом. Отрицательные записи предел не расходуют — они
/// стоят одну строку.
const MAX_CACHED_NAMED_FACES: usize = 64;

/// Метрики системных face-ов, которые для страницы выберет рендер: CSS
/// generic-семейства (`serif` / `sans-serif` / `monospace` / `cursive` /
/// `fantasy` / `system-ui`, CSS Fonts L4 §3.1) и конкретные системные
/// семейства по имени (`Arial`, `"Times New Roman"`, …).
///
/// BUG-128: рендер резолвит имена семейств в реальные системные face-ы
/// ([`lumen_core::ext::FontProvider::pick_generic_face`] и
/// [`lumen_core::ext::FontProvider::pick_face`]), а layout мерил их
/// bundled-Inter-ом — ширины строк не совпадали с нарисованными глифами.
/// Этот набор даёт измерителю те же метрики, что видит рендер.
///
/// Generic-и (их ровно 6) резолвятся сразу при постройке, конкретные имена —
/// лениво при первом измерении: список установленных в системе семейств
/// открыт, читать их все вперёд нельзя.
///
/// Строится ОДИН раз (скан системного индекса + чтение файлов шрифтов —
/// десятки мегабайт I/O), затем шарится между пересборками
/// [`MultiFontMeasurer`] через `Arc`: измеритель пересоздаётся на каждом
/// relayout, а этот набор — нет. Поэтому и ленивый кэш переживает relayout.
pub struct SystemFaceSet {
    /// `(generic-имя, метрики выбранного face-а)`. Generic-и, для которых на
    /// машине не нашлось ни одного кандидата, в набор не попадают.
    generics: Vec<(&'static str, Arc<OwnedFontMetrics>)>,
    /// Провайдер для ленивого резолва конкретных семейств. `None` — путь без
    /// системных шрифтов (тесты, детерминированный CPU-рендер): такие имена
    /// меряются bundled Inter-ом.
    provider: Option<Arc<dyn lumen_core::ext::FontProvider>>,
    /// Ленивый кэш конкретных семейств: ключ = lowercase имя, значение =
    /// метрики либо `None` («в системе нет / файл не читается» — чтобы не
    /// ходить в провайдер на каждый символ).
    named: std::sync::RwLock<HashMap<String, Option<Arc<OwnedFontMetrics>>>>,
}

impl SystemFaceSet {
    /// Резолвит все generic-семейства через провайдер, парсит метрики
    /// выбранных face-ов и запоминает провайдер для ленивого резолва
    /// конкретных семейств.
    ///
    /// Берётся regular-начертание (weight 400, normal, stretch 100):
    /// [`TextMeasurer`] не получает weight/style, поэтому измеритель
    /// принципиально одно-начертательный (то же и для @font-face-семей).
    /// Битые/нечитаемые файлы тихо пропускаются — семейство просто останется
    /// нерезолвленным и упадёт на bundled Inter, как до BUG-128.
    pub fn from_provider(provider: Arc<dyn lumen_core::ext::FontProvider>) -> Self {
        use lumen_core::ext::{FontStyle as CssFontStyle, GENERIC_FAMILIES, NORMAL_STRETCH_PERCENT};
        let mut generics = Vec::new();
        for generic in GENERIC_FAMILIES {
            let Some(rec) = provider.pick_generic_face(
                generic,
                400,
                CssFontStyle::Normal,
                NORMAL_STRETCH_PERCENT,
            ) else {
                continue;
            };
            if let Some(metrics) = face_metrics(provider.as_ref(), &rec.path) {
                generics.push((generic, Arc::new(metrics)));
            }
        }
        Self {
            generics,
            provider: Some(provider),
            named: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Пустой набор — для тестов и путей без системного провайдера
    /// (детерминированный CPU-рендер, headless-драйвер).
    pub fn empty() -> Self {
        Self {
            generics: Vec::new(),
            provider: None,
            named: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Сколько generic-семейств удалось резолвить. 0 — на машине не нашлось
    /// ни одного кандидата (или провайдер пуст).
    pub fn resolved_generic_count(&self) -> usize {
        self.generics.len()
    }

    /// Метрики face-а для семейства; `None` — имя не резолвится системой.
    ///
    /// `family_lower` обязан быть уже приведён к нижнему регистру (вызывающий
    /// код всё равно строит эту строку для поиска по @font-face-семьям —
    /// незачем аллоцировать её дважды на каждый символ). Регистр для самого
    /// поиска не важен: и таблица generic-кандидатов, и системный индекс
    /// сравнивают имена ASCII-case-insensitive (CSS Fonts L4 §4.3).
    fn metrics(&self, family_lower: &str) -> Option<Arc<OwnedFontMetrics>> {
        if let Some((_, m)) = self.generics.iter().find(|(name, _)| *name == family_lower) {
            return Some(Arc::clone(m));
        }
        // Нерезолвленный generic в системный индекс не отдаём: шрифта с
        // family name «serif» не ставит ни одна ОС.
        if self.provider.is_none() || lumen_core::ext::is_generic_family(family_lower) {
            return None;
        }
        if let Ok(cache) = self.named.read()
            && let Some(hit) = cache.get(family_lower)
        {
            return hit.clone();
        }
        self.resolve_named(family_lower)
    }

    /// Медленный путь [`SystemFaceSet::metrics`]: спрашивает провайдер,
    /// читает и парсит файл, кладёт результат (в том числе отрицательный) в
    /// кэш.
    fn resolve_named(&self, family_lower: &str) -> Option<Arc<OwnedFontMetrics>> {
        use lumen_core::ext::{FontStyle as CssFontStyle, NORMAL_STRETCH_PERCENT};
        let provider = self.provider.as_ref()?;
        let resolved = provider
            .pick_face(family_lower, 400, CssFontStyle::Normal, NORMAL_STRETCH_PERCENT)
            .and_then(|rec| face_metrics(provider.as_ref(), &rec.path))
            .map(Arc::new);
        let Ok(mut cache) = self.named.write() else {
            return resolved;
        };
        let at_capacity = resolved.is_some()
            && cache.values().filter(|v| v.is_some()).count() >= MAX_CACHED_NAMED_FACES;
        let stored = if at_capacity { None } else { resolved };
        cache.insert(family_lower.to_string(), stored.clone());
        stored
    }

    /// Сколько конкретных семейств реально держит кэш (без отрицательных
    /// записей) — для теста предела [`MAX_CACHED_NAMED_FACES`].
    #[cfg(test)]
    fn cached_named_face_count(&self) -> usize {
        self.named
            .read()
            .map(|c| c.values().filter(|v| v.is_some()).count())
            .unwrap_or(0)
    }
}

/// Читает и парсит метрики face-а по пути из [`lumen_core::ext::FaceRecord`].
///
/// Сначала пробует память провайдера (@font-face-байты, виртуальные пути),
/// затем файловую систему. `None` — файл не читается или не парсится.
fn face_metrics(
    provider: &dyn lumen_core::ext::FontProvider,
    path: &std::path::Path,
) -> Option<OwnedFontMetrics> {
    let bytes = match provider.read_face_bytes(path) {
        Some(mem) => mem.to_vec(),
        None => std::fs::read(path).ok()?,
    };
    OwnedFontMetrics::from_bytes(&bytes).ok()
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
    /// Системные face-ы — те же, что выберет рендер (BUG-128). `None` — путь
    /// без системного провайдера: все не-@font-face семьи меряются bundled
    /// Inter-ом.
    system: Option<Arc<SystemFaceSet>>,
}

impl MultiFontMeasurer {
    /// Создаёт измеритель с bundled-шрифтом как fallback.
    pub fn new(fallback_font: &lumen_font::Font<'static>) -> Result<Self, FontError> {
        Ok(Self {
            fallback: FontMeasurer::new(fallback_font)?,
            faces: HashMap::new(),
            system: None,
        })
    }

    /// Подключает системные face-ы (generic-семейства + конкретные системные
    /// семейства по имени, BUG-128).
    ///
    /// Набор строится один раз на процесс и передаётся сюда `Arc`-ом при
    /// каждой пересборке измерителя — парсинг метрик не повторяется.
    /// Без вызова системные имена меряются bundled Inter-ом (поведение до
    /// BUG-128).
    pub fn set_system_faces(&mut self, system: Arc<SystemFaceSet>) {
        self.system = Some(system);
    }

    /// Метрики системного семейства, если оно резолвится набором.
    /// `family_lower` — имя в нижнем регистре (см. [`SystemFaceSet::metrics`]).
    fn system_metrics(&self, family_lower: &str) -> Option<Arc<OwnedFontMetrics>> {
        self.system.as_ref()?.metrics(family_lower)
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
            let lower = family.to_ascii_lowercase();
            if let Some(slots) = self.faces.get(&lower) {
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
            // BUG-128: системное имя (generic или конкретное семейство) —
            // мерим тем же face-ом, который выберет рендер, а не bundled
            // Inter-ом.
            if let Some(m) = self.system_metrics(&lower)
                && let Some(w) = m.try_char_width(ch, font_size_px)
            {
                return w;
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
            let lower = family.to_ascii_lowercase();
            if let Some(slots) = self.faces.get(&lower) {
                for slot in slots {
                    if !codepoint_in_ranges(cp, &slot.unicode_ranges) {
                        continue;
                    }
                    if let Some(w) = slot.metrics.try_char_width_varied(ch, font_size_px, axes) {
                        return w;
                    }
                }
            }
            // BUG-128: см. `char_width_with_families`.
            if let Some(m) = self.system_metrics(&lower)
                && let Some(w) = m.try_char_width_varied(ch, font_size_px, axes)
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

    fn x_height_px(&self, font_size_px: f32) -> f32 {
        self.fallback.x_height_px(font_size_px)
    }
}

#[cfg(test)]
mod multi_font_tests {
    use super::*;
    use lumen_layout::TextMeasurer;

    static INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");

    /// Путь к bundled JetBrains Mono — «системный» моноширинный шрифт для
    /// generic-тестов (метрики заведомо отличаются от пропорционального Inter).
    const MONO_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../../assets/fonts/JetBrainsMono-Regular.ttf");

    /// Имя «установленного» конкретного системного семейства — не generic и не
    /// кандидат ни одного generic-а, поэтому найти его можно только через
    /// именной резолв (BUG-128, п.1).
    const NAMED_FAMILY: &str = "Lumen Test Mono";

    /// Префикс имён для проверки предела кэша: провайдер «знает» их сколько
    /// угодно.
    const CAP_FAMILY_PREFIX: &str = "cap-family-";

    /// Провайдер, у которого «установлены» первый платформенный кандидат на
    /// `monospace`, конкретное семейство [`NAMED_FAMILY`] и сколь угодно много
    /// `cap-family-<N>` — все указывают на bundled JetBrains Mono.
    ///
    /// Считает обращения к индексу: тесты ленивого кэша требуют ровно одного
    /// резолва на имя, сколько бы символов им ни мерили.
    struct MonoOnlyProvider {
        /// Число вызовов [`FontProvider::lookup_family`] с момента создания.
        lookups: std::sync::atomic::AtomicUsize,
    }

    impl MonoOnlyProvider {
        fn new() -> Self {
            Self { lookups: std::sync::atomic::AtomicUsize::new(0) }
        }

        /// Сколько раз провайдер спрашивали об именах семейств.
        fn lookups(&self) -> usize {
            self.lookups.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl lumen_core::ext::FontProvider for MonoOnlyProvider {
        fn lookup_family(&self, family: &str) -> Vec<std::path::PathBuf> {
            self.lookups.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let first = lumen_core::ext::generic_family_candidates("monospace")[0];
            let installed = family.eq_ignore_ascii_case(first)
                || family.eq_ignore_ascii_case(NAMED_FAMILY)
                || family.to_ascii_lowercase().starts_with(CAP_FAMILY_PREFIX);
            if installed {
                vec![std::path::PathBuf::from(MONO_PATH)]
            } else {
                Vec::new()
            }
        }
        fn list_families(&self) -> Vec<String> {
            vec![
                lumen_core::ext::generic_family_candidates("monospace")[0].to_string(),
                NAMED_FAMILY.to_string(),
            ]
        }
    }

    #[test]
    fn named_system_family_measured_with_system_face_not_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        // Ни @font-face, ни generic — только конкретное системное имя.
        let families = vec![NAMED_FAMILY.to_string()];
        let inter_i = m.char_width_with_families('i', 16.0, &families);
        let inter_w = m.char_width_with_families('W', 16.0, &families);
        assert!(inter_i < inter_w, "Inter пропорционален: {inter_i} vs {inter_w}");

        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let mono_i = m.char_width_with_families('i', 16.0, &families);
        let mono_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (mono_i - mono_w).abs() < 0.01,
            "конкретное системное семейство должно мериться своим face-ом, {mono_i} vs {mono_w}"
        );
    }

    #[test]
    fn named_family_matched_case_insensitively() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        // CSS Fonts L4 §4.3: имена семейств сравниваются case-insensitive.
        let exact = m.char_width_with_families('W', 16.0, &[NAMED_FAMILY.to_string()]);
        let shouty = m.char_width_with_families('W', 16.0, &[NAMED_FAMILY.to_uppercase()]);
        assert!((exact - shouty).abs() < f32::EPSILON, "{exact} vs {shouty}");
    }

    #[test]
    fn named_family_resolved_once_and_reused() {
        let provider = Arc::new(MonoOnlyProvider::new());
        let set = SystemFaceSet::from_provider(provider.clone());
        let after_generics = provider.lookups();

        for _ in 0..32 {
            assert!(set.metrics(&NAMED_FAMILY.to_ascii_lowercase()).is_some());
        }
        assert_eq!(
            provider.lookups(),
            after_generics + 1,
            "резолв конкретного семейства обязан быть ленивым и однократным"
        );

        // Отрицательный ответ кэшируется так же: в системе такого нет.
        for _ in 0..32 {
            assert!(set.metrics("no such family").is_none());
        }
        assert_eq!(
            provider.lookups(),
            after_generics + 2,
            "промах тоже кэшируется, иначе индекс опрашивается на каждый символ"
        );
    }

    #[test]
    fn unknown_named_family_falls_back_to_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["No Such Family".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let after = m.char_width_with_families('W', 16.0, &families);
        assert!((after - before).abs() < f32::EPSILON, "{before} → {after}");
    }

    #[test]
    fn named_face_cache_is_bounded() {
        let set = SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new()));
        for i in 0..MAX_CACHED_NAMED_FACES {
            assert!(
                set.metrics(&format!("{CAP_FAMILY_PREFIX}{i}")).is_some(),
                "первые {MAX_CACHED_NAMED_FACES} семейств кэшируются"
            );
        }
        // Сверх предела метрики не удерживаются: страница со списком из тысячи
        // имён не должна держать тысячу cmap-ов до конца процесса.
        assert!(set.metrics(&format!("{CAP_FAMILY_PREFIX}overflow")).is_none());
        assert_eq!(set.cached_named_face_count(), MAX_CACHED_NAMED_FACES);
    }

    #[test]
    fn font_face_family_wins_over_same_named_system_family() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(
            MonoOnlyProvider::new(),
        ))));
        let families = vec![NAMED_FAMILY.to_string()];
        let system_w = m.char_width_with_families('W', 16.0, &families);
        // CSS Fonts L4 §5: @font-face затеняет одноимённое системное семейство.
        m.register_family(NAMED_FAMILY, INTER.to_vec());
        let web_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (web_w - m.char_width('W', 16.0)).abs() < f32::EPSILON && web_w != system_w,
            "@font-face должен победить системное имя: {system_w} → {web_w}"
        );
    }

    #[test]
    fn generic_monospace_measured_with_system_face_not_inter() {
        let font = inter_font();
        let generics = Arc::new(SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new())));
        assert_eq!(generics.resolved_generic_count(), 1, "резолвиться должен только monospace");

        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["monospace".to_string()];
        // До подключения набора generic мерится bundled Inter-ом —
        // пропорциональным, значит 'i' уже 'W'.
        let inter_i = m.char_width_with_families('i', 16.0, &families);
        let inter_w = m.char_width_with_families('W', 16.0, &families);
        assert!(inter_i < inter_w, "Inter пропорционален: {inter_i} vs {inter_w}");

        m.set_system_faces(generics);
        let mono_i = m.char_width_with_families('i', 16.0, &families);
        let mono_w = m.char_width_with_families('W', 16.0, &families);
        assert!(
            (mono_i - mono_w).abs() < 0.01,
            "system monospace face: advance должен совпадать, {mono_i} vs {mono_w}"
        );
    }

    #[test]
    fn unresolved_generic_falls_back_to_inter() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["serif".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        // MonoOnlyProvider не знает ни одного serif-кандидата → generic
        // остаётся нерезолвленным, измеритель обязан вести себя как раньше.
        m.set_system_faces(Arc::new(SystemFaceSet::from_provider(Arc::new(MonoOnlyProvider::new()))));
        assert!((m.char_width_with_families('W', 16.0, &families) - before).abs() < f32::EPSILON);
    }

    #[test]
    fn empty_generic_set_keeps_inter_metrics() {
        let font = inter_font();
        let mut m = MultiFontMeasurer::new(&font).unwrap();
        let families = vec!["monospace".to_string()];
        let before = m.char_width_with_families('W', 16.0, &families);
        m.set_system_faces(Arc::new(SystemFaceSet::empty()));
        assert!((m.char_width_with_families('W', 16.0, &families) - before).abs() < f32::EPSILON);
    }

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
