//! Разбор `<basic-shape>` и свойства `clip-path` (CSS Shapes L1 §3,
//! CSS Masking L1 §4.5): `inset()`/`circle()`/`ellipse()`/`polygon()`/`path()`
//! и `<geometry-box>`.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_shape_value` … `fn parse_at_pair`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::style::parse::transform::parse_length_px;
use crate::style::{ClipPath, FillRule, ShapeValue};

/// Распарсить `<length-percentage>` координату basic-shape: `%` →
/// `ShapeValue::Pct`, остальное через `parse_length_px` → `ShapeValue::Px`.
fn parse_shape_value(s: &str) -> Option<ShapeValue> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('%') {
        return num.trim().parse::<f32>().ok().map(ShapeValue::Pct);
    }
    parse_length_px(s).map(ShapeValue::Px)
}

/// Парсит `<basic-shape>` для `clip-path` (CSS Masking L1 §3.5).
/// Поддерживает: `inset(t r b l)`, `circle(r at cx cy)`,
/// `ellipse(rx ry at cx cy)`, `polygon(x1 y1, x2 y2, ...)`,
/// `path([<fill-rule>,]? "<svg>")` (CSS Shapes L1 §4).
pub(in crate::style) fn parse_clip_path(s: &str) -> Option<ClipPath> {
    let s = s.trim();
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    let func = s[..open].trim().to_ascii_lowercase();
    let inner = s[open + 1..close].trim();
    match func.as_str() {
        "path" => {
            // `path([<fill-rule>,]? "<svg-path>")`. Опциональный fill-rule
            // (nonzero|evenodd) управляет заливкой самопересекающихся путей
            // (CSS Shapes L1 §4). Строка пути — в кавычках.
            let mut fill_rule = FillRule::NonZero;
            let inner = match inner.split_once(',') {
                Some((head, rest)) if matches!(head.trim(), "nonzero" | "evenodd") => {
                    if head.trim().eq_ignore_ascii_case("evenodd") {
                        fill_rule = FillRule::EvenOdd;
                    }
                    rest.trim()
                }
                _ => inner,
            };
            let path_str = inner
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .or_else(|| inner.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))?;
            let pts = crate::motion_path::flatten_path_to_polygon(path_str);
            if pts.len() < 3 {
                None
            } else {
                Some(ClipPath::Path(pts, fill_rule))
            }
        }
        "inset" => {
            let parts: Vec<ShapeValue> = inner
                .split_whitespace()
                .filter_map(parse_shape_value)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(ClipPath::Inset(parts))
            }
        }
        "circle" => {
            // `radius` [`at cx cy`]
            let (radius_part, at_part) = if let Some(idx) = inner.find(" at ") {
                (&inner[..idx], Some(&inner[idx + 4..]))
            } else {
                (inner, None)
            };
            let radius = parse_shape_value(radius_part.trim())?;
            let center = at_part.and_then(parse_at_pair);
            Some(ClipPath::Circle { radius, center })
        }
        "ellipse" => {
            let (radii_part, at_part) = if let Some(idx) = inner.find(" at ") {
                (&inner[..idx], Some(&inner[idx + 4..]))
            } else {
                (inner, None)
            };
            let radii: Vec<ShapeValue> = radii_part
                .split_whitespace()
                .filter_map(parse_shape_value)
                .collect();
            if radii.len() < 2 {
                return None;
            }
            let center = at_part.and_then(parse_at_pair);
            Some(ClipPath::Ellipse {
                rx: radii[0],
                ry: radii[1],
                center,
            })
        }
        "polygon" => {
            // `polygon([<fill-rule>,]? x1 y1, ...)`. Опциональный fill-rule —
            // первый токен перед первой запятой (CSS Shapes L1 §3).
            let mut fill_rule = FillRule::NonZero;
            let mut inner = inner;
            if let Some((head, rest)) = inner.split_once(',') {
                let head = head.trim();
                if head.eq_ignore_ascii_case("nonzero") || head.eq_ignore_ascii_case("evenodd") {
                    if head.eq_ignore_ascii_case("evenodd") {
                        fill_rule = FillRule::EvenOdd;
                    }
                    inner = rest.trim();
                }
            }
            let mut vertices = Vec::new();
            for pair in inner.split(',') {
                let coords: Vec<ShapeValue> = pair
                    .split_whitespace()
                    .filter_map(parse_shape_value)
                    .collect();
                if coords.len() >= 2 {
                    vertices.push((coords[0], coords[1]));
                }
            }
            if vertices.is_empty() {
                None
            } else {
                Some(ClipPath::Polygon(vertices, fill_rule))
            }
        }
        _ => None,
    }
}

fn parse_at_pair(s: &str) -> Option<(ShapeValue, ShapeValue)> {
    let parts: Vec<ShapeValue> = s.split_whitespace().filter_map(parse_shape_value).collect();
    if parts.len() >= 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}
