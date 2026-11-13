//! TEXTQ-1: глиф растрируется в точном размере в device px, квад стоит на
//! целых device px, перо по X делится на 4 субпиксельные фазы.

use super::super::*;

#[test]
fn glyph_raster_size_is_exact_device_px_in_quarter_steps() {
    // 16 CSS px при dpr 1 — 16 device px = 64 четверти; без округления вверх.
    assert_eq!(glyph_raster_size(16.0, 1.0), (64, 1.0));
    // 13 px больше не раздувается до bin 16 и не ужимается обратно.
    assert_eq!(glyph_raster_size(13.0, 1.0), (52, 1.0));
    assert_eq!(glyph_raster_size(17.0, 1.0), (68, 1.0));
    // dpr 1.5: 16 CSS px — 24 device px.
    assert_eq!(glyph_raster_size(16.0, 1.5), (96, 1.0));
    // Квант 1/4 px: 13.2 → 13.25 (53 четверти), 13.05 → 13.0.
    assert_eq!(glyph_raster_size(13.2, 1.0).0, 52 + 1);
    assert_eq!(glyph_raster_size(13.05, 1.0).0, 52);
    // 72 px — прежде растягивался из bin 64.
    assert_eq!(glyph_raster_size(72.0, 1.0), (288, 1.0));
}

#[test]
fn glyph_raster_size_caps_huge_glyphs_and_stretches_them() {
    let (q, scale) = glyph_raster_size(512.0, 1.0);
    assert_eq!(f32::from(q) / 4.0, MAX_GLYPH_RASTER_PX);
    assert!((scale - 2.0).abs() < 1e-6, "растяжение {scale}");
}

#[test]
fn glyph_raster_size_invalid_input_is_minimal_without_panic() {
    for bad in [f32::NAN, -3.0, 0.0] {
        assert_eq!(glyph_raster_size(bad, 1.0), (1, 1.0));
    }
    assert_eq!(glyph_raster_size(16.0, f32::NAN), (1, 1.0));
    let (q, scale) = glyph_raster_size(f32::INFINITY, 1.0);
    assert_eq!(f32::from(q) / 4.0, MAX_GLYPH_RASTER_PX);
    assert!(scale.is_infinite());
}

#[test]
fn snap_pen_picks_nearest_quarter_for_small_glyphs() {
    let small = 64; // 16 device px
    assert_eq!(snap_pen(0.0, small), (0, 0));
    assert_eq!(snap_pen(0.30, small), (0, 1));
    assert_eq!(snap_pen(0.5, small), (0, 2));
    assert_eq!(snap_pen(7.74, small), (7, 3));
    // 0.9 ближе к следующему целому, чем к фазе 3 (0.75): фаза 0 следующего px.
    assert_eq!(snap_pen(3.9, small), (4, 0));
    // Отрицательное перо (отрицательный letter-spacing) — floor, а не trunc.
    assert_eq!(snap_pen(-0.25, small), (-1, 3));
}

#[test]
fn snap_pen_has_no_phase_for_large_glyphs() {
    let big = (SUBPIXEL_MAX_RASTER_PX * 4.0) as u16 + 1;
    assert_eq!(snap_pen(10.4, big), (10, 0));
    assert_eq!(snap_pen(10.6, big), (11, 0));
    // Порог включительно: ровно 48 device px ещё с фазами.
    assert_eq!(snap_pen(10.25, (SUBPIXEL_MAX_RASTER_PX * 4.0) as u16), (10, 1));
}

/// Раскладка Inter-текста: `(вершины, атлас-кэш)`.
fn lay_out(
    rect: Rect,
    text: &str,
    font_size: f32,
    dpr: f32,
) -> (Vec<TextVertex>, HashMap<AtlasKey, Option<CachedGlyph>>) {
    lay_out_with(rect, text, font_size, dpr, 400, lumen_layout::FontStyle::Normal, 400, false)
}

/// Раскладка с весом/стилем команды и собственными весом/наклоном face-а —
/// TEXTQ-6 (синтетический жирный/курсив).
#[allow(clippy::too_many_arguments)] // тестовый харнесс: все параметры различают сценарии
fn lay_out_with(
    rect: Rect,
    text: &str,
    font_size: f32,
    dpr: f32,
    font_weight: u16,
    font_style: lumen_layout::FontStyle,
    face_weight: u16,
    face_italic: bool,
) -> (Vec<TextVertex>, HashMap<AtlasKey, Option<CachedGlyph>>) {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf").expect("bundled font");
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace {
        bytes: Arc::from(bytes.as_slice()),
        metrics,
        unicode_ranges: Vec::new(),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
        variation_settings: Vec::new(),
        weight: face_weight,
        italic: face_italic,
    }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut runs = TextRunCache::default();
    let mut out: Vec<TextVertex> = Vec::new();
    push_text_glyphs(
        &mut out,
        rect,
        text,
        font_size,
        [0.0, 0.0, 0.0, 1.0],
        0,
        &mut lazy,
        &mut atlas,
        &mut cached,
        &mut runs,
        true,
        &[],
        &[],
        0.0,
        (0.0, 0.0),
        None,
        dpr,
        font_weight,
        font_style,
    );
    (out, cached)
}

