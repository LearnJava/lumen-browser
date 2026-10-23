//! Гамма и контраст покрытия глифа (TEXTQ-2) — одна функция для GPU и CPU.
//!
//! Покрытие глифа смешивается в gamma-кодированном пространстве (поверхность
//! не-sRGB). Это делает тёмный текст на светлом толще, чем в линейном
//! смешивании, а светлый на тёмном — тоньше; DirectWrite и Skia поправляют
//! это таблицей предсмешивания по яркости цвета текста. Здесь — формула Skia
//! (`SkMaskGamma`, `SkTMaskGamma_build_correcting_lut`): фон считается
//! «противоположным» тексту (`dst = 1 − lum`), смешивание делается в линейном
//! пространстве с показателем [`GAMMA`], затем результат пересчитывается в
//! покрытие, которое при обычном смешивании даст тот же цвет. Перед этим
//! покрытие усиливается на [`CONTRAST`] (чем темнее текст, тем сильнее).
//!
//! Применяется в `fs_main` текстового шейдера (wgpu, WGSL из [`wgsl`]) и в
//! `cpu_raster::blit_glyph_coverage` (таблица [`coverage_lut`]) — формулы
//! обязаны совпадать, любая правка идёт в обе реализации сразу.
//!
//! `LUMEN_TEXT_GAMMA=0` отключает коррекцию (сравнение «до/после»).

use std::sync::OnceLock;

/// Показатель гаммы смешивания. Ориентир — DirectWrite по умолчанию (1.8);
/// подобрано 1.9 по толщине штрихов страницы `166-text-quality` против Edge:
/// суммарная «чернота» четырёх панелей (Arial/Segoe × чёрный/белый текст)
/// в пределах ±3 % от Edge (без коррекции — светлый текст на 15–16 % тоньше).
pub const GAMMA: f32 = 1.9;

/// Усиление контраста покрытия (Skia `contrast`, DirectWrite «enhanced
/// contrast»). Масштабируется линейной яркостью фона, поэтому светлый текст
/// на тёмном контраст не получает.
pub const CONTRAST: f32 = 1.0;

/// Включена ли коррекция (`LUMEN_TEXT_GAMMA=0` — выключена). Читается один раз.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("LUMEN_TEXT_GAMMA").map_or(true, |v| v.trim() != "0"))
}

/// Яркость цвета текста по Rec. 709 от кодированных sRGB-компонент (0..=1).
pub fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// `corrected = f(coverage, luminance)`: покрытие `cov` (0..=1) глифа цвета
/// яркости `lum` → покрытие для обычного смешивания в кодированном
/// пространстве. `f(0) = 0`, `f(1) = 1`, монотонна по `cov`.
pub fn correct_coverage(cov: f32, lum: f32) -> f32 {
    let cov = cov.clamp(0.0, 1.0);
    let src = lum.clamp(0.0, 1.0);
    let dst = 1.0 - src;
    let lin_src = src.powf(GAMMA);
    let lin_dst = dst.powf(GAMMA);
    let adjusted = CONTRAST * lin_dst;
    let a = cov + (1.0 - cov) * adjusted * cov;
    let denom = src - dst;
    if denom.abs() < 1e-2 {
        return a;
    }
    let lin_out = lin_src * a + (1.0 - a) * lin_dst;
    ((lin_out.powf(1.0 / GAMMA) - dst) / denom).clamp(0.0, 1.0)
}

/// Таблица коррекции 256 → 256 для цвета текста `(r, g, b)`. Тождество, когда
/// коррекция выключена.
pub fn coverage_lut(r: u8, g: u8, b: u8) -> [u8; 256] {
    let mut lut = [0u8; 256];
    if !enabled() {
        for (i, v) in lut.iter_mut().enumerate() {
            *v = i as u8;
        }
        return lut;
    }
    let lum = luminance(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    for (i, v) in lut.iter_mut().enumerate() {
        *v = (correct_coverage(i as f32 / 255.0, lum) * 255.0 + 0.5) as u8;
    }
    lut
}

/// WGSL-функция `text_gamma_correct(cov, color_rgb) -> f32` — та же формула,
/// константы подставляются из Rust. При `LUMEN_TEXT_GAMMA=0` — тождество.
pub fn wgsl() -> String {
    if !enabled() {
        return "fn text_gamma_correct(cov: f32, rgb: vec3<f32>) -> f32 { return cov; }\n".into();
    }
    format!(
        r#"
fn text_gamma_correct(cov: f32, rgb: vec3<f32>) -> f32 {{
    let g = {GAMMA:?};
    let src = clamp(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0);
    let dst = 1.0 - src;
    let lin_src = pow(src, g);
    let lin_dst = pow(dst, g);
    let a = cov + (1.0 - cov) * ({CONTRAST:?} * lin_dst) * cov;
    let denom = src - dst;
    if abs(denom) < 0.01 {{ return a; }}
    let lin_out = lin_src * a + (1.0 - a) * lin_dst;
    return clamp((pow(lin_out, 1.0 / g) - dst) / denom, 0.0, 1.0);
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_fixed_for_every_luminance() {
        for i in 0..=20 {
            let lum = i as f32 / 20.0;
            assert!(correct_coverage(0.0, lum).abs() < 1e-5, "f(0) lum={lum}");
            assert!((correct_coverage(1.0, lum) - 1.0).abs() < 1e-5, "f(1) lum={lum}");
        }
    }

    #[test]
    fn monotonic_in_coverage() {
        for i in 0..=20 {
            let lum = i as f32 / 20.0;
            let mut prev = -1.0;
            for c in 0..=255 {
                let v = correct_coverage(c as f32 / 255.0, lum);
                assert!(v >= prev - 1e-6, "lum={lum} c={c}: {v} < {prev}");
                prev = v;
            }
        }
    }

    #[test]
    fn black_text_gets_heavier() {
        assert!(correct_coverage(0.5, 0.0) > 0.5);
    }

    #[test]
    fn white_text_is_boosted_without_contrast() {
        // Светлый текст: фон тёмный, `lin_dst` = 0, контраст не применяется;
        // чистое линейное смешивание даёт `cov^(1/GAMMA)`.
        let f = correct_coverage(0.5, 1.0);
        assert!((f - 0.5f32.powf(1.0 / GAMMA)).abs() < 1e-4, "{f}");
    }

    #[test]
    fn lut_matches_function_and_is_monotonic() {
        let lut = coverage_lut(0, 0, 0);
        assert_eq!(lut[0], 0);
        assert_eq!(lut[255], 255);
        assert!(lut.windows(2).all(|w| w[0] <= w[1]));
        assert!(lut[128] > 128);
    }
}
