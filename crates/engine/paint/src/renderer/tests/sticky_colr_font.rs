use super::super::*;

// ── position:sticky offset/bound tests (BUG-336) ────────────────────────

#[test]
fn sticky_bound_defaults_to_full_viewport_with_no_clip_or_transform() {
    let bound = sticky_bound(&[], &[], 800.0, 600.0);
    assert_eq!(bound, Rect::new(0.0, 0.0, 800.0, 600.0));
}

#[test]
fn sticky_offset_dy_unclamped_matches_plain_page_scroll() {
    // No insets fire yet — behaves exactly like non-sticky content: dy = -scroll_y.
    let flow_rect = Rect::new(0.0, 200.0, 300.0, 50.0);
    let bound = Rect::new(0.0, 0.0, 800.0, 600.0);
    let dy = sticky_offset_dy(&flow_rect, Some(0.0), None, 100.0, bound);
    assert!((dy - (-100.0)).abs() < 0.01, "expected -100 (unclamped page scroll), got {dy}");
}

#[test]
fn sticky_offset_dy_sticks_to_bound_top() {
    // Scrolled past the top inset — dy pins screen_y at bound.y + top.
    let flow_rect = Rect::new(0.0, 200.0, 300.0, 50.0);
    let bound = Rect::new(0.0, 0.0, 800.0, 600.0);
    let dy = sticky_offset_dy(&flow_rect, Some(10.0), None, 250.0, bound);
    let screen_y = flow_rect.y + dy;
    assert!((screen_y - 10.0).abs() < 0.01, "expected pinned screen_y=10, got {screen_y}");
}

#[test]
fn sticky_bound_narrows_to_innermost_clip_rect() {
    // A sticky element nested inside an overflow:auto panel (its own
    // PushScrollLayer clip, already in screen space) must clamp against
    // that panel's scrollport, not the full viewport.
    let clip_stack = [Rect::new(50.0, 100.0, 300.0, 200.0)];
    let bound = sticky_bound(&clip_stack, &[], 800.0, 600.0);
    assert_eq!(bound, Rect::new(50.0, 100.0, 300.0, 200.0));
}

#[test]
fn sticky_bound_maps_screen_clip_back_through_ambient_transform() {
    // BUG-336: the panel's clip (screen space) sits at y=[100,300) once an
    // ambient translate(0, 80) is active (e.g. the panel's own scroll
    // container nested under a further shell-shift/CSS transform). The
    // bound handed to sticky_offset_dy must be in the SAME pre-transform
    // page-space as flow_rect — i.e. shifted back by -80.
    let clip_stack = [Rect::new(50.0, 100.0, 300.0, 200.0)];
    let transform_stack = [Mat4::translation_2d(0.0, 80.0)];
    let bound = sticky_bound(&clip_stack, &transform_stack, 800.0, 600.0);
    assert!((bound.y - 20.0).abs() < 0.01, "expected bound.y=20 (100 - 80), got {}", bound.y);
    assert!((bound.x - 50.0).abs() < 0.01, "x untouched by a pure y-translate, got {}", bound.x);
}

#[test]
fn sticky_nested_in_scroll_container_pins_within_local_scrollport() {
    // The BUG-336 regression scenario: `.net-table th { position:sticky;
    // top:0 }` inside a `.dt-panel { overflow-y:auto }` whose own
    // PushScrollLayer has already scrolled 120px (folded into the ambient
    // ty via transform_stack, exactly like the renderer's own accumulation
    // for PushScrollLayer). The page itself hasn't scrolled (scroll_y=0
    // below) — before the fix, `sdy` stayed `-scroll_y == 0` regardless of
    // the panel's own scroll, so the header just rode away with the
    // panel's transform like ordinary content instead of pinning.
    let clip_stack = [Rect::new(0.0, 40.0, 400.0, 240.0)]; // panel's screen-space scrollport
    let transform_stack = [Mat4::translation_2d(0.0, -120.0)]; // panel's own scroll(-y) translate
    let bound = sticky_bound(&clip_stack, &transform_stack, 800.0, 600.0);

    // Header's flow (page-space, pre-scroll) position: early in the table,
    // well above where the panel has scrolled to — its *unclamped* on-screen
    // position would be flow.y + ty = 150 - 120 = 30, above the panel's
    // visible top (40), i.e. scrolled out of view without the sticky clamp.
    let flow_rect = Rect::new(0.0, 150.0, 400.0, 24.0);
    let dy = sticky_offset_dy(&flow_rect, Some(0.0), None, 0.0, bound);

    // Final on-screen position = (flow.y + dy) transformed by the same
    // ambient ty the renderer applies afterward (transform_stack.last()) —
    // must land exactly at the panel's own visible top (40), not at the
    // unclamped 30, and not at flow.y itself (150, ignoring scroll).
    let ty = transform_stack[0].transform_point_2d(0.0, 0.0).1;
    let screen_y = flow_rect.y + dy + ty;
    assert!(
        (screen_y - 40.0).abs() < 0.01,
        "sticky header must pin at the panel's own scrollport top (40), got {screen_y}"
    );
}

// ─── font-palette: COLR/CPAL palette resolution (CSS Fonts L4 §11.3) ───

use lumen_font::PaletteColor;
use lumen_layout::font_palette::PaletteColorOverride;