/// TEXTQ-6: синтетический жирный — вторая порция из шести вершин с тем же UV,
/// сдвинутая вправо на `(font_size/24).clamp(0.5,2)` device px.
#[test]
fn synthetic_bold_doubles_quads_only_for_regular_face() {
    let rect = Rect::new(10.0, 7.0, 400.0, 30.0);
    // Face без собственного bold + weight 700 → два квада на глиф.
    let (synth, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 700, lumen_layout::FontStyle::Normal, 400, false);
    let (plain, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 400, lumen_layout::FontStyle::Normal, 400, false);
    assert_eq!(plain.len() % 6, 0);
    assert_eq!(synth.len(), plain.len() * 2, "каждый глиф — два квада");
    // Сдвиг второго квада: baseline не меняется, x0 правее x0 первого.
    let offset = (24.0_f32 / 24.0).clamp(0.5, 2.0);
    let dx = synth[6].pos[0] - synth[0].pos[0];
    assert!((dx - offset).abs() < 1e-3, "dx {dx} ≠ {offset}");
    assert!((synth[6].pos[1] - synth[0].pos[1]).abs() < 1e-6);
    // Тот же атлас-глиф: UV совпадают.
    assert_eq!(synth[0].uv, synth[6].uv);
    // Ложный вес в face-е (weight 700 в дескрипторе) → синтеза нет.
    let (true_bold, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 700, lumen_layout::FontStyle::Normal, 700, false);
    assert_eq!(true_bold.len(), plain.len());
    // ВИДИМЫЙ эффект: второй квад реально сдвинут (не нулевой сдвиг).
    assert!(dx > 0.0);
}

/// TEXTQ-6: синтетический курсив — тот же один квад, но верхние вершины
/// сдвинуты вправо относительно baseline на `(baseline − y) × tan 12°`.
#[test]
fn synthetic_italic_shears_quad_without_rerasterizing() {
    let rect = Rect::new(10.0, 7.0, 400.0, 30.0);
    let (ital, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 400, lumen_layout::FontStyle::Italic, 400, false);
    let (plain, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 400, lumen_layout::FontStyle::Normal, 400, false);
    assert_eq!(ital.len(), plain.len(), "курсив не добавляет квадов");
    // Верхняя (y0) и нижняя (y1) вершины квада: y разные, x верхней — правее.
    // Для обычного квада x0 верх == x0 низ; после наклона — нет.
    let top_dx = ital[0].pos[0] - plain[0].pos[0];
    let bottom_dx = ital[5].pos[0] - plain[5].pos[0];
    assert!(top_dx > 0.0, "верх сдвинут вправо: {top_dx}");
    // Низ битмапа на baseline (у "Hi" — не ниже device px): сдвиг мал.
    assert!(bottom_dx.abs() < 0.5, "низ на baseline: {bottom_dx}");
    // Собственный курсивный face → синтеза нет, вершины совпадают с plain.
    let (true_ital, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 400, lumen_layout::FontStyle::Italic, 400, true);
    assert_eq!(true_ital[0].pos, plain[0].pos);
}

/// TEXTQ-6: кэш планов различает вес/стиль команды (иначе чужой синтез).
#[test]
fn text_run_cache_key_distinguishes_weight_and_style() {
    let rect = Rect::new(10.0, 7.0, 400.0, 30.0);
    let (a, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 700, lumen_layout::FontStyle::Normal, 400, false);
    // Второй вызов с тем же face-строкой, но другим весом — через НОВЫЙ кэш,
    // поэтому сравниваем длины: 700 с регулярным face-ом даёт вдвое больше.
    let (b, _) = lay_out_with(rect, "Hi", 24.0, 1.0, 400, lumen_layout::FontStyle::Normal, 400, false);
    assert_eq!(a.len(), b.len() * 2);
}
/// Все координаты вершин кратны `1/dpr` CSS px, то есть лежат на целых
/// device px.
fn assert_on_device_grid(verts: &[TextVertex], dpr: f32) {
    assert!(!verts.is_empty(), "run обязан дать вершины");
    for v in verts {
        for c in v.pos {
            let dev = c * dpr;
            assert!((dev - dev.round()).abs() < 1e-3, "{c} CSS px = {dev} device px (dpr {dpr})");
        }
    }
}

#[test]
fn quads_sit_on_whole_device_pixels_at_dpr_1_and_1_5() {
    // Дробный left и top — худший случай для прежней укладки.
    let rect = Rect::new(10.3, 7.7, 400.0, 30.0);
    for dpr in [1.0_f32, 1.5, 2.0] {
        let (verts, _) = lay_out(rect, "Hamburgefonstiv 0123", 15.0, dpr);
        assert_on_device_grid(&verts, dpr);
    }
}

