//! TEXTQ-4: шейпинг (кернинг, контекстные формы, `font-feature-settings`) в
//! GPU-пути отрисовки текста — `push_text_glyphs` кладёт глифы из
//! `active_text_shaper()`, а не по одному из `cmap`.

use super::super::glyph_raster::shaping_features;
use super::super::*;

const FONT_SIZE: f32 = 32.0;

fn inter_bytes() -> Vec<u8> {
    std::fs::read("../../../assets/fonts/Inter-Regular.ttf").expect("bundled font")
}

/// Раскладка Inter-текста в начале координат при dpr 1:
/// `(конечное перо, число квадов)`. `runs` общий между вызовами, чтобы тест
/// мог проверить ключ кэша планов.
fn lay_out(
    runs: &mut TextRunCache,
    text: &str,
    features: &[([u8; 4], u32)],
    letter_spacing: f32,
) -> (f32, usize) {
    let bytes = inter_bytes();
    let faces = vec![LoadedFace {
        metrics: build_face_metrics(&bytes),
        bytes: Arc::from(bytes.as_slice()),
        unicode_ranges: Vec::new(),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
        variation_settings: Vec::new(),
    }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut out: Vec<TextVertex> = Vec::new();
    let end_x = push_text_glyphs(
        &mut out,
        Rect::new(0.0, 0.0, 400.0, 60.0),
        text,
        FONT_SIZE,
        [0.0, 0.0, 0.0, 1.0],
        0,
        &mut lazy,
        &mut atlas,
        &mut cached,
        runs,
        true,
        &[],
        features,
        0.0,
        (letter_spacing, 0.0),
        None,
        1.0,
    );
    (end_x, out.len() / 6)
}

/// Сумма advance-ов шейпера в px — то, что нарисует CPU-путь.
fn shaped_width(text: &str, features: &[([u8; 4], u32)]) -> f32 {
    let bytes = inter_bytes();
    let metrics = build_face_metrics(&bytes).expect("metrics");
    let shaped = lumen_font::active_text_shaper().shape(
        &bytes,
        text,
        lumen_core::ext::ShapeDirection::LeftToRight,
        None,
        features,
        &[],
    );
    let scale = FONT_SIZE / metrics.units_per_em as f32;
    shaped.iter().map(|g| g.x_advance as f32 * scale).sum()
}

/// Сумма advance-ов из `hmtx` по одному на символ — как GPU-путь до TEXTQ-4.
fn hmtx_width(text: &str) -> f32 {
    let bytes = inter_bytes();
    let metrics = build_face_metrics(&bytes).expect("metrics");
    let scale = FONT_SIZE / metrics.units_per_em as f32;
    text.chars()
        .map(|c| {
            let gid = metrics.cmap.glyph_index(c as u32).unwrap_or(0);
            f32::from(metrics.advances[gid as usize]) * scale
        })
        .sum()
}

#[test]
fn kerned_pairs_move_the_pen_like_the_shaper_not_like_hmtx() {
    let mut runs = TextRunCache::default();
    let (end_x, quads) = lay_out(&mut runs, "AVATAR", &[], 0.0);
    assert_eq!(quads, 6);
    let hmtx = hmtx_width("AVATAR");
    let shaped = shaped_width("AVATAR", &[]);
    assert!(
        (hmtx - shaped).abs() > 1.0,
        "в Inter у AV/VA/AT/TA есть кернинг: hmtx {hmtx}, шейпер {shaped}"
    );
    assert!(
        (end_x - shaped).abs() < 1e-3,
        "перо {end_x} обязано совпасть с шейпером {shaped} (как в CPU-пути), а не с hmtx {hmtx}"
    );
}

#[test]
fn cached_plan_replays_the_same_shaped_run() {
    let mut runs = TextRunCache::default();
    let first = lay_out(&mut runs, "AVATAR", &[], 0.0);
    let hits_before = runs.hits;
    let second = lay_out(&mut runs, "AVATAR", &[], 0.0);
    assert_eq!(first, second);
    assert!(runs.hits > hits_before, "повторный run обязан попасть в кэш плана");
}

#[test]
fn font_feature_settings_change_shaping_and_the_cache_key() {
    // В Inter `->` — контекстная стрелка (`calt`): одна глифа вместо двух.
    let mut runs = TextRunCache::default();
    let (arrow_x, arrow_quads) = lay_out(&mut runs, "->", &[], 0.0);
    let (plain_x, plain_quads) = lay_out(&mut runs, "->", &[(*b"calt", 0)], 0.0);
    assert_eq!(arrow_quads, 1, "стрелка — один глиф");
    assert_eq!(plain_quads, 2, "`calt` 0 возвращает два символа");
    assert!((arrow_x - shaped_width("->", &[])).abs() < 1e-3);
    assert!((plain_x - shaped_width("->", &[(*b"calt", 0)])).abs() < 1e-3);
    assert!((arrow_x - plain_x).abs() > 1.0, "{arrow_x} против {plain_x}");
    // Тот же run с теми же фичами берётся из кэша, а не подменяется чужим.
    assert_eq!(lay_out(&mut runs, "->", &[], 0.0).1, 1);
    assert_eq!(lay_out(&mut runs, "->", &[(*b"calt", 0)], 0.0).1, 2);
}

#[test]
fn letter_spacing_is_added_per_cluster_not_per_glyph() {
    // `->` — один глиф на кластер из двух символов: интервал кладётся раз.
    let mut runs = TextRunCache::default();
    let (end_x, quads) = lay_out(&mut runs, "->", &[], 3.0);
    assert_eq!(quads, 1);
    assert!((end_x - (shaped_width("->", &[]) + 3.0)).abs() < 1e-3, "перо {end_x}");
    // Обычный текст — по интервалу на каждый символ.
    let (end_x, _) = lay_out(&mut runs, "abc", &[], 3.0);
    assert!((end_x - (shaped_width("abc", &[]) + 9.0)).abs() < 1e-3, "перо {end_x}");
}

#[test]
fn letter_spacing_turns_optional_ligatures_off_but_author_can_override() {
    assert_eq!(shaping_features(&[], 0.0).as_ref(), &[]);
    let author = [(*b"liga", 1)];
    assert_eq!(shaping_features(&author, 0.0).as_ref(), &author);
    let spaced = shaping_features(&author, 1.0);
    assert_eq!(spaced.as_ref(), &[(*b"liga", 0), (*b"clig", 0), (*b"liga", 1)]);
}

/// TEXTQ-5: ширина, которую раскладка получает от измерителя
/// (`TextMeasurer::shaped_width`), — та же, на которую GPU-путь сдвигает перо.
fn measured_width(text: &str, features: &[([u8; 4], u32)], letter_spacing: f32) -> f32 {
    use lumen_layout::{ShapeParams, TextMeasurer};
    let bytes = inter_bytes();
    let font = lumen_font::Font::parse(&bytes).expect("font");
    let params = ShapeParams {
        font_size_px: FONT_SIZE,
        letter_spacing,
        families: &[],
        features,
        axes: &[],
    };
    let single = crate::FontMeasurer::new(&font).expect("measurer").shaped_width(text, &params);
    // Многошрифтовый измеритель на пустом каскаде — bundled Inter, та же ширина.
    let owned: &'static [u8] = Box::leak(bytes.clone().into_boxed_slice());
    let static_font = lumen_font::Font::parse(owned).expect("font");
    let multi = crate::MultiFontMeasurer::new(&static_font).expect("measurer").shaped_width(text, &params);
    assert_eq!(single, multi, "FontMeasurer и MultiFontMeasurer расходятся на {text:?}");
    single.expect("шейпер принял Inter")
}

#[test]
fn layout_measurer_width_equals_gpu_pen_for_avatar() {
    let mut runs = TextRunCache::default();
    let (end_x, _) = lay_out(&mut runs, "AVATAR", &[], 0.0);
    let measured = measured_width("AVATAR", &[], 0.0);
    assert!((measured - end_x).abs() < 0.01, "измеритель {measured}, GPU-перо {end_x}");
    // Кернинг реально учтён: посимвольная сумма из hmtx заметно шире.
    assert!((hmtx_width("AVATAR") - measured).abs() > 1.0);
}

#[test]
fn layout_measurer_width_equals_gpu_pen_with_features_and_spacing() {
    for (text, features, ls) in [
        ("->", vec![], 0.0),
        ("->", vec![(*b"calt", 0)], 0.0),
        ("->", vec![], 3.0),
        ("abc", vec![], 3.0),
        ("fi ffi", vec![(*b"liga", 1)], 2.5),
        ("Hello, World", vec![], 0.0),
    ] {
        let mut runs = TextRunCache::default();
        let (end_x, _) = lay_out(&mut runs, text, &features, ls);
        let measured = measured_width(text, &features, ls);
        assert!(
            (measured - end_x).abs() < 0.01,
            "{text:?} {features:?} ls={ls}: измеритель {measured}, GPU-перо {end_x}"
        );
    }
}