/// Builds a `ColorTables` with the given palettes (each a list of RGBA
/// tuples, all the same length) and optional `paletteType` flags. The
/// `COLR` half stays empty — the palette resolver never looks at it.
fn tables(palettes: &[&[(u8, u8, u8, u8)]], types: &[u32]) -> ColorTables {
    let entries = palettes.first().map_or(0, |p| p.len()) as u16;
    let mut color_records = Vec::new();
    let mut palette_record_indices = Vec::new();
    for p in palettes {
        palette_record_indices.push(color_records.len() as u16);
        color_records.extend(
            p.iter().map(|&(r, g, b, a)| PaletteColor { r, g, b, a }),
        );
    }
    ColorTables {
        colr: Colr { version: 0, base_glyphs: Vec::new(), layers: Vec::new() },
        cpal: Cpal {
            version: if types.is_empty() { 0 } else { 1 },
            num_palette_entries: entries,
            color_records,
            palette_record_indices,
            palette_types: types.to_vec(),
        },
    }
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn resolve_palette_normal_takes_palette_zero() {
    let t = tables(&[&[(255, 0, 0, 255)], &[(0, 255, 0, 255)]], &[]);
    let p = resolve_palette(&t, None).unwrap();
    assert!(close(p[0], [1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn resolve_palette_light_and_dark_pick_the_flagged_palette() {
    let t = tables(
        &[&[(1, 1, 1, 255)], &[(0, 0, 0, 255)], &[(255, 255, 255, 255)]],
        &[0, Cpal::USABLE_WITH_DARK_BACKGROUND, Cpal::USABLE_WITH_LIGHT_BACKGROUND],
    );
    let dark = resolve_palette(&t, Some(&FontPaletteSelection::Dark)).unwrap();
    assert!(close(dark[0], [0.0, 0.0, 0.0, 1.0]));
    let light = resolve_palette(&t, Some(&FontPaletteSelection::Light)).unwrap();
    assert!(close(light[0], [1.0, 1.0, 1.0, 1.0]));
}

#[test]
fn resolve_palette_light_without_flags_behaves_as_normal() {
    // CPAL v0 has no paletteType array — `light`/`dark` must fall back to
    // palette 0 rather than disabling the color path.
    let t = tables(&[&[(255, 0, 0, 255)], &[(0, 255, 0, 255)]], &[]);
    let p = resolve_palette(&t, Some(&FontPaletteSelection::Light)).unwrap();
    assert!(close(p[0], [1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn resolve_palette_custom_starts_from_base_palette() {
    let t = tables(&[&[(255, 0, 0, 255)], &[(0, 0, 255, 255)]], &[]);
    let sel = FontPaletteSelection::Custom { base_palette: 1, overrides: Vec::new() };
    let p = resolve_palette(&t, Some(&sel)).unwrap();
    assert!(close(p[0], [0.0, 0.0, 1.0, 1.0]));
}

#[test]
fn resolve_palette_custom_applies_override_colors() {
    let t = tables(&[&[(255, 0, 0, 255), (0, 255, 0, 255)]], &[]);
    let sel = FontPaletteSelection::Custom {
        base_palette: 0,
        overrides: vec![PaletteColorOverride {
            index: 1,
            color: Color { r: 0, g: 0, b: 255, a: 128 },
        }],
    };
    let p = resolve_palette(&t, Some(&sel)).unwrap();
    // Slot 0 untouched, slot 1 replaced.
    assert!(close(p[0], [1.0, 0.0, 0.0, 1.0]));
    assert!(close(p[1], [0.0, 0.0, 1.0, 128.0 / 255.0]));
}

#[test]
fn resolve_palette_override_past_palette_end_is_ignored() {
    let t = tables(&[&[(255, 0, 0, 255)]], &[]);
    let sel = FontPaletteSelection::Custom {
        base_palette: 0,
        overrides: vec![PaletteColorOverride {
            index: 9,
            color: Color { r: 0, g: 0, b: 255, a: 255 },
        }],
    };
    let p = resolve_palette(&t, Some(&sel)).unwrap();
    assert_eq!(p.len(), 1);
    assert!(close(p[0], [1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn resolve_palette_unknown_base_palette_falls_back_to_zero() {
    let t = tables(&[&[(255, 0, 0, 255)]], &[]);
    let sel = FontPaletteSelection::Custom { base_palette: 7, overrides: Vec::new() };
    let p = resolve_palette(&t, Some(&sel)).unwrap();
    assert!(close(p[0], [1.0, 0.0, 0.0, 1.0]));
}

#[test]
fn resolve_palette_without_any_palette_is_none() {
    let t = tables(&[], &[]);
    assert!(resolve_palette(&t, None).is_none());
}

#[test]
fn layer_color_foreground_index_uses_text_color() {
    let palette = [[1.0, 0.0, 0.0, 1.0]];
    let text = [0.0, 0.5, 0.25, 1.0];
    let c = layer_color(Some(&palette), lumen_font::PALETTE_INDEX_FOREGROUND, text);
    assert!(close(c, text));
}

#[test]
fn layer_color_scales_palette_alpha_by_text_alpha() {
    // A half-transparent text color must dim the color glyph the same way
    // it dims a monochrome one — otherwise the emoji stays fully opaque.
    let palette = [[1.0, 0.0, 0.0, 0.5]];
    let c = layer_color(Some(&palette), 0, [0.0, 0.0, 0.0, 0.5]);
    assert!(close(c, [1.0, 0.0, 0.0, 0.25]));
}

#[test]
fn layer_color_out_of_range_index_uses_text_color() {
    let palette = [[1.0, 0.0, 0.0, 1.0]];
    let text = [0.0, 1.0, 0.0, 1.0];
    assert!(close(layer_color(Some(&palette), 5, text), text));
    assert!(close(layer_color(None, 0, text), text));
}

// ─── COLR color glyph emission (end-to-end over a synthetic font) ─────

/// Minimal TTF with two color layers, built in memory so the whole
/// `build_face_metrics` → `push_text_glyphs` path can be exercised
/// without a GPU and without a real color font on disk:
/// glyph 1 (`A`) is a COLR base glyph whose layers are glyph 2 (palette
/// entry 0 = opaque red) and glyph 3 (`0xFFFF` = text color). All three
/// outlines are the same square, so every layer produces a real bitmap.
fn build_color_font() -> Vec<u8> {
    fn table_record(out: &mut Vec<u8>, tag: &[u8; 4], offset: u32, length: u32) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum — не валидируется
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
    }

    let mut head = Vec::new();
    head.extend_from_slice(&0x00010000u32.to_be_bytes()); // version
    head.extend_from_slice(&0u32.to_be_bytes()); // fontRevision
    head.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
    head.extend_from_slice(&0x5F0F3CF5u32.to_be_bytes()); // magic
    head.extend_from_slice(&0u16.to_be_bytes()); // flags
    head.extend_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head.extend_from_slice(&[0u8; 16]); // created + modified
    head.extend_from_slice(&0i16.to_be_bytes()); // xMin
    head.extend_from_slice(&0i16.to_be_bytes()); // yMin
    head.extend_from_slice(&500i16.to_be_bytes()); // xMax
    head.extend_from_slice(&500i16.to_be_bytes()); // yMax
    head.extend_from_slice(&[0u8; 6]); // macStyle + lowestRecPPEM + dirHint
    head.extend_from_slice(&0i16.to_be_bytes()); // indexToLocFormat = short
    head.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat

    const NUM_GLYPHS: u16 = 4;
    let mut hhea = Vec::new();
    hhea.extend_from_slice(&0x00010000u32.to_be_bytes()); // version
    hhea.extend_from_slice(&800i16.to_be_bytes()); // ascender
    hhea.extend_from_slice(&(-200i16).to_be_bytes()); // descender
    hhea.extend_from_slice(&0i16.to_be_bytes()); // lineGap
    hhea.extend_from_slice(&600u16.to_be_bytes()); // advanceWidthMax
    hhea.extend_from_slice(&[0u8; 22]); // до numberOfHMetrics
    hhea.extend_from_slice(&NUM_GLYPHS.to_be_bytes());

    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x00010000u32.to_be_bytes());
    maxp.extend_from_slice(&NUM_GLYPHS.to_be_bytes());
    maxp.extend_from_slice(&[0u8; 26]);

    // hmtx: одна longHorMetric на глиф, advance 600.
    let mut hmtx = Vec::new();
    for _ in 0..NUM_GLYPHS {
        hmtx.extend_from_slice(&600u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // lsb
    }

    // cmap format 12: 'A' → glyph 1.
    let a = u32::from('A');
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes()); // format
    sub.extend_from_slice(&0u16.to_be_bytes()); // reserved
    sub.extend_from_slice(&28u32.to_be_bytes()); // length = 16 + 1 group
    sub.extend_from_slice(&0u32.to_be_bytes()); // language
    sub.extend_from_slice(&1u32.to_be_bytes()); // numGroups
    sub.extend_from_slice(&a.to_be_bytes()); // startCharCode
    sub.extend_from_slice(&a.to_be_bytes()); // endCharCode
    sub.extend_from_slice(&1u32.to_be_bytes()); // startGlyphID
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes()); // version
    cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
    cmap.extend_from_slice(&3u16.to_be_bytes()); // platformID = Windows
    cmap.extend_from_slice(&10u16.to_be_bytes()); // encodingID = Unicode full
    cmap.extend_from_slice(&12u32.to_be_bytes()); // subtable offset
    cmap.extend_from_slice(&sub);

    // glyf: глиф 0 пустой, глифы 1–3 — один и тот же квадрат
    // 0,0 → 500,500 (4 точки, все on-curve, long-form дельты).
    let mut square = Vec::new();
    square.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
    square.extend_from_slice(&0i16.to_be_bytes()); // xMin
    square.extend_from_slice(&0i16.to_be_bytes()); // yMin
    square.extend_from_slice(&500i16.to_be_bytes()); // xMax
    square.extend_from_slice(&500i16.to_be_bytes()); // yMax
    square.extend_from_slice(&3u16.to_be_bytes()); // endPtsOfContours[0]
    square.extend_from_slice(&0u16.to_be_bytes()); // instructionLength
    square.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // flags: ON_CURVE only
    for dx in [0i16, 500, 0, -500] {
        square.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 500, 0] {
        square.extend_from_slice(&dy.to_be_bytes());
    }
    let sq_len = square.len();
    let mut glyf = Vec::new();
    for _ in 0..3 {
        glyf.extend_from_slice(&square);
    }
    // loca short: значения в словах (× 2 = байтовый offset).
    let mut loca = Vec::new();
    for i in 0..=NUM_GLYPHS {
        // Глиф 0 пустой: loca[0] == loca[1] == 0.
        let words = if i == 0 { 0 } else { (i as usize - 1) * sq_len / 2 };
        loca.extend_from_slice(&(words as u16).to_be_bytes());
    }

    // COLR v0: база = глиф 1, слои = глифы 2 (палитра 0) и 3 (текст).
    let mut colr = Vec::new();
    colr.extend_from_slice(&0u16.to_be_bytes()); // version
    colr.extend_from_slice(&1u16.to_be_bytes()); // numBaseGlyphRecords
    colr.extend_from_slice(&14u32.to_be_bytes()); // baseGlyphRecordsOffset
    colr.extend_from_slice(&20u32.to_be_bytes()); // layerRecordsOffset
    colr.extend_from_slice(&2u16.to_be_bytes()); // numLayerRecords
    colr.extend_from_slice(&1u16.to_be_bytes()); // baseGlyph.glyphID
    colr.extend_from_slice(&0u16.to_be_bytes()); // firstLayerIndex
    colr.extend_from_slice(&2u16.to_be_bytes()); // numLayers
    colr.extend_from_slice(&2u16.to_be_bytes()); // layer 0 glyphID
    colr.extend_from_slice(&0u16.to_be_bytes()); // layer 0 paletteIndex
    colr.extend_from_slice(&3u16.to_be_bytes()); // layer 1 glyphID
    colr.extend_from_slice(&0xFFFFu16.to_be_bytes()); // layer 1 = foreground

    // CPAL v0: одна палитра, одна запись — непрозрачный красный.
    let mut cpal = Vec::new();
    cpal.extend_from_slice(&0u16.to_be_bytes()); // version
    cpal.extend_from_slice(&1u16.to_be_bytes()); // numPaletteEntries
    cpal.extend_from_slice(&1u16.to_be_bytes()); // numPalettes
    cpal.extend_from_slice(&1u16.to_be_bytes()); // numColorRecords
    cpal.extend_from_slice(&14u32.to_be_bytes()); // offsetFirstColorRecord
    cpal.extend_from_slice(&0u16.to_be_bytes()); // colorRecordIndices[0]
    cpal.extend_from_slice(&[0, 0, 255, 255]); // BGRA → красный

    // Каталог таблиц по спеке отсортирован по тегу.
    let tables: [(&[u8; 4], Vec<u8>); 9] = [
        (b"COLR", colr),
        (b"CPAL", cpal),
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x00010000u32.to_be_bytes()); // sfntVersion
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0u8; 6]); // searchRange/entrySelector/rangeShift
    let mut offset = 12u32 + tables.len() as u32 * 16;
    let mut records = Vec::new();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        table_record(&mut records, tag, offset, data.len() as u32);
        offset += data.len() as u32;
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&records);
    out.extend_from_slice(&body);
    out
}

#[test]
fn synthetic_color_font_is_detected_as_color() {
    let bytes = build_color_font();
    let m = build_face_metrics(&bytes).expect("face metrics");
    let color = m.color.as_ref().expect("COLR+CPAL must enable the color path");
    assert_eq!(color.colr.layers_for(1).map(<[_]>::len), Some(2));
    assert_eq!(color.cpal.num_palettes(), 1);
    assert_eq!(m.cmap.glyph_index(u32::from('A')), Some(1));
}

#[test]
fn color_glyph_emits_one_quad_per_layer_with_palette_colors() {
    let bytes = build_color_font();
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace { bytes: Arc::from(bytes.as_slice()), metrics }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut out: Vec<TextVertex> = Vec::new();
    let mut runs = TextRunCache::default();
    let text_color = [0.0, 0.0, 1.0, 1.0];

    let end_x = push_text_glyphs(
        &mut out,
        Rect::new(0.0, 0.0, 100.0, 40.0),
        "A",
        32.0,
        text_color,
        0,
        &mut lazy,
        &mut atlas,
        &mut cached,
        &mut runs,
        true,
        &[],
        0.0,
        None,
    );

    // Два слоя → два quad-а → 12 вершин; монохромный фолбэк дал бы 6.
    assert_eq!(out.len(), 12, "expected one quad per COLR layer");
    // Слой 0 → запись палитры 0 (красный), слой 1 → 0xFFFF (текстовый).
    for v in &out[..6] {
        assert!(close(v.color, [1.0, 0.0, 0.0, 1.0]), "layer 0 color {:?}", v.color);
    }
    for v in &out[6..] {
        assert!(close(v.color, text_color), "layer 1 color {:?}", v.color);
    }
    // Advance берётся у базового глифа (600/1000 em при 32px = 19.2px),
    // а не суммой advance-ов слоёв.
    assert!((end_x - 19.2).abs() < 0.01, "pen advanced to {end_x}");
}

/// BUG-435: отказ атласа по МЕСТУ не должен попадать в `cached_glyphs` —
/// иначе буква не нарисуется уже никогда, даже после сброса атласа.
#[test]
fn atlas_exhaustion_is_not_memoized_and_heals_after_reset() {
    let bytes = build_color_font();
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace { bytes: Arc::from(bytes.as_slice()), metrics }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(64);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();

    // Забиваем атлас чужой записью так, чтобы места больше не осталось.
    let filler = Bitmap {
        width: 60,
        height: 60,
        pixels: vec![255; 60 * 60],
        left: 0.0,
        top: 0.0,
    };
    assert!(atlas.insert(AtlasKey::new(u16::MAX, 0, 1, 0), &filler).is_some());

    let key = atlas_key(0, 1, 32, 0);
    assert!(
        ensure_glyph(&mut cached, &mut atlas, &mut lazy, 0, 1, 32, &[]).is_none(),
        "в переполненный атлас глиф не лёг"
    );
    assert!(atlas.exhausted(), "атлас пометил себя исчерпанным");
    assert!(
        !cached.contains_key(&key),
        "отказ по месту мемоизировать нельзя — он временный"
    );

    // Сброс атласа (в рантайме — на старте следующего кадра).
    atlas.reset();
    let g = ensure_glyph(&mut cached, &mut atlas, &mut lazy, 0, 1, 32, &[]);
    assert!(g.is_some(), "после сброса тот же глиф растеризуется и ложится");
    assert!(cached.get(&key).copied().flatten().is_some(), "и попадает в мемоизацию");
}

#[test]
fn custom_palette_override_reaches_the_emitted_vertices() {
    let bytes = build_color_font();
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace { bytes: Arc::from(bytes.as_slice()), metrics }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut out: Vec<TextVertex> = Vec::new();
    let mut runs = TextRunCache::default();
    let selection = FontPaletteSelection::Custom {
        base_palette: 0,
        overrides: vec![PaletteColorOverride {
            index: 0,
            color: Color { r: 0, g: 255, b: 0, a: 255 },
        }],
    };

    push_text_glyphs(
        &mut out,
        Rect::new(0.0, 0.0, 100.0, 40.0),
        "A",
        32.0,
        [0.0, 0.0, 1.0, 1.0],
        0,
        &mut lazy,
        &mut atlas,
        &mut cached,
        &mut runs,
        true,
        &[],
        0.0,
        Some(&selection),
    );

    assert_eq!(out.len(), 12);
    // `@font-palette-values { override-colors: 0 green }` обязан
    // перекрасить первый слой; красный здесь означал бы, что выбор
    // палитры не доехал из `DrawText`.
    assert!(close(out[0].color, [0.0, 1.0, 0.0, 1.0]), "layer 0 color {:?}", out[0].color);
    // Слой `0xFFFF` остаётся текстового цвета независимо от override-ов.
    assert!(close(out[6].color, [0.0, 0.0, 1.0, 1.0]));
}

/// BUG-405 срез 13: run с цветным глифом мимо кэша.
///
/// Его квады зависят от палитры и от цвета текста, которых в ключе нет, —
/// попади он в кэш, второй вызов с другим цветом вернул бы чужие вершины.
/// Гейт — счётчик: попаданий обязано остаться ноль на любом числе вызовов.
#[test]
fn text_run_cache_skips_color_glyph_runs() {
    let bytes = build_color_font();
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace { bytes: Arc::from(bytes.as_slice()), metrics }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut runs = TextRunCache::default();
    let mut out: Vec<TextVertex> = Vec::new();

    for _ in 0..3 {
        out.clear();
        push_text_glyphs(
            &mut out,
            Rect::new(0.0, 0.0, 100.0, 40.0),
            "A",
            32.0,
            [0.0, 0.0, 1.0, 1.0],
            0,
            &mut lazy,
            &mut atlas,
            &mut cached,
            &mut runs,
            true,
            &[],
            0.0,
            None,
        );
    }
    assert_eq!(out.len(), 12, "цветной глиф всё ещё даёт quad на слой");
    assert_eq!(runs.hits, 0, "run с цветным глифом попал в кэш");
    assert_eq!(runs.misses, 3, "каждый вызов обязан уложить run заново");
}

/// BUG-405 срез 13: попадание кэша укладки даёт ПОБИТОВО те же вершины.
///
/// План хранит шаги, а не готовые вершины, ровно ради этого: перо
/// стартует с `rect.x` и накапливает те же слагаемые в том же порядке.
/// Сравнение по битам, а не с допуском: сложение `f32` не ассоциативно, и
/// расхождение в ULP означало бы, что позиция вынесена из плана неверно.
#[test]
fn text_run_cache_replays_identical_vertices() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let metrics = build_face_metrics(&bytes);
    let faces = vec![LoadedFace { bytes: Arc::from(bytes.as_slice()), metrics }];
    let mut lazy = LazyParsedFaces::new(&faces);
    let mut atlas = GlyphAtlas::new(ATLAS_DIM);
    let mut cached: HashMap<AtlasKey, Option<CachedGlyph>> = HashMap::new();
    let mut runs = TextRunCache::default();

    let mut lay = |runs: &mut TextRunCache, enabled: bool, y: f32| {
        let mut out: Vec<TextVertex> = Vec::new();
        push_text_glyphs(
            &mut out,
            Rect::new(13.5, y, 400.0, 30.0),
            "Привет, world!",
            17.0,
            [0.1, 0.2, 0.3, 1.0],
            0,
            &mut lazy,
            &mut atlas,
            &mut cached,
            runs,
            enabled,
            &[],
            0.0,
            None,
        );
        out
    };

    // Плечо отката: кэш не трогается ни разу.
    let mut off = TextRunCache::default();
    let base = lay(&mut off, false, 40.0);
    assert!(!base.is_empty(), "run обязан дать вершины");
    assert_eq!((off.hits, off.misses), (0, 0), "плечо отката трогало кэш");

    let first = lay(&mut runs, true, 40.0);
    assert_eq!((runs.hits, runs.misses), (0, 1), "первый вызов не мог попасть");
    let second = lay(&mut runs, true, 40.0);
    assert_eq!((runs.hits, runs.misses), (1, 1), "повторный вызов не попал в кэш");

    assert!(bits_eq(&first, &base), "укладка с кэшом разошлась с откатом");
    assert!(bits_eq(&second, &first), "попадание разошлось с укладкой");

    // Другая позиция — тот же ключ (позиция не в ключе), но вершины
    // обязаны сдвинуться: план воспроизводит перо от нового `rect`.
    let moved = lay(&mut runs, true, 90.0);
    assert_eq!(runs.hits, 2, "сдвиг по вертикали обязан быть попаданием");
    assert_eq!(moved.len(), first.len());
    assert!(
        moved.iter().zip(&first).all(|(a, b)| a.pos[0] == b.pos[0] && a.pos[1] > b.pos[1]),
        "попадание проигнорировало новую позицию run-а",
    );
}

/// BUG-405 срез 20: прогретая полоса обязана остаться НЕВАЛИДНОЙ.
///
/// Прогрев создаёт текстуры полосы заранее и чистит их пустым пассом —
/// содержимого в них нет. Пиксельная нейтральность правки держится ровно
/// на нулевом ключе: пока он не выставлен прошедшим Band-рендером,
/// `try_page_compose` не может сблитить полосу на экран. Тест гейтит
/// именно это, а не скорость: скорость меряет A/B через
/// `LUMEN_NO_BAND_WARM`.
///
/// Требует GPU-адаптер, поэтому `#[ignore]` — как headless-тесты в
/// `tests/cases/headless_tests.rs`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  warm_page_band_leaves_band_invalid -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn warm_page_band_leaves_band_invalid() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let mut r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    assert!(r.page_band.is_none(), "полоса не могла возникнуть до прогрева");

    r.warm_page_band(64, 200);
    let band = r.page_band.as_ref().expect("прогрев обязан создать полосу");
    assert_eq!((band.w_px, band.h_px), (64, 200), "прогрет чужой размер");
    assert_eq!(band.key, 0, "прогретая полоса выдала себя за валидную");

    // Смена размера заменяет полосу целиком и снова оставляет её невалидной.
    r.create_page_band(80, 240, 17.0);
    let band = r.page_band.as_ref().expect("полоса обязана пересоздаться");
    assert_eq!((band.w_px, band.h_px), (80, 240), "размер не обновился");
    assert_eq!(band.key, 0, "пересозданная полоса выдала себя за валидную");
}

/// BUG-405 срез 38: смещение страницы через `set_page_offset` даёт ТЕ ЖЕ
/// пиксели, что обёртка `PushTransform`, которую шелл строил каждый кадр.
///
/// Гейт правки, и он именно об идентичности, а не о скорости: снятая
/// статья — копия display list-а (0.42 мс, 19 % кадра попадания на стенде
/// среза 37), но принимать правку можно только доказав, что кадр от неё не
/// изменился. Список нарочно содержит вложенный `PushTransform` с
/// поворотом: страничная трансляция обязана остаться САМОЙ ВНЕШНЕЙ
/// (`page · rot`), а наивная реализация «прибавить смещение к rect-у»
/// дала бы `rot · page` и провалила бы этот `assert`. Overlay в обеих
/// половинах один и тот же и смещения не берёт — если бы затравка не
/// снималась на границе списков, хром уехал бы вниз вместе со страницей.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  page_offset_matches_push_transform_wrapper -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn page_offset_matches_push_transform_wrapper() {
    use crate::DisplayCommand as C;

    let (off_x, off_y) = (7.0_f32, 11.0_f32);
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect { x, y, width: w, height: h };
    let rgb = |r: u8, g: u8, b: u8| Color { r, g, b, a: 255 };
    let radii = |v: f32| crate::CornerRadii {
        tl: v, tr: v, br: v, bl: v, tl_y: v, tr_y: v, br_y: v, bl_y: v,
    };
    let base: Vec<C> = vec![
        C::FillRect { rect: rect(2.0, 3.0, 20.0, 10.0), color: rgb(200, 30, 40) },
        C::PushClipRect { rect: rect(4.0, 6.0, 30.0, 20.0) },
        C::FillRoundedRect {
            rect: rect(5.0, 7.0, 25.0, 15.0),
            color: rgb(30, 160, 90),
            radii: radii(4.0),
        },
        C::PopClip,
        C::PushTransform { matrix: Mat4::rotate_2d(0.35) },
        C::FillRect { rect: rect(10.0, 12.0, 14.0, 9.0), color: rgb(20, 60, 220) },
        C::PopTransform,
    ];
    // Overlay — viewport-locked хром: в обеих половинах идёт отдельным
    // списком и обязан нарисоваться в одних и тех же пикселях.
    let overlay: Vec<C> =
        vec![C::FillRect { rect: rect(0.0, 0.0, 64.0, 4.0), color: rgb(250, 250, 10) }];

    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let mut r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");

    // Плечо A — как рисовал шелл до среза: смещения у рендерера нет,
    // страница завёрнута в `PushTransform` внутри самого списка.
    let mut wrapped: Vec<C> = Vec::with_capacity(base.len() + 2);
    wrapped.push(C::PushTransform { matrix: Mat4::translation_2d(off_x, off_y) });
    wrapped.extend_from_slice(&base);
    wrapped.push(C::PopTransform);
    let a = r
        .render_to_image_with_overlay(&wrapped, &overlay, 5.0, 0.0)
        .expect("render плеча A");

    // Плечо B — фаст-пас: тот же список ПО ССЫЛКЕ, смещение у рендерера.
    r.set_page_offset(off_x, off_y);
    let b = r
        .render_to_image_with_overlay(&base, &overlay, 5.0, 0.0)
        .expect("render плеча B");

    assert_eq!((a.width, a.height), (b.width, b.height), "разные размеры кадров");
    assert!(
        a.data == b.data,
        "фаст-пас нарисовал не то же, что обёртка: {} байт из {} различаются",
        a.data.iter().zip(&b.data).filter(|(x, y)| x != y).count(),
        a.data.len(),
    );

    // Нулевое смещение обязано вернуть путь «матрицы нет» — headless и
    // `render_to_image` обёртки не знали и знать не должны.
    r.set_page_offset(0.0, 0.0);
    assert_eq!(r.page_offset(), (0.0, 0.0), "сброс смещения не сработал");
    // Нефинитное значение — «без смещения», как у femtovg.
    r.set_page_offset(f32::NAN, 3.0);
    assert_eq!(r.page_offset(), (0.0, 0.0), "NaN просочился в CTM");
}

/// BUG-405 срез 41: overlay-кэш (`Renderer::overlay_cache_step`) даёт ТЕ
/// ЖЕ пиксели, что полная перерисовка overlay-списка каждый кадр.
///
/// Первая версия этого среза (реплей горячей команды ПОВЕРХ всего
/// остального, с восстановлением контекста через
/// `anim_split_compose_plan`) была забракована переписью на реальном
/// хроме: скроллбар геометрически пересекается с фоновой панелью
/// хедера, рисуемой ПОЗЖЕ — реплей поверх всего накрыл бы его чужим
/// содержимым, и `anim_split_compose_plan` совершенно верно отказывал
/// на каждом кадре стенда (0 из ~750 попыток). Эта версия порядок не
/// меняет: нестабильный ПРЕФИКС списка рисуется живьём, стабильный
/// ХВОСТ остаётся на своём месте — блитуется. Список нарочно кладёт
/// нестабильную команду ПЕРЕД клипом, а не внутри него: точка разреза
/// обязана дотянуться до ближайшей СБАЛАНСИРОВАННОЙ по push/pop
/// границы, а не просто до первого различия (`balanced_cut_at_or_after`).
///
/// Гейт слайса, и он именно об идентичности: `pending_overlay_blit`
/// проверяется через ОБЩИЙ по режимам путь `render_impl` (границу
/// content|overlay), поэтому headless (без `wgpu::Surface`, где
/// `compose_page` в принципе недостижим — `prepare_page_compose` рубит
/// его первым же условием) годится как стенд: вызывающий сам решает,
/// что подать в overlay-параметр `render()`, ровно как `compose_page`
/// решает это для Compose-пасса.
///
/// Четыре кадра подряд бьют четыре разных пути `overlay_cache_step`:
/// A — кэша ещё нет (первый кадр, ничего не с чем сравнивать); B —
/// нестабильная команда (индекс 0, ДО клипа) сменилась с прошлого
/// кадра → разрез тривиален (1, сразу после неё — клип целиком уходит
/// в хвост нетронутым), кэш строится и используется тем же кадром
/// (MISS); C — список побуквенно тот же, что B → кэш валиден без
/// пересборки (HIT); D меняет команду ВНУТРИ кэшированного хвоста
/// (индекс 2, ПОД клипом) — наивный разрез сразу за ней (3) лёг бы
/// ПОСЕРЕДИНЕ открытого клипа, поэтому `balanced_cut_at_or_after`
/// обязана сдвинуть его до 4 (сразу за `PopClip`), и кэш обязан
/// признать хвост устаревшим и пересобраться заново с этим разрезом.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  overlay_cache_matches_full_overlay_redraw -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn overlay_cache_matches_full_overlay_redraw() {
    use crate::DisplayCommand as C;

    let rect = |x: f32, y: f32, w: f32, h: f32| Rect { x, y, width: w, height: h };
    let rgb = |r: u8, g: u8, b: u8| Color { r, g, b, a: 255 };

    // index: 0 НЕСТАБИЛЬНАЯ (аналог ползунка — меняется каждый кадр),
    // 1 PushClipRect, 2 команда под клипом, 3 PopClip, 4 сосед-после.
    let overlay_with = |unstable: Color, clipped: Color| -> Vec<C> {
        vec![
            C::FillRect { rect: rect(0.0, 0.0, 10.0, 10.0), color: unstable },
            C::PushClipRect { rect: rect(20.0, 0.0, 10.0, 10.0) },
            C::FillRect { rect: rect(20.0, 0.0, 10.0, 10.0), color: clipped },
            C::PopClip,
            C::FillRect { rect: rect(40.0, 0.0, 10.0, 10.0), color: rgb(30, 160, 90) },
        ]
    };
    let stable_clip = rgb(80, 90, 200);
    let overlay_a = overlay_with(rgb(20, 60, 220), stable_clip);
    let overlay_b = overlay_with(rgb(250, 210, 10), stable_clip);
    // D: команда ПОД КЛИПОМ (индекс 2, внутри закэшированного хвоста
    // кадра B/C) сменилась — кэш обязан пересобраться, а не показать
    // устаревший цвет из текстуры.
    let overlay_d = overlay_with(rgb(250, 210, 10), rgb(10, 200, 140));

    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");

    // Плечо-эталон: та же последовательность кадров, но каждый — полная
    // перерисовка (второй, независимый headless-рендерер, чтобы кэш
    // тестируемого не мог просочиться в эталон никаким состоянием).
    let mut r_ref = Renderer::new_headless(bytes.clone(), 64, 48, ColorSpace::Srgb)
        .expect("headless renderer (эталон)");
    let ref_a = r_ref
        .render_to_image_with_overlay(&[], &overlay_a, 0.0, 0.0)
        .expect("эталон A");
    let ref_b = r_ref
        .render_to_image_with_overlay(&[], &overlay_b, 0.0, 0.0)
        .expect("эталон B");
    let ref_c = r_ref
        .render_to_image_with_overlay(&[], &overlay_b, 0.0, 0.0)
        .expect("эталон C (= B)");
    let ref_d = r_ref
        .render_to_image_with_overlay(&[], &overlay_d, 0.0, 0.0)
        .expect("эталон D");

    let mut r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer (тест)");

    // Кадр A: кэша ещё нет — `overlay_cache_step` обязан отказаться.
    let prefix_a = r.overlay_cache_step(&overlay_a).expect("шаг A");
    assert!(prefix_a.is_none(), "кадр без предыдущего не мог построить кэш");
    assert!(r.overlay_cache.is_none(), "кэш не мог возникнуть на кадре A");
    let img_a = r
        .render_to_image_with_overlay(&[], &overlay_a, 0.0, 0.0)
        .expect("render A");
    assert_eq!(img_a.data, ref_a.data, "кадр A разошёлся с эталоном без всякого кэша");

    // Кадр B: индекс 0 сменился — MISS, разрез сдвигается балансом с 1
    // (первое различие) до 4 (мимо клипа), кэш строится и уже
    // используется этим же кадром.
    let prefix_b = r.overlay_cache_step(&overlay_b).expect("шаг B");
    let prefix_b = prefix_b.expect("кадр B обязан был построить и применить кэш");
    assert_eq!(prefix_b, 1, "разрез B — сразу после нестабильной команды, клип не тронут");
    assert!(r.overlay_cache.is_some(), "кэш обязан существовать после кадра B");
    let img_b = r
        .render_to_image_with_overlay(&[], &overlay_b[..prefix_b], 0.0, 0.0)
        .expect("render B (живой префикс + кэш хвоста)");
    assert_eq!(
        img_b.data, ref_b.data,
        "MISS-кадр B: {} байт из {} разошлись с полной перерисовкой",
        img_b.data.iter().zip(&ref_b.data).filter(|(x, y)| x != y).count(),
        img_b.data.len(),
    );

    // Кадр C: список побуквенно тот же, что B — HIT, кэш переиспользуется
    // без пересборки текстуры. `&c._texture as *const _` тут не годится:
    // это адрес ПОЛЯ `self.overlay_cache`, то есть смещение внутри
    // `self` — он одинаков независимо от того, какое значение лежит по
    // этому смещению. Считаем сами создания текстур глобальным
    // счётчиком (`TEXTURES_CREATED`, уже используется движком для
    // ровно такого гейта в других тестах этого файла).
    let created_before_c = load_counter(&TEXTURES_CREATED);
    let prefix_c = r.overlay_cache_step(&overlay_b).expect("шаг C");
    let prefix_c = prefix_c.expect("HIT обязан вернуть длину живого префикса");
    assert_eq!(prefix_c, prefix_b, "HIT обязан вернуть тот же разрез, что MISS");
    assert_eq!(
        load_counter(&TEXTURES_CREATED), created_before_c,
        "HIT пересоздал текстуру кэша"
    );
    let img_c = r
        .render_to_image_with_overlay(&[], &overlay_b[..prefix_c], 0.0, 0.0)
        .expect("render C (HIT)");
    assert_eq!(
        img_c.data, ref_c.data,
        "HIT-кадр C: {} байт из {} разошлись с полной перерисовкой",
        img_c.data.iter().zip(&ref_c.data).filter(|(x, y)| x != y).count(),
        img_c.data.len(),
    );

    // Кадр D: сменилась команда ВНУТРИ кэшированного хвоста — кэш
    // обязан пересобраться (не показать устаревший цвет из текстуры).
    let created_before_d = load_counter(&TEXTURES_CREATED);
    let prefix_d = r.overlay_cache_step(&overlay_d).expect("шаг D");
    let prefix_d = prefix_d.expect("хвост изменился, но разрез всё ещё возможен");
    assert_eq!(
        prefix_d, 4,
        "наивный разрез (3, сразу после изменившейся команды) лёг бы внутри клипа — \
         balanced_cut_at_or_after обязана была сдвинуть его за PopClip"
    );
    assert_eq!(
        load_counter(&TEXTURES_CREATED), created_before_d + 1,
        "изменение под клипом обязано было пересобрать РОВНО одну новую текстуру кэша"
    );
    let img_d = r
        .render_to_image_with_overlay(&[], &overlay_d[..prefix_d], 0.0, 0.0)
        .expect("render D (кэш пересобран)");
    assert_eq!(
        img_d.data, ref_d.data,
        "кадр D разошёлся с эталоном после пересборки кэша: {} байт из {} различаются",
        img_d.data.iter().zip(&ref_d.data).filter(|(x, y)| x != y).count(),
        img_d.data.len(),
    );
}

/// BUG-406 срез 2: пять горячих пайплайнов собраны РАЗНЫМИ потоками.
///
/// Гейт правки. Wall-clock старта в него не годится: на DX12 разброс
/// между прогонами одного бинарника доходит до 2.5× (`docs/perf-method.md`,
/// числа — в `bugs/BUG-406-OPEN.md`), поэтому проверяется идентичность —
/// компиляции действительно выданы с пяти разных потоков, а не сложены
/// в один. Дефект переноса (случайно вернувшаяся последовательная сборка)
/// уронит именно этот `assert_eq`.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  hot_pipelines_built_on_distinct_threads -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn hot_pipelines_built_on_distinct_threads() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    // Срез 3: пайплайны приезжают с фоновых потоков лениво, поэтому потоки
    // становятся известны только после материализации всех пяти. Кадр
    // делает ровно этот вызов на входе в `render`.
    r.await_all_hot_pipelines();
    assert_eq!(
        r.hot_pipeline_threads(),
        5,
        "горячие пайплайны снова компилируются на одном потоке",
    );
}

/// BUG-406 срез 3: горячие пайплайны не должен компилировать UI-поток.
///
/// Гейт среза. Срез 2 увёл компиляции на пять потоков, но конструктор
/// рендера ждал их все, поэтому окно не пампило сообщения всё время
/// сборки. Срез 3 ожидание убрал: потоки стартуют в `init_pipelines`, а
/// вызывающий забирает готовое позже. Сколько это дало по времени —
/// вопрос wall-clock (числа в `bugs/BUG-406-OPEN.md`); тест пиннит
/// утверждение об идентичности: ни один из пяти не собран потоком-
/// владельцем рендера. Ненулевой счётчик означает, что аварийная ветка
/// `await_hot` сработала и цена вернулась на UI-поток.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  hot_pipelines_never_built_on_owner_thread -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn hot_pipelines_never_built_on_owner_thread() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    r.await_all_hot_pipelines();
    assert_eq!(
        r.hot_pipelines_built_on_ui_thread(),
        0,
        "горячий пайплайн скомпилирован потоком-владельцем рендера",
    );
    assert!(
        !r.hot_pipeline_threads.borrow().contains(&std::thread::current().id()),
        "поток-владелец попал в число сборщиков горячих пайплайнов",
    );
}

/// BUG-453: потерянное устройство не должно паниковать `render()`/`resize()`.
///
/// `wgpu::SurfaceTexture::present()` возвращает `()` и паникует внутри
/// библиотеки при потере устройства — перехватить это исключение из
/// `render_impl` нельзя, единственный корректный вариант — не доходить
/// до вызова. Тест не воспроизводит настоящую потерю устройства (TDR —
/// свойство драйвера, а не тестового окружения), а напрямую взводит
/// `device_lost` — ту же ячейку, которую в проде заполняет коллбэк
/// `Device::set_device_lost_callback`, — и проверяет, что оба места,
/// читающие её (`render_impl`/`resize`), деградируют вместо падения.
/// Headless-режим (`windowed_frame` всегда `None`, `present()` не
/// вызывается вовсе) здесь достаточен: правка — общий ранний выход в
/// начале `render_impl`, до какого-либо обращения к `device`/`queue`,
/// а не что-то специфичное для swapchain-пути.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  device_lost_skips_present_instead_of_panicking -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn device_lost_skips_present_instead_of_panicking() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let mut r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    assert!(
        r.device_lost_reason().is_none(),
        "свежий рендерер не должен считаться потерявшим устройство",
    );
    assert!(
        r.render(&[], &[], 0.0, 0.0).is_ok(),
        "обычный кадр на живом устройстве обязан рисоваться",
    );
    r.device_lost
        .set("Unknown: simulated TDR".to_string())
        .expect("флаг ещё не должен быть взведён");
    assert_eq!(
        r.device_lost_reason().as_deref(),
        Some("Unknown: simulated TDR"),
        "device_lost_reason() обязан вернуть причину, а не булев флаг",
    );
    let result = r.render(&[], &[], 0.0, 0.0);
    assert!(
        matches!(result, Err(wgpu::SurfaceError::Lost)),
        "render() на потерянном устройстве обязан вернуть Err, а не рисовать: {result:?}",
    );
    // resize() не должен трогать device/surface на потерянном устройстве —
    // не паникует и есть весь смысл проверки.
    r.resize(128, 96);
}

