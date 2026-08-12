//! Интеграционный тест на bundled Inter-Regular.ttf.
//!
//! Проверяет, что наш парсер не падает на реальном (не-синтетическом)
//! шрифте: разбор всех таблиц, маппинг Latin/Cyrillic, advance widths,
//! outline для конкретных букв и растеризация в bitmap.

use std::path::PathBuf;

use lumen_font::{Font, Outline, Rasterizer};

fn font_bytes() -> Vec<u8> {
    // CARGO_MANIFEST_DIR = D:\...\crates\engine\font ;
    // шрифт лежит на 3 уровня выше в assets\fonts\.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts")
        .join("Inter-Regular.ttf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

#[test]
fn parses_offset_table_and_records() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter-Regular");
    // У современного TTF обычно 14+ таблиц.
    assert!(
        font.tables().len() >= 10,
        "expected ≥10 tables, got {}",
        font.tables().len()
    );
    // head / maxp / cmap / glyf / loca / hhea / hmtx обязаны быть.
    for tag in [b"head", b"maxp", b"cmap", b"hhea", b"hmtx", b"glyf", b"loca"] {
        assert!(
            font.table(tag).is_some(),
            "table {} missing",
            std::str::from_utf8(tag).unwrap()
        );
    }
}

#[test]
fn parses_head_maxp_hhea() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let head = font.head().unwrap();
    assert!(head.units_per_em > 0);
    // Inter использует units_per_em = 2048 в современных версиях,
    // но допускаем диапазон.
    assert!(
        (256..=4096).contains(&head.units_per_em),
        "unusual units_per_em: {}",
        head.units_per_em
    );

    let maxp = font.maxp().unwrap();
    // Inter покрывает множество скриптов — глифов должно быть много.
    assert!(maxp.num_glyphs > 500);

    let hhea = font.hhea().unwrap();
    assert!(hhea.ascent > 0);
    assert!(hhea.descent < 0);
    assert!(hhea.number_of_h_metrics > 0);
}

#[test]
fn cmap_maps_latin_letters() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();
    for ch in ['A', 'B', 'M', 'a', 'z'] {
        let gid = cmap
            .glyph_index(ch as u32)
            .unwrap_or_else(|| panic!("'{ch}' not mapped"));
        assert_ne!(gid, 0, "'{ch}' mapped to .notdef");
    }
}

#[test]
fn cmap_maps_cyrillic_letters() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();
    // А, Б, Я (заглавные) + а, я (строчные) + ж (часто чисто-кириллическая).
    for ch in ['А', 'Б', 'Я', 'а', 'я', 'ж'] {
        let gid = cmap
            .glyph_index(ch as u32)
            .unwrap_or_else(|| panic!("'{ch}' not mapped"));
        assert_ne!(gid, 0, "'{ch}' mapped to .notdef");
    }
}

#[test]
fn hmtx_advance_widths_positive_for_letters() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();
    let hmtx = font.hmtx().unwrap();
    for ch in ['A', 'M', 'i', 'А', 'Я'] {
        let gid = cmap.glyph_index(ch as u32).unwrap();
        let aw = hmtx.advance_width(gid).expect("advance for {ch}");
        assert!(aw > 0, "advance for '{ch}' is zero");
    }
}

#[test]
fn glyph_outline_for_uppercase_a_is_non_trivial() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();
    let gid = cmap.glyph_index('A' as u32).unwrap();
    let glyph = font.glyph(gid).unwrap().expect("A has outline");
    let Outline::Simple(contours) = &glyph.outline else {
        panic!("A unexpectedly composite");
    };
    // У буквы 'A' — два контура (внешний + внутренний треугольный wee-bit).
    assert!(!contours.is_empty());
    let total_points: usize = contours.iter().map(|c| c.points.len()).sum();
    assert!(total_points >= 6, "A should have ≥6 points");
}

