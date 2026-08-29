//! Разбор `transform` и `filter` (CSS Transforms L1/L2 §11, CSS Filter Effects
//! L1 §2) вместе с примитивами значений, которые они делят с остальными
//! парсерами: угол, `<number-percentage>` и `<length>` в пикселях.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_angle_to_radians` … `fn parse_length_px`, `fn parse_transform_list` … `fn parse_filter_list`, `fn parse_filter_fn`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::style::{FilterFn, TransformFn};

/// Парсит угол в радианах из строки вида `45deg`, `1.5rad`, `0.25turn`,
/// `100grad`. Без единицы — number-as-radians (для совместимости).
pub(in crate::style) fn parse_angle_to_radians(s: &str) -> Option<f32> {
    let s = s.trim();
    for (suffix, factor) in [
        ("deg", std::f32::consts::PI / 180.0),
        ("rad", 1.0),
        ("turn", std::f32::consts::TAU),
        ("grad", std::f32::consts::PI / 200.0),
    ] {
        if let Some(num) = s.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f32>()
        {
            return Some(v * factor);
        }
    }
    s.parse::<f32>().ok()
}

/// Парсит `<number>` или `<percentage>` для filter-функций.
/// Number 0..=1.0 (или %  0..=100%) — типичная семантика.
fn parse_number_or_percent(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        num.trim().parse::<f32>().ok().map(|v| v / 100.0)
    } else {
        s.parse::<f32>().ok()
    }
}

/// Распарсить `<length>` в px (без `%`). Поддерживает px/em/rem
/// упрощённо — em/rem трактуем как 16px-base; viewport-units игнорируем
/// (Phase 0 — clip-path/transform/filter не критичны к точному
/// разрешению относительных длин на этапе parsing).
pub(in crate::style) fn parse_length_px(s: &str) -> Option<f32> {
    let s = s.trim();
    for (suffix, factor) in [("px", 1.0), ("em", 16.0), ("rem", 16.0)] {
        if let Some(num) = s.strip_suffix(suffix)
            && let Ok(v) = num.trim().parse::<f32>()
        {
            return Some(v * factor);
        }
    }
    // Без единицы — допустимо для 0.
    s.parse::<f32>().ok()
}

/// Парсит `<transform-list>` — последовательность `func(args)` через
/// whitespace (без запятых). Каждая `func` распознаётся отдельно.
pub fn parse_transform_list(s: &str) -> Vec<TransformFn> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Read ident до `(`.
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = s[name_start..i].trim().to_ascii_lowercase();
        if name.is_empty() {
            break;
        }
        // Expect `(`.
        if i >= bytes.len() || bytes[i] != b'(' {
            break;
        }
        i += 1;
        // Find matching `)`.
        let args_start = i;
        let mut depth = 1usize;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let args = &s[args_start..i.saturating_sub(1)];
        if let Some(tf) = parse_transform_fn(&name, args) {
            out.push(tf);
        }
    }
    out
}