/// BUG-771: слот пре-резолва face-а адресуется индексом САМОЙ команды.
///
/// Дефект был не в резолве, а в его чтении: id-ы клались подряд только за
/// `DrawText`, а забирались курсором по мере отрисовки — и команда текста,
/// не дошедшая до своей ветки (viewport-кулинг), курсор не двигала, после
/// чего весь остаток кадра рисовался чужими face-ами. Тест пиннит
/// инвариант, который это исключает: у каждой команды свой слот, и он не
/// зависит ни от того, сколько команд текста было раньше, ни от того,
/// нарисовались ли они.
#[test]
fn text_face_ids_are_addressed_by_command_index() {
    fn text(family: &str) -> DisplayCommand {
        DisplayCommand::DrawText {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            text: "x".to_string(),
            font_size: 12.0,
            color: Color { r: 0, g: 0, b: 0, a: 255 },
            font_family: vec![family.to_string()],
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_stretch: FontStretch::NORMAL,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 8.0,
            highlight_name: None,
            text_orientation: None,
        }
    }
    let fill = || DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        color: Color { r: 0, g: 0, b: 0, a: 255 },
    };
    // Страница: текст между нетекстовыми командами; хром: свой текст.
    let content = vec![fill(), text("page-a"), fill(), text("page-b")];
    let overlay = vec![fill(), text("chrome")];
    // Резолвер отвечает по имени family — так видно, какой команде достался
    // чей ответ.
    let ids = resolve_text_face_ids(&content, &overlay, |fam, _, _, _| {
        match fam.first().map(String::as_str) {
            Some("page-a") => 10,
            Some("page-b") => 20,
            Some("chrome") => 30,
            _ => 0,
        }
    });

    assert_eq!(ids.len(), content.len() + overlay.len(), "слот не у каждой команды");
    assert_eq!(ids[text_face_slot(false, 1, content.len())], 10);
    assert_eq!(ids[text_face_slot(false, 3, content.len())], 20);
    assert_eq!(ids[text_face_slot(true, 1, content.len())], 30);
    for (list, idx) in [(false, 0), (false, 2), (true, 0)] {
        assert_eq!(
            ids[text_face_slot(list, idx, content.len())],
            NO_TEXT_FACE,
            "нетекстовая команда заняла слот face-а",
        );
    }

    // Ядро регрессии: пропуск команды текста (кулинг) не смещает соседей.
    // Курсорное чтение отдало бы хрому ответ для `page-b`.
    let drawn: Vec<usize> = [(false, 1), (true, 1)]
        .into_iter()
        .map(|(list, idx)| ids[text_face_slot(list, idx, content.len())])
        .collect();
    assert_eq!(drawn, vec![10, 30], "пропуск соседней команды сдвинул face-ы");
}

