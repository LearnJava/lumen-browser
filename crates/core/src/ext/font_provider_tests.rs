use super::*;

fn face(family: &str, weight: u16, style: FontStyle) -> FaceRecord {
    FaceRecord {
        family: family.to_string(),
        weight,
        style,
        stretch: 100,
        path: PathBuf::from(format!("{family}-{weight}-{style:?}.ttf")),
        unicode_ranges: Vec::new(),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
        line_gap_override: None,
        variation_settings: Vec::new(),
    }
}

fn face_with_stretch(
    family: &str,
    weight: u16,
    style: FontStyle,
    stretch: u16,
) -> FaceRecord {
    FaceRecord {
        family: family.to_string(),
        weight,
        style,
        stretch,
        path: PathBuf::from(format!("{family}-{weight}-{style:?}-{stretch}.ttf")),
        unicode_ranges: Vec::new(),
        ascent_override: None,
        descent_override: None,
        size_adjust: None,
        line_gap_override: None,
        variation_settings: Vec::new(),
    }
}

#[test]
fn codepoint_in_face_ranges_empty_means_unrestricted() {
    assert!(codepoint_in_face_ranges(0x41, &[]));
    assert!(codepoint_in_face_ranges(0x1F600, &[]));
}

#[test]
fn codepoint_in_face_ranges_checks_membership() {
    let ranges = [(0x41, 0x44), (0x400, 0x4FF)];
    assert!(codepoint_in_face_ranges(0x42, &ranges)); // 'B', внутри первого
    assert!(codepoint_in_face_ranges(0x410, &ranges)); // 'А', внутри второго
    assert!(!codepoint_in_face_ranges(0x61, &ranges)); // 'a', вне обоих
}

#[test]
fn font_style_parse_keyword() {
    assert_eq!(FontStyle::parse_keyword("normal"), Some(FontStyle::Normal));
    assert_eq!(FontStyle::parse_keyword("ITALIC"), Some(FontStyle::Italic));
    assert_eq!(FontStyle::parse_keyword("Oblique"), Some(FontStyle::Oblique));
    assert_eq!(
        FontStyle::parse_keyword("oblique 20deg"),
        Some(FontStyle::Oblique)
    );
    assert_eq!(FontStyle::parse_keyword("bogus"), None);
    assert_eq!(FontStyle::parse_keyword(""), None);
}

#[test]
fn match_face_exact_weight_and_style() {
    let faces = vec![
        face("Inter", 400, FontStyle::Normal),
        face("Inter", 700, FontStyle::Normal),
        face("Inter", 400, FontStyle::Italic),
    ];
    let m = match_face(&faces, 700, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 700);
    assert_eq!(m.style, FontStyle::Normal);
}

#[test]
fn match_face_400_prefers_500_over_300() {
    // Spec: для desired=400 сначала пробуется 500, потом descending.
    let faces = vec![
        face("F", 300, FontStyle::Normal),
        face("F", 500, FontStyle::Normal),
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 500);
}

#[test]
fn match_face_500_prefers_400_over_600() {
    let faces = vec![
        face("F", 400, FontStyle::Normal),
        face("F", 600, FontStyle::Normal),
    ];
    let m = match_face(&faces, 500, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 400);
}

#[test]
fn match_face_300_descends_then_ascends() {
    // desired=300 — сначала меньше desired, потом больше.
    // Кандидаты: 100 (-200), 500 (+200) → должен победить 100.
    let faces = vec![
        face("F", 100, FontStyle::Normal),
        face("F", 500, FontStyle::Normal),
    ];
    let m = match_face(&faces, 300, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 100);
}

#[test]
fn match_face_700_ascends_then_descends() {
    // desired=700 — сначала больше desired, потом меньше.
    let faces = vec![
        face("F", 400, FontStyle::Normal),
        face("F", 900, FontStyle::Normal),
    ];
    let m = match_face(&faces, 700, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 900);
}

#[test]
fn match_face_style_filter_strict_over_weight() {
    // Если запросили italic и italic есть с любым весом — он побеждает
    // даже более точный по весу normal.
    let faces = vec![
        face("F", 700, FontStyle::Normal),  // exact weight, but normal
        face("F", 100, FontStyle::Italic),  // wrong weight, but italic
    ];
    let m = match_face(&faces, 700, FontStyle::Italic, 100).unwrap();
    assert_eq!(m.style, FontStyle::Italic);
    assert_eq!(m.weight, 100);
}