#[test]
fn rasterize_uppercase_a_produces_visible_bitmap() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let head = font.head().unwrap();
    let cmap = font.cmap().unwrap();
    let gid = cmap.glyph_index('A' as u32).unwrap();
    let glyph = font.glyph(gid).unwrap().unwrap();
    let raster = Rasterizer::new(32.0, head.units_per_em);
    let bitmap = raster.rasterize(&glyph).expect("rasterize A");

    assert!((10..=60).contains(&bitmap.width));
    assert!((10..=60).contains(&bitmap.height));

    let visible_pixels = bitmap.pixels.iter().filter(|&&p| p > 16).count();
    let total = bitmap.pixels.len();
    // Буква занимает заметную долю своего bbox.
    assert!(
        visible_pixels * 5 > total,
        "A has too few visible pixels: {visible_pixels} / {total}"
    );
}

#[test]
fn rasterize_cyrillic_ya() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let head = font.head().unwrap();
    let cmap = font.cmap().unwrap();
    let gid = cmap.glyph_index('Я' as u32).unwrap();
    let glyph = font.glyph_resolved(gid).unwrap().unwrap();
    let raster = Rasterizer::new(32.0, head.units_per_em);
    let bitmap = raster.rasterize(&glyph).expect("rasterize Я");
    let visible = bitmap.pixels.iter().filter(|&&p| p > 16).count();
    assert!(visible > 0, "Я rasterized as empty");
}

#[test]
fn advance_width_varied_no_hvar_returns_base() {
    // Inter Regular has no HVAR table, so advance_width_varied must return the
    // base advance width regardless of the coords slice.
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let hmtx = font.hmtx().unwrap();
    let cmap = font.cmap().unwrap();
    let gid = cmap.glyph_index('A' as u32).unwrap();
    let base_aw = hmtx.advance_width(gid).expect("advance width for A");
    assert!(base_aw > 0, "base advance width must be positive");
    // Empty coords: shortcut, no HVAR access.
    assert_eq!(font.advance_width_varied(gid, &hmtx, &[]), base_aw);
    // Non-empty coords: HVAR table absent → fallback to base.
    assert_eq!(font.advance_width_varied(gid, &hmtx, &[0.5]), base_aw);
    assert_eq!(font.advance_width_varied(gid, &hmtx, &[-1.0, 0.0]), base_aw);
}

/// Кириллическая 'А' (U+0410) в Inter — composite glyph, ссылается на латинскую
/// 'A' (U+0041). До поддержки composite этот тест бы провалился (glyph_resolved
/// возвращал бы composite-форму, которую rasterize отказывается рисовать);
/// теперь — должен пройти, потому что glyph_resolved разворачивает в Simple.
#[test]
fn rasterize_cyrillic_a_via_composite_resolution() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let head = font.head().unwrap();
    let cmap = font.cmap().unwrap();
    let gid = cmap.glyph_index('А' as u32).unwrap();

    // Сырой glyph должен быть composite — проверим это, чтобы убедиться,
    // что тест действительно тестирует composite-путь.
    let raw = font.glyph(gid).unwrap().unwrap();
    assert!(
        matches!(raw.outline, Outline::Composite(_)),
        "expected Cyrillic 'А' to be composite in Inter (it reuses Latin 'A')"
    );

    let resolved = font.glyph_resolved(gid).unwrap().unwrap();
    assert!(
        matches!(resolved.outline, Outline::Simple(_)),
        "glyph_resolved should produce Simple outline"
    );

    let raster = Rasterizer::new(32.0, head.units_per_em);
    let bitmap = raster.rasterize(&resolved).expect("rasterize Cyrillic А");
    let visible = bitmap.pixels.iter().filter(|&&p| p > 16).count();
    assert!(visible > 50, "Cyrillic А rasterized as too few pixels: {visible}");
}

/// `glyph_resolved_with_coords` для Inter-Regular (без `gvar`) обязан вести
/// себя как `glyph_resolved`: coords игнорируются, deltas не применяются,
/// outline точка-в-точку совпадает с base.
#[test]
fn glyph_resolved_with_coords_ignored_on_non_variable_font() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();

    // Inter-Regular — статический шрифт, gvar отсутствует.
    assert!(
        font.gvar().is_err(),
        "Inter-Regular shouldn't have gvar; if upstream switched bundle to a \
         variable build, this test (and rasterizer cache key) needs revisit"
    );

    for ch in ['A', 'M', 'g', 'А', 'Я'] {
        let gid = cmap.glyph_index(ch as u32).unwrap();
        let base = font.glyph_resolved(gid).unwrap().unwrap();
        let with_coords = font
            .glyph_resolved_with_coords(gid, &[0.5, -0.25])
            .unwrap()
            .unwrap();
        assert_eq!(
            base.bbox, with_coords.bbox,
            "bbox should match for '{ch}' without gvar"
        );
        let Outline::Simple(base_c) = base.outline else {
            panic!("Inter '{ch}' must resolve to Simple");
        };
        let Outline::Simple(coord_c) = with_coords.outline else {
            panic!("with_coords '{ch}' must resolve to Simple");
        };
        assert_outlines_equal(&base_c, &coord_c, ch);
    }
}

