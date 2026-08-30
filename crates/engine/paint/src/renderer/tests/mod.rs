use super::*;

/// BUG-405 срез 24: квад H-прохода в bbox-офскрин обязан читать источник
/// по ТЕМ ЖЕ текселям, что полноразмерный проход, — иначе картинка
/// поедет на пиксель. Проверка на центрах фрагментов: интерполяция UV по
/// офскрину шириной `rw` в точке `t = (i + 0.5) / rw` должна давать
/// `(rx + i + 0.5) / surf`.
#[test]
fn region_src_quad_uv_matches_full_size_pass() {
    let (surf_w, surf_h) = (1884.0_f32, 2501.0_f32);
    let region = [640_u32, 1216, 320, 384];
    let mut out = Vec::new();
    push_region_src_quad(&mut out, region, surf_w, surf_h, 1.0);
    assert_eq!(out.len(), 6);
    // Позиция — весь офскрин: NDC от −1 до 1 по обеим осям.
    assert_eq!(out[0].pos, [-1.0, 1.0]);
    assert_eq!(out[2].pos, [1.0, -1.0]);
    let (u0, u1) = (out[0].uv[0], out[1].uv[0]);
    let (v0, v1) = (out[0].uv[1], out[5].uv[1]);
    let (rx, ry, rw, rh) = (640.0_f32, 1216.0, 320.0, 384.0);
    for i in [0.0_f32, 1.0, 159.0, 319.0] {
        let t = (i + 0.5) / rw;
        let want = (rx + i + 0.5) / surf_w;
        assert!((u0 + (u1 - u0) * t - want).abs() < 1e-6, "u на фрагменте {i}");
    }
    for j in [0.0_f32, 1.0, 383.0] {
        let t = (j + 0.5) / rh;
        let want = (ry + j + 0.5) / surf_h;
        assert!((v0 + (v1 - v0) * t - want).abs() < 1e-6, "v на фрагменте {j}");
    }
}

/// Слитый пасс кроет в цели ровно прямоугольник региона, а UV пробегает
/// офскрин целиком (0..1): смещение сидит в позиции, а не в выборке.
#[test]
fn region_dst_quad_covers_region_rect() {
    let (surf_w, surf_h) = (1884.0_f32, 2501.0_f32);
    let mut out = Vec::new();
    push_region_dst_quad(&mut out, [640, 1216, 320, 384], surf_w, surf_h, 1.0);
    assert_eq!(out.len(), 6);
    let ndc_x = |px: f32| px / surf_w * 2.0 - 1.0;
    let ndc_y = |px: f32| 1.0 - px / surf_h * 2.0;
    assert_eq!(out[0].pos, [ndc_x(640.0), ndc_y(1216.0)]);
    assert_eq!(out[2].pos, [ndc_x(960.0), ndc_y(1600.0)]);
    assert_eq!(out[0].uv, [0.0, 0.0]);
    assert_eq!(out[2].uv, [1.0, 1.0]);
}

#[test]
fn size_bin_for_exact_match() {
    // Точное совпадение — bin == входу.
    for &bin in &SIZE_BINS {
        assert_eq!(size_bin_for(f32::from(bin)), bin, "bin {bin}");
    }
}

#[test]
fn size_bin_for_rounds_up_to_next_bin() {
    // 9 → 12, 13 → 16, 17 → 20, 25 → 32, 33 → 48.
    assert_eq!(size_bin_for(9.0), 12);
    assert_eq!(size_bin_for(13.0), 16);
    assert_eq!(size_bin_for(17.0), 20);
    assert_eq!(size_bin_for(25.0), 32);
    assert_eq!(size_bin_for(33.0), 48);
    // Дробные: 13.5 → 16 (ceil 14 → bin 16).
    assert_eq!(size_bin_for(13.5), 16);
}

#[test]
fn size_bin_for_below_min_clamps_to_min() {
    // < 8 — bin 8 (нечитаемо иначе).
    assert_eq!(size_bin_for(1.0), 8);
    assert_eq!(size_bin_for(7.0), 8);
    assert_eq!(size_bin_for(0.5), 8);
}