/// BUG-405 срез 21: bind group блита обязана создаваться ровно один раз
/// на полосу.
///
/// Её входы — view полосы и постоянный sampler — меняются только вместе с
/// самой полосой, поэтому каждый Compose-кадр брал готовую группу, а не
/// собирал новую (прогон прокрутки `lenta.ru`: 40 наборов дескрипторов за
/// прогон против 1). Тест пиннит связку «создание полосы = создание
/// группы»: возврат пересборки на кадр поднимет счётчик выше числа
/// созданных полос.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`; запуск:
/// `cargo test -p lumen-paint --features backend-wgpu
///  band_blit_bind_group_lives_with_band -- --include-ignored`.
#[test]
#[ignore = "requires GPU adapter"]
fn band_blit_bind_group_lives_with_band() {
    let bytes = std::fs::read("../../../assets/fonts/Inter-Regular.ttf")
        .expect("bundled font");
    let mut r = Renderer::new_headless(bytes, 64, 48, ColorSpace::Srgb)
        .expect("headless renderer");
    let before = BAND_BLIT_BGS_CREATED.load(std::sync::atomic::Ordering::Relaxed);

    r.create_page_band(64, 200, 0.0);
    assert_eq!(
        BAND_BLIT_BGS_CREATED.load(std::sync::atomic::Ordering::Relaxed) - before,
        1,
        "полоса создана, а группа блита — нет (или собрана дважды)",
    );

    // Пересоздание полосы (смена размера окна) обязано дать новую группу:
    // старая ссылается на исчезнувший view.
    r.create_page_band(80, 240, 17.0);
    assert_eq!(
        BAND_BLIT_BGS_CREATED.load(std::sync::atomic::Ordering::Relaxed) - before,
        2,
        "пересозданная полоса осталась со старой группой блита",
    );
}