fn parse_transform_fn(name: &str, args: &str) -> Option<TransformFn> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    match name {
        "translate" => {
            let x = parse_length_px(parts.first()?)?;
            let y = parts.get(1).and_then(|s| parse_length_px(s)).unwrap_or(0.0);
            Some(TransformFn::Translate(x, y))
        }
        "translatex" => parse_length_px(parts.first()?).map(TransformFn::TranslateX),
        "translatey" => parse_length_px(parts.first()?).map(TransformFn::TranslateY),
        "translatez" => parse_length_px(parts.first()?).map(TransformFn::TranslateZ),
        "translate3d" => {
            let x = parse_length_px(parts.first()?)?;
            let y = parse_length_px(parts.get(1)?)?;
            let z = parts.get(2).and_then(|s| parse_length_px(s)).unwrap_or(0.0);
            Some(TransformFn::Translate3d(x, y, z))
        }
        "rotate" => parse_angle_to_radians(parts.first()?).map(TransformFn::Rotate),
        "rotatex" => parse_angle_to_radians(parts.first()?).map(TransformFn::RotateX),
        "rotatey" => parse_angle_to_radians(parts.first()?).map(TransformFn::RotateY),
        "rotatez" => parse_angle_to_radians(parts.first()?).map(TransformFn::RotateZ),
        "rotate3d" => {
            if parts.len() < 4 {
                return None;
            }
            let x = parts[0].parse::<f32>().ok()?;
            let y = parts[1].parse::<f32>().ok()?;
            let z = parts[2].parse::<f32>().ok()?;
            let angle = parse_angle_to_radians(parts[3])?;
            Some(TransformFn::Rotate3d(x, y, z, angle))
        }
        "scale" => {
            let x = parts.first()?.parse::<f32>().ok()?;
            let y = parts
                .get(1)
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(x);
            Some(TransformFn::Scale(x, y))
        }
        "scalex" => parts.first()?.parse::<f32>().ok().map(TransformFn::ScaleX),
        "scaley" => parts.first()?.parse::<f32>().ok().map(TransformFn::ScaleY),
        "scalez" => parts.first()?.parse::<f32>().ok().map(TransformFn::ScaleZ),
        "scale3d" => {
            let x = parts.first()?.parse::<f32>().ok()?;
            let y = parts.get(1)?.parse::<f32>().ok()?;
            let z = parts.get(2).and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
            Some(TransformFn::Scale3d(x, y, z))
        }
        "skewx" => parse_angle_to_radians(parts.first()?).map(TransformFn::SkewX),
        "skewy" => parse_angle_to_radians(parts.first()?).map(TransformFn::SkewY),
        "skew" => {
            // `skew(x, y)` — для совместимости. Phase 0: храним как X-only.
            parse_angle_to_radians(parts.first()?).map(TransformFn::SkewX)
        }
        "matrix" => {
            if parts.len() != 6 {
                return None;
            }
            let mut m = [0.0f32; 6];
            for (i, p) in parts.iter().enumerate() {
                m[i] = p.parse::<f32>().ok()?;
            }
            Some(TransformFn::Matrix(m))
        }
        "matrix3d" => {
            if parts.len() != 16 {
                return None;
            }
            let mut m = [0.0f32; 16];
            for (i, p) in parts.iter().enumerate() {
                m[i] = p.parse::<f32>().ok()?;
            }
            Some(TransformFn::Matrix3d(m))
        }
        "perspective" => {
            parse_length_px(parts.first()?).and_then(|px| {
                if px > 0.0 { Some(TransformFn::Perspective(px)) } else { None }
            })
        }
        _ => None,
    }
}

/// Парсит `<filter-function-list>` — последовательность функций
/// через whitespace.
pub(in crate::style) fn parse_filter_list(s: &str) -> Vec<FilterFn> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && bytes[i] != b'(' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = s[name_start..i].trim().to_ascii_lowercase();
        if name.is_empty() {
            break;
        }
        if i >= bytes.len() || bytes[i] != b'(' {
            break;
        }
        i += 1;
        let args_start = i;
        let mut depth = 1usize;
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let args = s[args_start..i.saturating_sub(1)].trim();
        if let Some(f) = parse_filter_fn(&name, args) {
            out.push(f);
        }
    }
    out
}

fn parse_filter_fn(name: &str, args: &str) -> Option<FilterFn> {
    match name {
        "blur" => parse_length_px(args).map(FilterFn::Blur),
        "brightness" => parse_number_or_percent(args).map(FilterFn::Brightness),
        "contrast" => parse_number_or_percent(args).map(FilterFn::Contrast),
        "grayscale" => parse_number_or_percent(args).map(FilterFn::Grayscale),
        "hue-rotate" => parse_angle_to_radians(args).map(FilterFn::HueRotate),
        "invert" => parse_number_or_percent(args).map(FilterFn::Invert),
        "opacity" => parse_number_or_percent(args).map(FilterFn::Opacity),
        "saturate" => parse_number_or_percent(args).map(FilterFn::Saturate),
        "sepia" => parse_number_or_percent(args).map(FilterFn::Sepia),
        _ => None,
    }
}
