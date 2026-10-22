//! Разбор `<basic-shape>` и свойства `clip-path` (CSS Shapes L1 §3,
//! CSS Masking L1 §4.5): `inset()`/`circle()`/`ellipse()`/`polygon()`/`path()`
//! и `<geometry-box>`.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_shape_value` … `fn parse_at_pair`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::style::parse::transform::parse_length_px;
use crate::style::{ClipPath, FillRule, GeometryBox, ShapeValue};

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
/// Поддерживает: `inset(t r b l)`, `rect()`/`xywh()` (сводятся к `inset()`), `circle(r at cx cy)`,
/// `ellipse(rx ry at cx cy)`, `polygon(x1 y1, x2 y2, ...)`,
/// `path([<fill-rule>,]? "<svg>")` (CSS Shapes L1 §4).
pub(in crate::style) fn parse_clip_path(s: &str) -> Option<ClipPath> {
    // `[<basic-shape> || <geometry-box>]` (CSS Masking L1 §3): бокс стоит до или после формы.
    let mut geometry = None;
    let mut shape = None;
    for tok in split_top_level(s.trim()) {
        if let Some(g) = parse_geometry_box(tok) {
            if geometry.replace(g).is_some() {
                return None;
            }
        } else if shape.replace(tok).is_some() {
            return None;
        }
    }
    match (shape, geometry) {
        (None, None) => None,
        (None, Some(geometry)) => Some(ClipPath::Boxed { shape: None, geometry }),
        (Some(shape), None | Some(GeometryBox::BorderBox)) => parse_basic_shape(shape),
        (Some(shape), Some(geometry)) => match parse_basic_shape(shape)? {
            ClipPath::Url(_) => None,
            shape => Some(ClipPath::Boxed { shape: Some(Box::new(shape)), geometry }),
        },
    }
}

/// Токены верхнего уровня, разделённые пробелами вне скобок.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

fn parse_geometry_box(tok: &str) -> Option<GeometryBox> {
    Some(match tok.to_ascii_lowercase().as_str() {
        "border-box" => GeometryBox::BorderBox,
        "padding-box" => GeometryBox::PaddingBox,
        "content-box" => GeometryBox::ContentBox,
        "margin-box" => GeometryBox::MarginBox,
        "fill-box" => GeometryBox::FillBox,
        "stroke-box" => GeometryBox::StrokeBox,
        "view-box" => GeometryBox::ViewBox,
        _ => return None,
    })
}

/// Радиус `circle()`/`ellipse()`: `closest-side | farthest-side | <length-percentage [0,∞)>`.
fn parse_radius(tok: &str) -> Option<ShapeValue> {
    let v = match tok.to_ascii_lowercase().as_str() {
        "closest-side" => ShapeValue::ClosestSide,
        "farthest-side" => ShapeValue::FarthestSide,
        _ => parse_shape_value(tok)?,
    };
    match v {
        ShapeValue::Px(n) | ShapeValue::Pct(n) if n < 0.0 => None,
        v => Some(v),
    }
}

/// Делит аргументы формы на токены до и после ключевого слова `at`.
fn split_at_keyword(inner: &str) -> (Vec<&str>, Option<Vec<&str>>) {
    let toks: Vec<&str> = inner.split_whitespace().collect();
    match toks.iter().position(|t| t.eq_ignore_ascii_case("at")) {
        Some(i) => (toks[..i].to_vec(), Some(toks[i + 1..].to_vec())),
        None => (toks, None),
    }
}

/// 1..=4 `<length-percentage>` подряд.
fn shape_values(toks: &[&str]) -> Option<Vec<ShapeValue>> {
    let v: Option<Vec<ShapeValue>> = toks.iter().map(|t| parse_shape_value(t)).collect();
    v.filter(|v| (1..=4).contains(&v.len()))
}

/// `<border-radius>` после `round`: `<radii>{1,4} [/ <radii>{1,4}]?` → (rx, ry);
/// `ry` пуст, если `/` не было.
fn parse_round_radii(radii: &[&str]) -> Option<(Vec<ShapeValue>, Vec<ShapeValue>)> {
    Some(match radii.iter().position(|t| *t == "/") {
        Some(j) => (shape_values(&radii[..j])?, shape_values(&radii[j + 1..])?),
        None => (shape_values(radii)?, Vec::new()),
    })
}

/// `inset(…)`: `<sides>{1,4} [round <radii>{1,4} [/ <radii>{1,4}]]?`.
fn parse_inset(inner: &str) -> Option<ClipPath> {
    let toks: Vec<&str> = inner.split_whitespace().collect();
    let round = toks.iter().position(|t| t.eq_ignore_ascii_case("round"));
    let sides = shape_values(&toks[..round.unwrap_or(toks.len())])?;
    let Some(i) = round else { return Some(ClipPath::Inset(sides)) };
    let (rx, ry) = parse_round_radii(&toks[i + 1..])?;
    Some(ClipPath::InsetRound { sides, rx, ry })
}

/// Отступ от дальней стороны для края, заданного позицией `parts` от ближней:
/// `100% − Σ parts`. Чистый px или чистый процент остаются простыми значениями.
fn far_inset(parts: &[ShapeValue]) -> ShapeValue {
    let (mut px, mut pct) = (0.0, 100.0);
    for p in parts {
        match *p {
            ShapeValue::Px(v) => px -= v,
            ShapeValue::Pct(v) => pct -= v,
            ShapeValue::Lin { px: a, pct: b } => {
                px -= a;
                pct -= b;
            }
            ShapeValue::ClosestSide | ShapeValue::FarthestSide => {}
        }
    }
    if px == 0.0 { ShapeValue::Pct(pct) } else { ShapeValue::Lin { px, pct } }
}