/// BUG-405 срез 22: полоса выше лимита текстуры ужимается, а не отключает
/// композитор.
///
/// Гейт на арифметику [`band_geometry`], без GPU: до среза окно клиентской
/// высотой больше ~819 device px давало отказ (полная полоса — 2.5
/// вьюпорта, лимит `downlevel_defaults` — 2048), то есть скролл-композитор
/// не работал почти ни на одном развёрнутом окне. Возврат безусловного
/// отказа виден здесь как `Err` на строке 952 px.
#[test]
fn band_geometry_clamps_to_texture_limit() {
    const MAX: u32 = 2048;

    // Окно, при котором полная полоса влезает: ужатие ничего не меняет,
    // числа обязаны совпасть с формулой «3/4 вьюпорта с каждой стороны».
    let (m, h) = band_geometry(1024, 792, 1.0, MAX, true, None).expect("полоса влезает целиком");
    assert_eq!((m, h), (594, 1980), "ужатие тронуло полосу, которая влезала");
    assert_eq!(
        band_geometry(1024, 792, 1.0, MAX, false, None),
        Ok((594, 1980)),
        "плечи разошлись там, где ужатие не срабатывает",
    );

    // Окно после роста (перепись среза): полная полоса — 2380 px, лимит
    // 2048. С ужатием полоса живёт, без него композитор выключается.
    let (m, h) = band_geometry(1184, 952, 1.0, MAX, true, None).expect("полоса обязана ужаться");
    assert_eq!((m, h), (548, 2048), "запас ужат не до лимита");
    assert!(
        (m as f32) >= BAND_MIN_MARGIN_RATIO * 952.0,
        "ужатый запас обязан остаться выше порога полезности",
    );
    assert_eq!(
        band_geometry(1184, 952, 1.0, MAX, false, None),
        Err("полоса выше лимита текстуры (ужатие отключено)"),
        "плечо отката обязано повторять поведение до среза",
    );

    // Вьюпорт, которому лимит не оставляет полезного запаса: 1700 px дают
    // 174 px запаса при пороге 425 — честнее монолит.
    assert_eq!(
        band_geometry(1024, 1700, 1.0, MAX, true, None),
        Err("вьюпорт не оставляет запаса в лимите текстуры"),
        "полоса с бесполезным запасом обязана отключаться",
    );
    // Поверхность крупнее лимита — полоса невозможна в принципе, и
    // `max_dim - sh` не должен уйти в минус.
    assert_eq!(
        band_geometry(1024, 2100, 1.0, MAX, true, None),
        Err("вьюпорт выше лимита текстуры"),
    );
    assert_eq!(
        band_geometry(4096, 800, 1.0, MAX, true, None),
        Err("вьюпорт выше лимита текстуры"),
    );
    assert_eq!(band_geometry(0, 800, 1.0, MAX, true, None), Err("нулевой размер поверхности"));

    // dpr ≠ 1: запас считается в device px, потолок 768 — в CSS px.
    let (m, h) = band_geometry(2048, 1200, 2.0, 4096, true, None).expect("полоса при dpr 2");
    assert_eq!((m, h), (900, 3000), "потолок 768 CSS px применён не в тех единицах");
}