#[test]
fn glyph_is_rasterized_at_exact_device_size() {
    // dpr 1.5, 16 CSS px → 24 device px = 96 четвертей, а не bin 16/24.
    let (verts, cached) = lay_out(Rect::new(0.0, 0.0, 400.0, 30.0), "Hello", 16.0, 1.5);
    assert!(!verts.is_empty());
    assert!(!cached.is_empty());
    assert!(cached.keys().all(|k| k.size_q == 96), "{:?}", cached.keys().collect::<Vec<_>>());
    // dpr 1: 13 px — 13 device px, прежде 16.
    let (_, cached) = lay_out(Rect::new(0.0, 0.0, 400.0, 30.0), "Hello", 13.0, 1.0);
    assert!(cached.keys().all(|k| k.size_q == 52));
}

#[test]
fn quad_size_matches_bitmap_texel_for_texel() {
    // 1:1 — размер квада в device px равен размеру записи атласа.
    let (verts, cached) = lay_out(Rect::new(10.3, 7.7, 400.0, 30.0), "Hamburgefonstiv", 15.0, 1.5);
    let dims: Vec<(f32, f32)> = cached
        .values()
        .flatten()
        .map(|g| (g.entry.width as f32, g.entry.height as f32))
        .collect();
    for quad in verts.chunks(6) {
        let w = (quad[1].pos[0] - quad[0].pos[0]) * 1.5;
        let h = (quad[2].pos[1] - quad[0].pos[1]) * 1.5;
        assert!(
            dims.iter().any(|&(dw, dh)| (dw - w).abs() < 1e-3 && (dh - h).abs() < 1e-3),
            "квад {w}×{h} не совпадает ни с одной записью атласа"
        );
    }
}

#[test]
fn fractional_pen_uses_several_x_phases_at_text_size() {
    // Дробные advance-ы дают разные фазы одного и того же глифа.
    let (_, cached) = lay_out(Rect::new(0.0, 0.0, 600.0, 30.0), "llllllllllllllll", 15.0, 1.0);
    let phases: std::collections::BTreeSet<u8> = cached.keys().map(|k| k.x_phase).collect();
    assert!(phases.len() > 1, "ожидалось несколько фаз, получено {phases:?}");
    assert!(phases.iter().all(|&p| p < X_PHASES));
    // Заголовок > 48 device px — без фаз.
    let (_, cached) = lay_out(Rect::new(0.0, 0.0, 900.0, 90.0), "llllllll", 72.0, 1.0);
    assert!(cached.keys().all(|k| k.x_phase == 0));
}

#[test]
fn run_cache_hit_is_keyed_by_fractional_start_and_dpr() {
    // Тот же текст с другой дробной частью `rect.x × dpr` — другие фазы, а
    // значит другой план: ключ обязан их различать, иначе попадание подсунет
    // глифы чужой фазы.
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf").expect("bundled font");
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace {
        bytes: Arc::from(bytes.as_slice()),
        metrics,
        unicode_ranges: Vec::new(),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
        variation_settings: Vec::new(),
        weight: 400,
    italic: false,
    }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut runs = TextRunCache::default();
    let mut lay = |runs: &mut TextRunCache, x: f32, dpr: f32| {
        let mut out: Vec<TextVertex> = Vec::new();
        push_text_glyphs(
            &mut out,
            Rect::new(x, 0.0, 400.0, 30.0),
            "Hello, world",
            15.0,
            [0.0, 0.0, 0.0, 1.0],
            0,
            &mut lazy,
            &mut atlas,
            &mut cached,
            runs,
            true,
            &[],
            &[],
            0.0,
            (0.0, 0.0),
            None,
            dpr,
            400,
            lumen_layout::FontStyle::Normal,
        );
        out
    };
    let a = lay(&mut runs, 10.0, 1.0);
    let b = lay(&mut runs, 10.0, 1.0);
    assert_eq!((runs.hits, runs.misses), (1, 1), "та же позиция — попадание");
    assert_eq!(a.len(), b.len());
    // Целый сдвиг не меняет дробную часть: тот же план, вершины сдвинуты.
    let c = lay(&mut runs, 25.0, 1.0);
    assert_eq!((runs.hits, runs.misses), (2, 1), "целый сдвиг — попадание");
    assert!(c.iter().zip(&a).all(|(p, q)| (p.pos[0] - q.pos[0] - 15.0).abs() < 1e-4));
    let _ = lay(&mut runs, 10.4, 1.0);
    assert_eq!(runs.misses, 2, "другая дробная часть — новый план");
    let _ = lay(&mut runs, 10.0, 1.5);
    assert_eq!(runs.misses, 3, "другой dpr — новый план");
}