/// `rect(<top> <right> <bottom> <left> [round …])` и `xywh(<x> <y> <w> <h> [round …])`
/// (CSS Shapes L1 §3.2–3.3) сводятся к `inset()`: `rect` задаёт края расстояниями от
/// верхнего/левого края reference box (`auto` — сам край), `xywh` — позицией и размером;
/// отступ от правой/нижней стороны — `100% − …` ([`far_inset`]).
fn parse_rect_xywh(func: &str, inner: &str) -> Option<ClipPath> {
    let toks: Vec<&str> = inner.split_whitespace().collect();
    let round = toks.iter().position(|t| t.eq_ignore_ascii_case("round"));
    let args = &toks[..round.unwrap_or(toks.len())];
    let [a, b, c, d] = args else { return None };
    let zero = ShapeValue::Px(0.0);
    let sides = if func == "rect" {
        let edge = |t: &&str| -> Option<Option<ShapeValue>> {
            if t.eq_ignore_ascii_case("auto") { Some(None) } else { parse_shape_value(t).map(Some) }
        };
        let (top, right, bottom, left) = (edge(a)?, edge(b)?, edge(c)?, edge(d)?);
        vec![
            top.unwrap_or(zero),
            right.map_or(zero, |r| far_inset(&[r])),
            bottom.map_or(zero, |v| far_inset(&[v])),
            left.unwrap_or(zero),
        ]
    } else {
        let (x, y) = (parse_shape_value(a)?, parse_shape_value(b)?);
        let (w, h) = (parse_shape_value(c)?, parse_shape_value(d)?);
        let negative = |v: ShapeValue| matches!(v, ShapeValue::Px(n) | ShapeValue::Pct(n) if n < 0.0);
        if negative(w) || negative(h) {
            return None;
        }
        vec![y, far_inset(&[x, w]), far_inset(&[y, h]), x]
    };
    let Some(i) = round else { return Some(ClipPath::Inset(sides)) };
    let (rx, ry) = parse_round_radii(&toks[i + 1..])?;
    Some(ClipPath::InsetRound { sides, rx, ry })
}

/// `at <position>`: 1–2 значения, ключевые слова `left|center|right|top|bottom`.
/// Трёх- и четырёхзначная форма (`right 10px bottom 20px`) не поддержана.
fn parse_position(toks: &[&str]) -> Option<(ShapeValue, ShapeValue)> {
    let kw = |t: &str| match t.to_ascii_lowercase().as_str() {
        "left" | "top" => Some(ShapeValue::Pct(0.0)),
        "right" | "bottom" => Some(ShapeValue::Pct(100.0)),
        "center" => Some(ShapeValue::Pct(50.0)),
        _ => None,
    };
    let val = |t: &str| kw(t).or_else(|| parse_shape_value(t));
    let center = ShapeValue::Pct(50.0);
    let is = |t: &str, names: &[&str]| names.iter().any(|n| t.eq_ignore_ascii_case(n));
    match toks {
        [a] if is(a, &["top", "bottom"]) => Some((center, val(a)?)),
        [a] => Some((val(a)?, center)),
        [a, b] if is(a, &["top", "bottom"]) || is(b, &["left", "right"]) => Some((val(b)?, val(a)?)),
        [a, b] => Some((val(a)?, val(b)?)),
        _ => None,
    }
}

fn parse_basic_shape(s: &str) -> Option<ClipPath> {
    let s = s.trim();
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close <= open {
        return None;
    }
    let func = s[..open].trim().to_ascii_lowercase();
    let inner = s[open + 1..close].trim();
    match func.as_str() {
        // CSS Masking L1 §3.1 — `url(#id)`: ссылка на `<clipPath>` того же
        // документа (внешние `file.svg#id` не поддерживаются — не заданы).
        "url" => {
            let target = inner.trim_matches(|c| c == '"' || c == '\'').trim();
            let id = target.strip_prefix('#')?;
            if id.is_empty() {
                None
            } else {
                Some(ClipPath::Url(id.to_string()))
            }
        }
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
        "inset" => parse_inset(inner),
        "rect" | "xywh" => parse_rect_xywh(&func, inner),
        "circle" => {
            // `[<radius>]? [at <position>]?`; без радиуса — `closest-side`.
            let (radii, at) = split_at_keyword(inner);
            let radius = match radii.as_slice() {
                [] => ShapeValue::ClosestSide,
                [r] => parse_radius(r)?,
                _ => return None,
            };
            let center = match at {
                Some(t) => Some(parse_position(&t)?),
                None => None,
            };
            Some(ClipPath::Circle { radius, center })
        }
        "ellipse" => {
            let (radii, at) = split_at_keyword(inner);
            let (rx, ry) = match radii.as_slice() {
                [] => (ShapeValue::ClosestSide, ShapeValue::ClosestSide),
                [rx, ry] => (parse_radius(rx)?, parse_radius(ry)?),
                _ => return None,
            };
            let center = match at {
                Some(t) => Some(parse_position(&t)?),
                None => None,
            };
            Some(ClipPath::Ellipse { rx, ry, center })
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