/// Пустой `coords` short-circuit-ит на путь `glyph_resolved` — для любого
/// glyph-а (simple и composite) результат идентичен (включая когда `gvar`
/// у font-а есть, но caller хочет default-instance).
#[test]
fn glyph_resolved_with_coords_empty_matches_glyph_resolved() {
    let data = font_bytes();
    let font = Font::parse(&data).unwrap();
    let cmap = font.cmap().unwrap();

    // Latin 'A' — simple; кириллическая 'А' — composite (use Latin 'A').
    for ch in ['A', 'А'] {
        let gid = cmap.glyph_index(ch as u32).unwrap();
        let base = font.glyph_resolved(gid).unwrap().unwrap();
        let empty = font
            .glyph_resolved_with_coords(gid, &[])
            .unwrap()
            .unwrap();
        assert_eq!(base.bbox, empty.bbox, "bbox differs for '{ch}'");
        match (base.outline, empty.outline) {
            (Outline::Simple(a), Outline::Simple(b)) => assert_outlines_equal(&a, &b, ch),
            _ => panic!("glyph_resolved must return Simple for '{ch}'"),
        }
    }
}

fn assert_outlines_equal(
    a: &[lumen_font::Contour],
    b: &[lumen_font::Contour],
    label: char,
) {
    assert_eq!(
        a.len(),
        b.len(),
        "'{label}': contour count differs ({} vs {})",
        a.len(),
        b.len()
    );
    for (i, (ca, cb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(
            ca.points.len(),
            cb.points.len(),
            "'{label}' contour {i}: point count differs"
        );
        for (j, (pa, pb)) in ca.points.iter().zip(cb.points.iter()).enumerate() {
            assert_eq!(pa, pb, "'{label}' contour {i} point {j} differs");
        }
    }
}

/// Эталонная сборка composite-глифа общей f32-формулой — независимая от движка
/// копия того, что делал `glyph_resolved_inner` до среза 19 BUG-405.
///
/// Существует ровно чтобы гейт ниже сравнивал движок не с самим собой: после
/// правки identity-компонент внутри движка всегда уходит в целочисленный путь,
/// и «прогнать тот же вход через оба тела» на реальном шрифте больше нельзя.
fn resolve_reference(
    font: &Font,
    gid: u16,
    depth: u32,
) -> Option<lumen_font::glyf::Glyph> {
    use lumen_font::glyf::{Anchor, Contour, Outline, OutlinePoint};

    if depth > 8 {
        return None;
    }
    let glyph = font.glyph(gid).ok()??;
    let components = match &glyph.outline {
        Outline::Simple(_) => return Some(glyph),
        Outline::Composite(c) => c.clone(),
    };

    let nth = |contours: &[Contour], idx: usize| -> Option<(f32, f32)> {
        let mut counter = 0usize;
        for c in contours {
            if idx < counter + c.points.len() {
                let p = c.points[idx - counter];
                return Some((p.x as f32, p.y as f32));
            }
            counter += c.points.len();
        }
        None
    };

    let mut merged: Vec<Contour> = Vec::new();
    for comp in components {
        let Some(sub) = resolve_reference(font, comp.glyph_id, depth + 1) else {
            continue;
        };
        let Outline::Simple(sub_contours) = sub.outline else {
            continue;
        };
        let (dx, dy) = match comp.anchor {
            Anchor::Offset(dx, dy) => (dx, dy),
            Anchor::Points { parent: pi, child: ci } => {
                let parent_xy = nth(&merged, pi as usize);
                let child_xy = nth(&sub_contours, ci as usize).map(|(cx, cy)| {
                    (
                        comp.transform[0] * cx + comp.transform[2] * cy,
                        comp.transform[1] * cx + comp.transform[3] * cy,
                    )
                });
                match (parent_xy, child_xy) {
                    (Some((px, py)), Some((tx, ty))) => (px - tx, py - ty),
                    _ => (0.0, 0.0),
                }
            }
        };
        for contour in sub_contours {
            let points = contour
                .points
                .into_iter()
                .map(|p| {
                    let nx = comp.transform[0] * p.x as f32
                        + comp.transform[2] * p.y as f32
                        + dx;
                    let ny = comp.transform[1] * p.x as f32
                        + comp.transform[3] * p.y as f32
                        + dy;
                    OutlinePoint {
                        x: nx.round() as i16,
                        y: ny.round() as i16,
                        on_curve: p.on_curve,
                    }
                })
                .collect();
            merged.push(Contour { points });
        }
    }

    let bbox = lumen_font::glyf::bbox_from_contours(&merged).unwrap_or(glyph.bbox);
    Some(lumen_font::glyf::Glyph {
        bbox,
        outline: Outline::Simple(merged),
    })
}

/// Гейт среза 19 BUG-405, сквозное плечо: быстрый путь сборки компонента даёт
/// на реальном шрифте те же точки, что общая формула, для КАЖДОГО глифа,
/// достижимого через cmap (латиница, Latin-1 с диакритикой, кириллица).
///
/// Ловит и обратную ошибку — сужение предиката identity до «никогда»: если
/// быстрый путь перестанет срабатывать, тест останется зелёным, поэтому здесь
/// же утверждается, что composite-глифы в корпусе есть и их компоненты
/// действительно identity+целый offset (иначе правка меряла бы пустоту).
#[test]
fn composite_assembly_matches_general_formula_on_inter() {
    use lumen_font::glyf::{Anchor, Outline};

    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter-Regular");
    let cmap = font.cmap().expect("cmap");

    let mut checked = 0usize;
    let mut composites = 0usize;
    let mut fast_components = 0usize;
    let mut all_components = 0usize;

    for cp in (0x20u32..0x7F).chain(0xC0..0x180).chain(0x400..0x460) {
        let Some(gid) = cmap.glyph_index(cp) else {
            continue;
        };
        if gid == 0 {
            continue;
        }
        if let Ok(Some(raw)) = font.glyph(gid)
            && let Outline::Composite(comps) = &raw.outline
        {
            composites += 1;
            all_components += comps.len();
            fast_components += comps
                .iter()
                .filter(|c| {
                    c.transform == [1.0, 0.0, 0.0, 1.0]
                        && matches!(c.anchor, Anchor::Offset(dx, dy)
                            if dx.fract() == 0.0 && dy.fract() == 0.0)
                })
                .count();
        }

        let engine = font.glyph_resolved(gid).expect("resolve");
        let reference = resolve_reference(&font, gid, 0);
        match (engine, reference) {
            (Some(e), Some(r)) => {
                assert_eq!(e.bbox, r.bbox, "bbox gid {gid} (U+{cp:04X})");
                let (Outline::Simple(e), Outline::Simple(r)) = (e.outline, r.outline)
                else {
                    panic!("оба разбора обязаны быть простыми, gid {gid}");
                };
                assert_outlines_equal(
                    &e,
                    &r,
                    char::from_u32(cp).unwrap_or('?'),
                );
                checked += 1;
            }
            (None, None) => {}
            _ => panic!("движок и эталон разошлись по наличию глифа {gid}"),
        }
    }

    assert!(checked > 300, "корпус гейта неожиданно мал: {checked} глифов");
    assert!(
        composites > 100,
        "у Inter обязаны быть composite-глифы, найдено {composites}"
    );
    assert_eq!(
        fast_components, all_components,
        "правка среза 19 обслуживает identity+целый offset; в корпусе таких \
         {fast_components} из {all_components} — если доля упала, перепись цены \
         устарела"
    );
}

#[test]
fn reads_family_name_from_inter() {
    let data = font_bytes();
    let font = Font::parse(&data).expect("parse Inter-Regular");
    let name = font.name().expect("parse name table");
    // Inter записывает family как "Inter" (typographic) — оба поля должны
    // быть прочитаны.
    let family = name.best_family().expect("Inter must expose a family name");
    assert_eq!(
        family, "Inter",
        "expected Inter to identify itself as 'Inter', got {family:?}"
    );
}