#[test]
fn rotate_text_vertices_cw_maps_horizontal_run_into_vertical_column() {
    // Ph3 writing-mode vertical, Срез 2: a horizontal run laid out at the
    // local origin (0,0)..(40,10) — a wide, short glyph quad — must land
    // as a tall, narrow quad once rotated 90° CW onto dest (100, 50).
    let dest = Rect::new(100.0, 50.0, 10.0, 40.0);
    let mut verts = [
        TextVertex { pos: [0.0, 0.0], z: 0.0, uv: [0.0, 0.0], color: [0.0; 4] },
        TextVertex { pos: [40.0, 0.0], z: 0.0, uv: [1.0, 0.0], color: [0.0; 4] },
        TextVertex { pos: [40.0, 10.0], z: 0.0, uv: [1.0, 1.0], color: [0.0; 4] },
        TextVertex { pos: [0.0, 10.0], z: 0.0, uv: [0.0, 1.0], color: [0.0; 4] },
    ];
    rotate_text_vertices_cw(&mut verts, dest);
    // (0,0) -> (-0 + 100, 0 + 50) = (100, 50): local origin lands on dest origin.
    assert_eq!(verts[0].pos, [100.0, 50.0]);
    // (40,0) -> (0 + 100, 40 + 50) = (100, 90): local width becomes vertical extent.
    assert_eq!(verts[1].pos, [100.0, 90.0]);
    // (40,10) -> (-10 + 100, 40 + 50) = (90, 90).
    assert_eq!(verts[2].pos, [90.0, 90.0]);
    // (0,10) -> (-10 + 100, 0 + 50) = (90, 50): local height becomes horizontal extent.
    assert_eq!(verts[3].pos, [90.0, 50.0]);
    // UV/color untouched — only screen position rotates.
    assert_eq!(verts[0].uv, [0.0, 0.0]);
}

#[test]
fn size_bin_for_above_max_clamps_to_max() {
    // > 64 — bin 64 (с up-scaling-ом для редких headline-ов).
    assert_eq!(size_bin_for(72.0), 64);
    assert_eq!(size_bin_for(120.0), 64);
    assert_eq!(size_bin_for(1000.0), 64);
}

#[test]
fn size_bin_for_invalid_returns_min() {
    // NaN / negative / 0 → bin 8 (минимум, без panic).
    assert_eq!(size_bin_for(f32::NAN), 8);
    assert_eq!(size_bin_for(-1.0), 8);
    assert_eq!(size_bin_for(0.0), 8);
    assert_eq!(size_bin_for(f32::INFINITY), 64);
}

#[test]
fn atlas_key_distinguishes_size_bins() {
    // Один и тот же глиф на двух размерах = два разных ключа.
    let k16 = atlas_key(0, 42, 16, 0);
    let k32 = atlas_key(0, 42, 32, 0);
    assert_ne!(k16, k32);
}

#[test]
fn atlas_key_distinguishes_glyph_ids() {
    let k_a = atlas_key(0, 100, 16, 0);
    let k_b = atlas_key(0, 200, 16, 0);
    assert_ne!(k_a, k_b);
}

#[test]
fn atlas_key_distinguishes_face_ids() {
    let k0 = atlas_key(0, 42, 16, 0);
    let k1 = atlas_key(1, 42, 16, 0);
    assert_ne!(k0, k1);
}

#[test]
fn atlas_key_distinguishes_variation_coords_hashes() {
    // Тот же (face, glyph, size), но разные normalized coords ⇒ разные
    // ключи. Без этого variant glyph перезаписывал бы default-instance
    // в atlas-кеше.
    let k_default = atlas_key(0, 42, 16, 0);
    let k_bold = atlas_key(0, 42, 16, 0xdead_beef_cafe_babe);
    assert_ne!(k_default, k_bold);
}

#[test]
fn atlas_key_is_deterministic() {
    assert_eq!(atlas_key(3, 17, 24, 0), atlas_key(3, 17, 24, 0));
    assert_eq!(atlas_key(3, 17, 24, 42), atlas_key(3, 17, 24, 42));
}

mod clip_blend_vertex;
mod sticky_colr_font;