#[test]
fn match_face_italic_falls_back_to_oblique_then_normal() {
    let faces_with_oblique = vec![
        face("F", 400, FontStyle::Normal),
        face("F", 400, FontStyle::Oblique),
    ];
    let m = match_face(&faces_with_oblique, 400, FontStyle::Italic, 100).unwrap();
    assert_eq!(m.style, FontStyle::Oblique);

    let faces_normal_only = vec![face("F", 400, FontStyle::Normal)];
    let m = match_face(&faces_normal_only, 400, FontStyle::Italic, 100).unwrap();
    assert_eq!(m.style, FontStyle::Normal);
}

#[test]
fn match_face_normal_prefers_oblique_over_italic() {
    let faces = vec![
        face("F", 400, FontStyle::Italic),
        face("F", 400, FontStyle::Oblique),
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.style, FontStyle::Oblique);
}

#[test]
fn match_face_empty_returns_none() {
    let faces: Vec<FaceRecord> = Vec::new();
    assert!(match_face(&faces, 400, FontStyle::Normal, 100).is_none());
}

#[test]
fn match_face_full_css_weight_ladder_for_400() {
    // Order: 400, 500, 300, 200, 100, 600, 700, 800, 900.
    let weights = [100, 200, 300, 500, 600, 700, 800, 900];
    let mut faces: Vec<FaceRecord> =
        weights.iter().map(|&w| face("F", w, FontStyle::Normal)).collect();
    // 500 first
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 500);
    // remove 500, expect 300
    faces.retain(|f| f.weight != 500);
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 300);
    // remove 300/200/100, expect 600
    faces.retain(|f| f.weight > 500);
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.weight, 600);
}

#[test]
fn match_face_stretch_exact_match() {
    // Точное совпадение по stretch должно иметь приоритет.
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 75),   // condensed
        face_with_stretch("F", 400, FontStyle::Normal, 100),  // normal
        face_with_stretch("F", 400, FontStyle::Normal, 125),  // expanded
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 75).unwrap();
    assert_eq!(m.stretch, 75);
}

#[test]
fn match_face_stretch_falls_back_to_narrower() {
    // Если точное совпадение есть, но мы запросили больший stretch,
    // должны выбрать ближайший меньший (второй выбор).
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 75),  // condensed
        face_with_stretch("F", 400, FontStyle::Normal, 125), // expanded
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.stretch, 75); // ближе к 100 чем 125
}

#[test]
fn match_face_stretch_falls_back_to_wider() {
    // Если ближайший меньший не найден, выбираем больший.
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 100), // normal
        face_with_stretch("F", 400, FontStyle::Normal, 150), // extra-expanded
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 75).unwrap();
    assert_eq!(m.stretch, 100); // ближайший больший
}

#[test]
fn match_face_stretch_above_normal_prefers_wider_first() {
    // CSS Fonts L4 §5.2: при desired > 100% сначала перебираются значения
    // ВЫШЕ desired. 100 и 150 равноудалены от 125 — выиграть обязан 150.
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 100), // normal
        face_with_stretch("F", 400, FontStyle::Normal, 150), // extra-expanded
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 125).unwrap();
    assert_eq!(m.stretch, 150);
}

#[test]
fn match_face_stretch_above_normal_falls_back_to_narrower() {
    // desired > 100%, но шире ничего нет — берём ближайший меньший.
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 75),
        face_with_stretch("F", 400, FontStyle::Normal, 100),
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 125).unwrap();
    assert_eq!(m.stretch, 100);
}

#[test]
fn match_face_stretch_at_normal_prefers_narrower_first() {
    // desired == 100% попадает в ветку `<= 100`: 75 и 125 равноудалены,
    // выигрывает более узкий. Зеркало предыдущего теста — фиксирует, что
    // точка разворота именно на 100%, а не на 101%.
    let faces = vec![
        face_with_stretch("F", 400, FontStyle::Normal, 75),
        face_with_stretch("F", 400, FontStyle::Normal, 125),
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 100).unwrap();
    assert_eq!(m.stretch, 75);
}

#[test]
fn match_face_stretch_filter_strict_over_weight() {
    // Stretch имеет приоритет над weight.
    let faces = vec![
        face_with_stretch("F", 700, FontStyle::Normal, 100), // exact weight, wrong stretch
        face_with_stretch("F", 100, FontStyle::Normal, 75),  // wrong weight, exact stretch
    ];
    let m = match_face(&faces, 400, FontStyle::Normal, 75).unwrap();
    assert_eq!(m.stretch, 75);
    assert_eq!(m.weight, 100);
}