/// BUG-405 срез 27: запас полосы переопределяется целиком, и это меняет
/// ТОЛЬКО её высоту.
///
/// Гейт на рычаг переписи `LUMEN_BAND_MARGIN_CSS`: свип цены промаха по
/// площади строится на том, что при неизменных вьюпорте и содержимом высота
/// полосы задаётся одним числом. Переопределение обязано быть полным, а не
/// потолком: штатный запас на развёрнутом окне упирается в долю 0.75
/// вьюпорта (762 px) раньше, чем в потолок 768 CSS px, поэтому потолком
/// свип не поднял бы полосу выше штатной — три верхние точки первого свипа
/// (768/1100/1500) дали одну и ту же полосу 2541 px. Ограничения выше
/// рычага (лимит текстуры, порог полезности) обязаны продолжать работать.
#[test]
fn band_geometry_honours_margin_override() {
    const MAX: u32 = 8192;
    // Развёрнутое окно 1920×1017: штатный запас — доля 0.75, то есть 762 px.
    assert_eq!(band_geometry(1920, 1017, 1.0, MAX, true, None), Ok((762, 2541)));
    // Рычаг и ужимает полосу, и поднимает её выше штатной.
    assert_eq!(band_geometry(1920, 1017, 1.0, MAX, true, Some(400.0)), Ok((400, 1817)));
    assert_eq!(band_geometry(1920, 1017, 1.0, MAX, true, Some(1500.0)), Ok((1500, 4017)));
    // Потолок 768 CSS px виден только на окне повыше, где доля его перебивает.
    assert_eq!(band_geometry(1920, 1400, 1.0, MAX, true, None), Ok((768, 2936)));
    // Лимит текстуры сильнее рычага: 3000 px запаса дали бы полосу 7017 px.
    assert_eq!(band_geometry(1920, 1017, 1.0, 4096, true, Some(3000.0)), Ok((1539, 4095)));
    // Порог полезности сильнее рычага: 0.25 вьюпорта от 1017 — это 254 px,
    // и запас ниже него обязан отключать композитор, а не давать полосу.
    assert_eq!(
        band_geometry(1920, 1017, 1.0, MAX, true, Some(100.0)),
        Err("вьюпорт не оставляет запаса в лимите текстуры"),
    );
}

/// BUG-405 срез 29: гейт на арифметику рычага переписи
/// [`band_draw_fraction`] → [`band_cull_height`].
///
/// Рычаг понижает высоту цели ТОЛЬКО для отсева, поэтому его единственная
/// опасная точка — вырождение: доля, дающая ноль строк, схлопнула бы каждый
/// scissor в пустой, и свип померил бы «полоса не рисуется» вместо «полоса
/// рисуется на четверть». Полная доля обязана возвращать саму высоту —
/// иначе плечо 1.0 свипа не совпадает со штатным путём.
#[test]
fn band_cull_height_is_a_fraction_of_the_target() {
    // Полоса развёрнутого окна 1920×1017 (см. `band_geometry` выше).
    assert_eq!(band_cull_height(2541, 1.0), 2541, "полная доля = штатный путь");
    assert_eq!(band_cull_height(2541, 0.5), 1271);
    assert_eq!(band_cull_height(2541, 0.25), 635);
    // Вырождение: доля меньше строки — одна строка, но не ноль.
    assert_eq!(band_cull_height(2541, 0.0001), 1);
    // Доля выше единицы не растягивает цель: отсев не может видеть больше
    // текстуры, чем создано.
    assert_eq!(band_cull_height(2541, 2.0), 2541);
}

/// BUG-405 срез 32: гейт на арифметику кольцевой дорисовки
/// [`ring_advance_plan`] — какие строки текстуры перерисовывает сдвиг
/// полосы.
///
/// Опасных точек три, и все три проверяются здесь, потому что живой стенд
/// на них не наступит: (1) перекрытие обязано ПЕРЕЖИВАТЬ сдвиг — план,
/// покрывающий больше строк, чем |сдвиг|, означал бы, что кольцо ничего не
/// экономит; (2) кромка, разрезанная краем текстуры, обязана прийти двумя
/// диапазонами — один пасс через край невозможен, scissor непрерывен;
/// (3) сдвиг не меньше высоты полосы обязан ОТКАЗАТЬ (перекрытия нет
/// вовсе), а не вернуть план на всю полосу задом наперёд.
#[test]
fn ring_advance_plan_redraws_only_the_edge() {
    // Полоса развёрнутого окна 1920×1017 (см. `band_geometry` выше).
    const H: u32 = 2541;
    // Первый сдвиг вниз от свежей полосы: база = прежний верх, значит
    // кромка ложится в голову текстуры, край её не режет.
    assert_eq!(
        ring_advance_plan(H, 0, 0, 1229),
        Some(vec![RingStrip { row0: 0, rows: 1229, doc_y0: 2541 }]),
    );
    // Второй сдвиг той же длины: фаза уже 1229, но кромка ещё влезает в
    // хвост текстуры целиком (1229 + 1229 ≤ 2541) — по-прежнему один пасс.
    assert_eq!(
        ring_advance_plan(H, 0, 1229, 2458),
        Some(vec![RingStrip { row0: 1229, rows: 1229, doc_y0: 3770 }]),
    );
    // Третий сдвиг: до края текстуры осталось 83 строки, и кромку режет
    // край — два диапазона, вместе ровно |сдвиг| строк.
    assert_eq!(
        ring_advance_plan(H, 0, 2458, 3687),
        Some(vec![
            RingStrip { row0: 2458, rows: 83, doc_y0: 4999 },
            RingStrip { row0: 0, rows: 1146, doc_y0: 5082 },
        ]),
    );
    // Скролл вверх обновляет ГОЛОВУ полосы, а не хвост.
    assert_eq!(
        ring_advance_plan(H, 0, 1000, 700),
        Some(vec![RingStrip { row0: 700, rows: 300, doc_y0: 700 }]),
    );
    // Сдвиг ровно на высоту полосы и больше — перекрытия нет, кольцо
    // отказывает в пользу полной перерисовки.
    assert_eq!(ring_advance_plan(H, 0, 0, i64::from(H)), None);
    assert_eq!(ring_advance_plan(H, 0, 5000, 0), None);
    // Нулевой сдвиг — промах пришёл не от движения полосы, дорисовывать
    // нечего: полная перерисовка (иначе кольцо вернуло бы пустой план и
    // полоса осталась бы с прежним содержимым под новым ключом).
    assert_eq!(ring_advance_plan(H, 0, 700, 700), None);
    // Инвариант на весь диапазон сдвигов при произвольной фазе: строк
    // ровно |сдвиг|, они лежат внутри текстуры и не пересекаются.
    for base in [0_i64, 137, 2540] {
        for delta in [-2540_i64, -999, -1, 1, 613, 2540] {
            let strips = ring_advance_plan(H, base, 4000, 4000 + delta)
                .unwrap_or_else(|| panic!("нет плана для сдвига {delta}"));
            let rows: u32 = strips.iter().map(|s| s.rows).sum();
            assert_eq!(u64::from(rows), delta.unsigned_abs(), "сдвиг {delta}");
            for s in &strips {
                assert!(s.rows > 0 && s.row0 + s.rows <= H, "вышли за текстуру: {s:?}");
            }
            if let [a, b] = strips.as_slice() {
                assert_eq!(a.row0 + a.rows, H, "первый диапазон обязан упереться в край");
                assert_eq!(b.row0, 0, "второй обязан начинаться со строки 0");
            }
        }
    }
}

/// BUG-405 срез 32: гейт на квад блита кольцевой полосы
/// [`band_blit_quads`].
///
/// Нулевая фаза обязана давать РОВНО uv `0…1` — это путь до среза, и любое
/// его изменение переставляло бы пиксели на каждом кадре композиции, а не
/// только после сдвига полосы. Ненулевая фаза обязана сдвигать ОБА конца
/// диапазона на одну и ту же долю: диапазон короче или длиннее единицы
/// растянул бы полосу по вертикали, а это уже не сдвиг, а масштаб.
#[test]
fn band_blit_quad_offsets_uv_by_the_ring_phase() {
    const H: u32 = 2541;
    let one = band_blit_quads(-762.0, 1920.0, H, 0, 1.0);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].1, [0.0, 0.0]);
    assert_eq!(one[0].2, [1.0, 1.0]);
    assert_eq!(one[0].0.height, 2541.0);

    let shifted = band_blit_quads(-762.0, 1920.0, H, 1229, 1.0);
    assert_eq!(shifted.len(), 1, "квад один при любой фазе — заворачивает sampler");
    // Геометрия квада от фазы не зависит: сдвигаются только uv.
    assert_eq!(shifted[0].0.height, one[0].0.height);
    assert_eq!(shifted[0].0.y, one[0].0.y);
    assert!((shifted[0].1[1] - 1229.0 / 2541.0).abs() < 1e-6, "верх полосы = фаза");
    assert!(
        (shifted[0].2[1] - shifted[0].1[1] - 1.0).abs() < 1e-6,
        "диапазон uv обязан остаться ровно в одну высоту текстуры",
    );
    // По горизонтали кольца нет — u остаётся 0…1 при любой фазе.
    assert_eq!((shifted[0].1[0], shifted[0].2[0]), (0.0, 1.0));
}

/// BUG-405 срез 30: гейт на разбор рычага переписи [`band_pass_load_ops`]
/// → [`band_pass_load_choice`].
///
/// Опасная точка рычага — не арифметика, а МОЛЧАЛИВОЕ плечо: опечатка в
/// имени («colour», пустая строка, `1`) не должна снимать ни одного клира,
/// иначе свип сравнит штатный путь со штатным путём и напечатает разницу
/// как эффект. Поэтому неизвестное значение = штатный путь, а плечи
/// перечислены поимённо.
#[test]
fn band_pass_load_choice_only_names_known_arms() {
    assert_eq!(band_pass_load_choice("color"), (true, false));
    assert_eq!(band_pass_load_choice("depth"), (false, true));
    assert_eq!(band_pass_load_choice("both"), (true, true));
    // Пробелы вокруг имени плеча — не опечатка (env из скрипта).
    assert_eq!(band_pass_load_choice(" both "), (true, true));
    // Всё остальное — штатный путь, оба клира на месте.
    for v in ["", "1", "colour", "none", "color,depth"] {
        assert_eq!(band_pass_load_choice(v), (false, false), "плечо {v:?}");
    }
}

/// BUG-405 срез 23: сторона текстуры запрашивается по тиру адаптера, и
/// именно она решает, работает ли композитор на высоком окне.
///
/// Гейт на арифметику [`requested_max_texture_dim`] плюс её связка с
/// [`band_geometry`]: с прежними 2048 окно клиентской высотой 1401 px
/// (развёрнутое 1080p с dpr 1) композитора не получало вовсе — перепись
/// среза даёт на нём 0 Compose-кадров и `page-compose skip: вьюпорт не
/// оставляет запаса в лимите текстуры`.
#[test]
fn requested_texture_dim_follows_adapter() {
    let downlevel = wgpu::Limits::downlevel_defaults().max_texture_dimension_2d;

    // Обычный дискретный/интегрированный адаптер: берём цель, не больше.
    assert_eq!(requested_max_texture_dim(16384, true), MAX_TEXTURE_DIM_TARGET);
    assert_eq!(requested_max_texture_dim(8192, true), 8192);
    // Адаптер беднее цели — просим ровно его потолок, иначе
    // `request_device` вернул бы ошибку и окно не открылось.
    assert_eq!(requested_max_texture_dim(4096, true), 4096);
    // Ниже downlevel не опускаемся: такой адаптер не потянул бы и запрос
    // до среза, поведение в этом углу не меняется.
    assert_eq!(requested_max_texture_dim(1024, true), downlevel);
    // Плечо отката обязано повторять поведение до среза при любом адаптере.
    assert_eq!(requested_max_texture_dim(16384, false), downlevel);

    // Связка с полосой: 1024×1401 (развёрнутое 1080p) — отказ на 2048 и
    // полный запас на поднятом лимите.
    assert_eq!(
        band_geometry(
            1024,
            1401,
            1.0,
            requested_max_texture_dim(16384, false),
            true,
            None,
        ),
        Err("вьюпорт не оставляет запаса в лимите текстуры"),
        "плечо отката перестало воспроизводить потерю композитора",
    );
    let (m, h) = band_geometry(
        1024,
        1401,
        1.0,
        requested_max_texture_dim(16384, true),
        true,
        None,
    )
    .expect("на поднятом лимите полоса обязана быть");
    // Запас упирается в потолок 768 CSS px, а не в лимит текстуры.
    assert_eq!((m, h), (768, 2937), "полоса ужата там, где лимит уже не жмёт");
}

/// BUG-405 срез 28: перепись цены КОПИИ полосы GPU→GPU (пункт 43 остатка).
///
/// Это инструмент переписи, а не гейт: он печатает таблицу и проверяет
/// только осмысленность собственных чисел. Вопрос, ради которого написан:
/// инкрементальная дорисовка полосы (не перерисовывать всю полосу на
/// промахе, а сдвинуть её и дорисовать вышедшую кромку) уже строилась
/// 2026-07-13 и была откачена — диагноз того среза называет убийцей не
/// сэкономленный fill, а саму `copy_texture_to_texture` перекрытия. Тот
/// замер снят на полосе ~1890 px (3.9 МБ копии); на развёрнутом окне
/// полоса 1920×2541, а копия перекрытия — 1920×1322, то есть 10.2 МБ.
/// Прежде чем строить путь заново, нужна цена этой копии на этом
/// устройстве.
///
/// Метод. Меряется ТОЛЬКО `submit` + ожидание GPU (`PollType::Wait`);
/// запись команд в encoder за скобками, потому что правку интересует
/// работа устройства, а не наша. Цена одной копии берётся как НАКЛОН по
/// числу копий в одном submit-е (`(t(8) − t(1)) / 7`), а не как `t(1)`:
/// так из числа уходит пол пола — цена самого submit-а и барьера, — и
/// остаётся то, что платит именно копия. Пол печатается отдельной строкой
/// (пустой encoder), чтобы было видно, велика ли поправка. Первое
/// обращение к свежей текстуре одноразово дорого (п. 40 остатка), поэтому
/// перед замером каждая пара текстур прогревается.
///
/// Второе плечо — налог за право копировать: те же биты
/// `COPY_SRC | COPY_DST`, что нужны правке, на многих драйверах отключают
/// сжатие цели без потерь, то есть могут удорожить сам промах. Здесь он
/// меряется прокси-работой (пасс с `Clear` по всей полосе — та же запись
/// во всю площадь, что и на промахе, но без нашей геометрии) в интерливед
/// A/B; живой ответ на том же вопросе даёт рычаг
/// `LUMEN_BAND_COPY_USAGE=1` со `scripts/band_miss_census.py`.
///
/// Требует GPU-адаптер, поэтому `#[ignore]`. Бэкенд задавать обязательно
/// (числа DX12 и Vulkan несопоставимы, срез 14):
/// `WGPU_BACKEND=vulkan cargo test -p lumen-paint --features backend-wgpu
///  band_copy_cost_census -- --include-ignored --nocapture`.
#[test]
#[ignore = "requires GPU adapter"]
fn band_copy_cost_census() {
    /// Медиана выборки.
    fn med(mut v: Vec<f64>) -> f64 {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    }
    /// Один замер: `reps` записей в один encoder, затем submit и ожидание.
    fn shot(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        reps: usize,
        mut rec: impl FnMut(&mut wgpu::CommandEncoder),
    ) -> f64 {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("band-copy-census"),
        });
        for _ in 0..reps {
            rec(&mut enc);
        }
        let t0 = std::time::Instant::now();
        queue.submit(Some(enc.finish()));
        let _ = device.poll(wgpu::PollType::Wait);
        t0.elapsed().as_secs_f64() * 1000.0
    }

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default().with_env());
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        println!("нет GPU-адаптера — перепись пропущена");
        return;
    };
    let info = adapter.get_info();
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_texture_dimension_2d = adapter
        .limits()
        .max_texture_dimension_2d
        .min(MAX_TEXTURE_DIM_TARGET);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("band-copy-census"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("устройство переписи");
    println!(
        "\nадаптер: {} ({:?}, {:?}), сторона текстуры {}",
        info.name,
        info.device_type,
        info.backend,
        device.limits().max_texture_dimension_2d,
    );

    // Формат цели — как у поверхности окна на Windows; на цену копии
    // влияет только через 4 байта на пиксель.
    const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
    let make = |w: u32, h: u32, copyable: bool| {
        let mut usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        if copyable {
            usage |= wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        }
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("band-census-tex"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage,
            view_formats: &[],
        })
    };

    const SAMPLES: usize = 9;
    // Пол: пустой submit с ожиданием. Всё, что ниже, — над ним.
    let floor = med((0..SAMPLES).map(|_| shot(&device, &queue, 0, |_| {})).collect());
    println!("пол (пустой submit + ожидание): {floor:.3} мс");

    // Точки свипа: ширина, высота полосы, высота копии, что это такое.
    // 1920×2541 — полоса развёрнутого окна 1920×1017 (срез 27); 1322 —
    // перекрытие, остающееся после хода 1219 px до промаха; 1219 — кромка,
    // которую пришлось бы дорисовать. 1024×1890/983 — масштаб замера
    // 2026-07-13, ради сопоставимости с его отрицательным результатом.
    let points: [(u32, u32, u32, &str); 5] = [
        (1920, 2541, 2541, "вся полоса развёрнутого окна"),
        (1920, 2541, 1322, "перекрытие после хода 1219 px"),
        (1920, 2541, 1219, "кромка (то, что дорисовывалось бы)"),
        (1024, 1890, 983, "перекрытие на стенде 2026-07-13"),
        (1024, 1890, 1890, "вся полоса стенда 2026-07-13"),
    ];
    println!(
        "\n{:<38} {:>10} {:>8} {:>9} {:>9} {:>9}",
        "копия", "МБ", "t(1),мс", "t(8),мс", "цена,мс", "ГБ/с",
    );
    let mut copy_ms: Vec<(u32, f64)> = Vec::new();
    for (w, band_h, copy_h, what) in points {
        if band_h > device.limits().max_texture_dimension_2d {
            println!("{what}: полоса выше лимита текстуры — точка пропущена");
            continue;
        }
        let src = make(w, band_h, true);
        let dst = make(w, band_h, true);
        let copy = |enc: &mut wgpu::CommandEncoder| {
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &src,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: band_h - copy_h, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &dst,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: w, height: copy_h, depth_or_array_layers: 1 },
            );
        };
        // Прогрев: первое обращение к свежей текстуре одноразово дорого.
        for _ in 0..3 {
            shot(&device, &queue, 1, copy);
        }
        let t1 = med((0..SAMPLES).map(|_| shot(&device, &queue, 1, copy)).collect());
        let t8 = med((0..SAMPLES).map(|_| shot(&device, &queue, 8, copy)).collect());
        let each = (t8 - t1) / 7.0;
        let mb = f64::from(w) * f64::from(copy_h) * 4.0 / 1024.0 / 1024.0;
        println!(
            "{:<38} {mb:>10.1} {t1:>8.3} {t8:>9.3} {each:>9.3} {:>9.2}",
            format!("{w}x{copy_h} ({what})"),
            mb / 1024.0 / each * 1000.0,
        );
        copy_ms.push((copy_h, each));
        assert!(each.is_finite() && each > 0.0, "цена копии не измерена: {each}");
    }

    // Налог за право копировать: пасс `Clear` по всей полосе на текстуре
    // со штатными битами usage против текстуры с COPY_SRC|COPY_DST.
    // Интерливед A/B, сравнение по МИНИМУМУ (`docs/perf-method.md`).
    let plain = make(1920, 2541, false);
    let copyable = make(1920, 2541, true);
    let clear = |tex: &wgpu::Texture, enc: &mut wgpu::CommandEncoder| {
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("band-census-clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    };
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for i in 0..(SAMPLES * 2) {
        let plain_ms = shot(&device, &queue, 4, |e| clear(&plain, e));
        let copy_ms_ = shot(&device, &queue, 4, |e| clear(&copyable, e));
        if i >= 3 {
            // первые заходы — прогрев обеих целей (п. 40 остатка)
            a.push(plain_ms);
            b.push(copy_ms_);
        }
    }
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    println!(
        "\nналог за COPY_SRC|COPY_DST (4 пасса Clear по 1920x2541, {} пар):\n  \
         штатные биты: min {:.3} p50 {:.3} мс\n  \
         с копией:     min {:.3} p50 {:.3} мс",
        a.len(),
        min(&a),
        med(a.clone()),
        min(&b),
        med(b.clone()),
    );
}

/// Побитовое равенство двух наборов вершин текста.
fn bits_eq(a: &[TextVertex], b: &[TextVertex]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.pos[0].to_bits() == y.pos[0].to_bits()
                && x.pos[1].to_bits() == y.pos[1].to_bits()
                && x.z.to_bits() == y.z.to_bits()
                && x.uv[0].to_bits() == y.uv[0].to_bits()
                && x.uv[1].to_bits() == y.uv[1].to_bits()
                && x.color.iter().zip(&y.color).all(|(p, q)| p.to_bits() == q.to_bits())
        })
}
