//! Легаси-градиенты Compat Standard §3 (`compat.spec.whatwg.org/#css-gradients`):
//! `-webkit-linear-gradient()`, `-webkit-radial-gradient()`,
//! `-webkit-repeating-linear-gradient()`, `-webkit-repeating-radial-gradient()`
//! и старый `-webkit-gradient(linear|radial, …, from(), to(), color-stop())`.
//!
//! Все формы приводятся к тем же [`ParsedGradient`], что и беспрефиксные
//! функции, поэтому отрисовка не знает о легаси-синтаксисе. Отличия от
//! стандартного синтаксиса, которые здесь нормализуются:
//!
//! * линейный: направление без `to` — это *начальная* сторона (`top` =
//!   `to bottom`), угол `0deg` смотрит на восток и растёт против часовой
//!   (`standard = 90° − legacy`); `to <side>` недопустим;
//! * радиальный: `[<position>,]? [<shape> || <size>,]?` без `at`, размер —
//!   либо extent-ключевое слово (`contain` = `closest-side`, `cover` =
//!   `farthest-corner`), либо радиусы `<length-percentage>{2}` (круг — один);
//! * `-webkit-gradient()`: две точки (`left top`, `10 20`, `50% 50%`), ступени
//!   `from()`/`to()`/`color-stop(<offset>, <color>)`.
//!
//! Невидимое отличие модели: явные радиусы и пиксельные центры радиального
//! градиента `ParsedGradient` не хранит (см. `RadialSize`), поэтому они
//! проверяются на синтаксис, но рисуются как `farthest-corner` с центром 50 %.

use crate::style::parse::color::parse_color;
use crate::style::parse::image::parse_gradient_stops;
use crate::style::{
    GradientCorner, GradientStop, Length, ParsedGradient, RadialShape, RadialSize, parse_length_q,
    split_top_level_commas,
};

/// Имена легаси-функций; для каждой — (имя с `(`, вид).
const WEBKIT_FNS: [(&str, Kind); 5] = [
    ("-webkit-linear-gradient(", Kind::Linear { repeating: false }),
    ("-webkit-repeating-linear-gradient(", Kind::Linear { repeating: true }),
    ("-webkit-radial-gradient(", Kind::Radial { repeating: false }),
    ("-webkit-repeating-radial-gradient(", Kind::Radial { repeating: true }),
    ("-webkit-gradient(", Kind::Old),
];

#[derive(Clone, Copy)]
enum Kind {
    Linear { repeating: bool },
    Radial { repeating: bool },
    Old,
}

/// Разобрать имя функции в начале `s` и вернуть вид и содержимое скобок.
fn split_function(s: &str) -> Option<(Kind, &str)> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    let (name, kind) = WEBKIT_FNS.iter().find(|(n, _)| lower.starts_with(n))?;
    let inner = s.strip_suffix(')')?;
    Some((*kind, &inner[name.len()..]))
}

/// `s` начинается с одной из легаси-градиентных функций.
pub(in crate::style) fn is_webkit_gradient_function(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    WEBKIT_FNS.iter().any(|(n, _)| lower.starts_with(n))
}

/// Разобрать легаси-градиент; `None` — синтаксис недопустим.
pub fn parse_webkit_gradient(s: &str) -> Option<ParsedGradient> {
    let (kind, inner) = split_function(s)?;
    match kind {
        Kind::Linear { repeating } => parse_linear(inner, repeating),
        Kind::Radial { repeating } => parse_radial(inner, repeating),
        Kind::Old => parse_old(inner),
    }
}

/// `CSS.supports('background-image', value)`: `false`, только если значение —
/// список `<image>`, в котором есть неразборчивый `-webkit-(repeating-)linear/
/// radial-gradient()`. Остальное (в том числе `-webkit-gradient()` с `calc()` в
/// `color-stop()`, который Chrome принимает на этапе парсинга) считается годным.
#[must_use]
pub fn webkit_gradient_value_valid(value: &str) -> bool {
    split_top_level_commas(value).iter().all(|piece| {
        match split_function(piece) {
            Some((Kind::Old, _)) | None => true,
            Some(_) => parse_webkit_gradient(piece).is_some(),
        }
    })
}

// ── Линейный ────────────────────────────────────────────────────────────

fn parse_linear(inner: &str, repeating: bool) -> Option<ParsedGradient> {
    let segs = split_top_level_commas(inner);
    let (angle_deg, corner, rest) = match segs.first().and_then(|f| legacy_linear_direction(f)) {
        Some((a, c)) => (a, c, &segs[1..]),
        None => (180.0, None, &segs[..]),
    };
    let stops = validated_stops(rest)?;
    Some(ParsedGradient::Linear { angle_deg, corner, stops, repeating })
}

/// Первый сегмент как направление: `<angle>` (0° — восток, против часовой)
/// или `[left|right] || [top|bottom]` (начальная сторона). Иначе `None`.
fn legacy_linear_direction(seg: &str) -> Option<(f32, Option<GradientCorner>)> {
    let seg = seg.trim().to_ascii_lowercase();
    if let Some(deg) = legacy_angle_deg(&seg) {
        return Some((90.0 - deg, None));
    }
    let (mut h, mut v) = (None, None);
    for tok in seg.split_whitespace() {
        match tok {
            "left" | "right" if h.is_none() => h = Some(tok),
            "top" | "bottom" if v.is_none() => v = Some(tok),
            _ => return None,
        }
    }
    // Начальная сторона → стандартное направление «к противоположной».
    Some(match (h, v) {
        (None, None) => return None,
        (None, Some("top")) => (180.0, None),
        (None, Some(_)) => (0.0, None),
        (Some("left"), None) => (90.0, None),
        (Some(_), None) => (270.0, None),
        (Some("left"), Some("top")) => (135.0, Some(GradientCorner::DiagBottomRight)),
        (Some("right"), Some("top")) => (225.0, Some(GradientCorner::DiagBottomLeft)),
        (Some("left"), Some(_)) => (45.0, Some(GradientCorner::DiagTopRight)),
        (Some(_), Some(_)) => (315.0, Some(GradientCorner::DiagTopLeft)),
    })
}

/// `<angle>` в градусах; безразмерный `0` допустим (CSS Images §3.1).
fn legacy_angle_deg(s: &str) -> Option<f32> {
    let s = s.trim();
    for (suffix, factor) in [("deg", 1.0), ("grad", 0.9), ("turn", 360.0), ("rad", 180.0 / std::f32::consts::PI)] {
        if let Some(n) = s.strip_suffix(suffix)
            && let Ok(v) = n.trim().parse::<f32>()
        {
            return Some(v * factor);
        }
    }
    (s.parse::<f32>() == Ok(0.0)).then_some(0.0)
}

// ── Радиальный ──────────────────────────────────────────────────────────

fn parse_radial(inner: &str, repeating: bool) -> Option<ParsedGradient> {
    let segs = split_top_level_commas(inner);
    let mut idx = 0;
    let (mut cx, mut cy) = (0.5, 0.5);
    if let Some(seg) = segs.get(idx)
        && let Some(c) = legacy_position(seg)
    {
        (cx, cy) = c;
        idx += 1;
    }
    let (mut shape, mut size) = (RadialShape::Ellipse, RadialSize::FarthestCorner);
    if let Some(seg) = segs.get(idx)
        && let Some(ss) = legacy_shape_size(seg)
    {
        (shape, size) = ss;
        idx += 1;
    }
    let stops = validated_stops(&segs[idx..])?;
    Some(ParsedGradient::Radial { center_x_pct: cx, center_y_pct: cy, shape, size, stops, repeating })
}

fn is_position_word(t: &str) -> bool {
    matches!(t, "left" | "right" | "top" | "bottom" | "center")
}

/// `<length-percentage>` (в том числе отрицательный); `None` — не значение.
fn length_pct(t: &str) -> Option<Length> {
    parse_length_q(t, false)
}

/// `[<position>]` легаси-радиального: 1–2 токена, `(x y)` или `(y x)`.
/// Возвращает центр долями бокса. Пиксельные смещения проверяются, но центр для
/// них остаётся 50 % (`ParsedGradient` хранит только доли).
fn legacy_position(seg: &str) -> Option<(f32, f32)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Axis {
        H,
        V,
        Any,
    }
    let axis = |t: &str| match t {
        "left" | "right" => Axis::H,
        "top" | "bottom" => Axis::V,
        _ => Axis::Any,
    };
    let seg = seg.trim().to_ascii_lowercase();
    let toks: Vec<&str> = seg.split_whitespace().collect();
    if !(1..=2).contains(&toks.len())
        || !toks.iter().all(|t| is_position_word(t) || length_pct(t).is_some())
    {
        return None;
    }
    let frac = |t: &str| -> f32 {
        match t {
            "left" | "top" => 0.0,
            "right" | "bottom" => 1.0,
            _ => t.strip_suffix('%').and_then(|v| v.parse::<f32>().ok()).map_or(0.5, |p| p / 100.0),
        }
    };
    let (x, y) = match toks.as_slice() {
        [a] => match axis(a) {
            Axis::V => ("center", *a),
            _ => (*a, "center"),
        },
        [a, b] => match (axis(a), axis(b)) {
            (Axis::V, Axis::H | Axis::Any) if !length_only(b) => (*b, *a),
            (Axis::H | Axis::Any, Axis::V | Axis::Any) => (*a, *b),
            (Axis::Any, Axis::H) if !length_only(a) => (*b, *a),
            _ => return None,
        },
        _ => return None,
    };
    Some((frac(x), frac(y)))
}

/// Токен — число/процент, а не ключевое слово позиции.
fn length_only(t: &str) -> bool {
    !is_position_word(t)
}

/// `[<ending-shape> || <size>]`: форма, extent-слово или радиусы.
/// `None` — сегмент не относится к форме/размеру.
fn legacy_shape_size(seg: &str) -> Option<(RadialShape, RadialSize)> {
    let seg = seg.trim().to_ascii_lowercase();
    let mut shape = None;
    let mut size = None;
    let mut radii = 0usize;
    for tok in seg.split_whitespace() {
        match tok {
            "circle" if shape.is_none() => shape = Some(RadialShape::Circle),
            "ellipse" if shape.is_none() => shape = Some(RadialShape::Ellipse),
            "closest-side" | "contain" if size.is_none() => size = Some(RadialSize::ClosestSide),
            "closest-corner" if size.is_none() => size = Some(RadialSize::ClosestCorner),
            "farthest-side" if size.is_none() => size = Some(RadialSize::FarthestSide),
            "farthest-corner" | "cover" if size.is_none() => size = Some(RadialSize::FarthestCorner),
            _ => {
                // Радиус: неотрицательный `<length-percentage>`.
                match length_pct(tok) {
                    Some(Length::Px(v) | Length::Percent(v)) if v < 0.0 => return None,
                    Some(_) => radii += 1,
                    None => return None,
                }
            }
        }
    }
    if shape.is_none() && size.is_none() && radii == 0 {
        return None;
    }
    // Радиусы несовместимы с extent-словом; кругу — один, эллипсу — два.
    let ok = match (radii, size.is_some(), shape) {
        (0, _, _) => true,
        (_, true, _) => false,
        (1, false, Some(RadialShape::Circle)) => true,
        (2, false, Some(RadialShape::Ellipse) | None) => true,
        _ => false,
    };
    ok.then(|| (shape.unwrap_or(RadialShape::Ellipse), size.unwrap_or(RadialSize::FarthestCorner)))
}

// ── Ступени ─────────────────────────────────────────────────────────────

/// Список ступеней стандартной грамматики (`<color> <lp>{0,2}` или подсказка
/// `<lp>`), строгий: лишний токен — недопустимое значение. Первая и последняя
/// запись обязаны быть цветом, всего ≥ 2 записей.
fn validated_stops(segs: &[&str]) -> Option<Vec<GradientStop>> {
    if segs.len() < 2 {
        return None;
    }
    for (i, seg) in segs.iter().enumerate() {
        let toks = crate::style::parse::image::paren_whitespace_tokens(seg.trim());
        match toks.as_slice() {
            [] => return None,
            [single] if length_pct(single).is_some() && parse_color(single).is_none() => {
                if i == 0 || i == segs.len() - 1 {
                    return None;
                }
            }
            [color, positions @ ..] => {
                parse_color(color)?;
                if positions.len() > 2 || positions.iter().any(|p| length_pct(p).is_none()) {
                    return None;
                }
            }
        }
    }
    let stops = parse_gradient_stops(&format!("g({})", segs.join(",")));
    (stops.len() >= 2).then_some(stops)
}

// ── -webkit-gradient() ──────────────────────────────────────────────────

/// Координата точки старого синтаксиса: число — пиксели, `%` — доля бокса.
#[derive(Clone, Copy, PartialEq)]
enum Coord {
    Px(f32),
    Pct(f32),
}

impl Coord {
    fn value(self) -> f32 {
        match self {
            Self::Px(v) | Self::Pct(v) => v,
        }
    }
}

fn old_coord(t: &str, horizontal: bool) -> Option<Coord> {
    let (lo, mid, hi) = if horizontal { ("left", "center", "right") } else { ("top", "center", "bottom") };
    let t = t.trim().to_ascii_lowercase();
    if t == lo {
        return Some(Coord::Pct(0.0));
    }
    if t == mid {
        return Some(Coord::Pct(50.0));
    }
    if t == hi {
        return Some(Coord::Pct(100.0));
    }
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse::<f32>().ok().map(Coord::Pct);
    }
    t.parse::<f32>().ok().map(Coord::Px)
}

fn old_point(seg: &str) -> Option<(Coord, Coord)> {
    let toks: Vec<&str> = seg.split_whitespace().collect();
    let [x, y] = toks.as_slice() else { return None };
    Some((old_coord(x, true)?, old_coord(y, false)?))
}

/// Смещение ступени в долях отрезка `0..=1`: число или процент.
fn old_offset(t: &str) -> Option<f32> {
    let t = t.trim();
    match t.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok().map(|v| v / 100.0),
        None => t.parse::<f32>().ok(),
    }
}

/// `from(c)` / `to(c)` / `color-stop(offset, c)` → `(offset, цвет)`.
fn old_stop(seg: &str) -> Option<(f32, GradientStop)> {
    let seg = seg.trim();
    let lower = seg.to_ascii_lowercase();
    let arg = |name: &str| -> Option<&str> {
        lower.starts_with(name).then(|| seg[name.len()..].strip_suffix(')')).flatten()
    };
    let (offset, color) = if let Some(c) = arg("from(") {
        (0.0, c)
    } else if let Some(c) = arg("to(") {
        (1.0, c)
    } else {
        let parts = split_top_level_commas(arg("color-stop(")?);
        let [off, col] = parts.as_slice() else { return None };
        (old_offset(off)?, *col)
    };
    let color = parse_color(color.trim())?;
    Some((offset, GradientStop { color, position: None, ..Default::default() }))
}

fn parse_old(inner: &str) -> Option<ParsedGradient> {
    let segs = split_top_level_commas(inner);
    let kind = segs.first()?.trim().to_ascii_lowercase();
    let (head, radial) = match kind.as_str() {
        "linear" => (3, false),
        "radial" => (5, true),
        _ => return None,
    };
    if segs.len() < head {
        return None;
    }
    let mut stops: Vec<(f32, GradientStop)> = segs[head..].iter().map(|s| old_stop(s)).collect::<Option<_>>()?;
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    if radial {
        let (p1, p2) = (old_point(segs[1])?, old_point(segs[3])?);
        let (r1, r2) = (old_radius(segs[2])?, old_radius(segs[4])?);
        let _ = p1;
        Some(old_radial(p2, r1, r2, stops))
    } else {
        old_linear(old_point(segs[1])?, old_point(segs[2])?, stops)
    }
}

fn old_radius(t: &str) -> Option<f32> {
    t.trim().parse::<f32>().ok().filter(|r| *r >= 0.0)
}

/// Круг(и) с общим центром: ступень `t` лежит на расстоянии `r1 + t·(r2−r1)` px.
/// Центр берётся от внешней окружности; несовпадающие центры — приближение.
fn old_radial((cx, cy): (Coord, Coord), r1: f32, r2: f32, stops: Vec<(f32, GradientStop)>) -> ParsedGradient {
    let frac = |c: Coord| match c {
        Coord::Pct(p) => p / 100.0,
        Coord::Px(_) => 0.5,
    };
    ParsedGradient::Radial {
        center_x_pct: frac(cx),
        center_y_pct: frac(cy),
        shape: RadialShape::Circle,
        size: RadialSize::FarthestCorner,
        stops: stops
            .into_iter()
            .map(|(t, mut s)| {
                s.position = Some(Length::Px(r1 + t * (r2 - r1)));
                s
            })
            .collect(),
        repeating: false,
    }
}

/// Общая единица двух координат одной оси; `0` совместим с любой.
fn unify(a: Coord, b: Coord) -> Option<(f32, f32, bool)> {
    match (a, b) {
        (Coord::Px(x), Coord::Px(y)) => Some((x, y, false)),
        (Coord::Pct(x), Coord::Pct(y)) => Some((x, y, true)),
        (Coord::Px(x), Coord::Pct(y)) if x == 0.0 => Some((x, y, true)),
        (Coord::Pct(x), Coord::Px(y)) if y == 0.0 => Some((x, y, true)),
        (Coord::Px(x), Coord::Pct(y)) if y == 0.0 => Some((x, y, false)),
        (Coord::Pct(x), Coord::Px(y)) if x == 0.0 => Some((x, y, false)),
        _ => None,
    }
}

fn old_linear(a: (Coord, Coord), b: (Coord, Coord), stops: Vec<(f32, GradientStop)>) -> Option<ParsedGradient> {
    let (dx, dy) = (b.0.value() - a.0.value(), b.1.value() - a.1.value());
    // Осевые градиенты задаются без размера бокса: позиция ступени — её
    // координата вдоль оси от верхнего/левого края (в px или %). Если отрезок
    // идёт назад (вверх/влево), порядок ступеней обращается.
    let axis = |from: Coord, to: Coord, angle_deg: f32| -> Option<ParsedGradient> {
        let (v1, v2, pct) = unify(from, to)?;
        let mut seq = stops.clone();
        if v2 < v1 {
            seq.reverse();
        }
        let stops = seq
            .into_iter()
            .map(|(t, mut s)| {
                let v = v1 + t * (v2 - v1);
                s.position = Some(if pct { Length::Percent(v) } else { Length::Px(v) });
                s
            })
            .collect();
        Some(ParsedGradient::Linear { angle_deg, corner: None, stops, repeating: false })
    };
    if dx == 0.0 {
        return axis(a.1, b.1, 180.0);
    }
    if dy == 0.0 {
        return axis(a.0, b.0, 90.0);
    }
    // Диагональ: для угловых точек линия градиента идёт вдоль диагонали бокса
    // (Diag-варианты), для прочих — угол вектора. Позиции — доли отрезка (приближение).
    let corner = match (a, b) {
        ((Coord::Pct(ax), Coord::Pct(ay)), (Coord::Pct(bx), Coord::Pct(by)))
            if [ax, ay, bx, by].iter().all(|v| *v == 0.0 || *v == 100.0) =>
        {
            Some(match (bx > ax, by > ay) {
                (true, true) => (135.0, GradientCorner::DiagBottomRight),
                (false, true) => (225.0, GradientCorner::DiagBottomLeft),
                (true, false) => (45.0, GradientCorner::DiagTopRight),
                (false, false) => (315.0, GradientCorner::DiagTopLeft),
            })
        }
        _ => None,
    };
    let (angle_deg, corner) = match corner {
        Some((a, c)) => (a, Some(c)),
        None => (dx.atan2(-dy).to_degrees().rem_euclid(360.0), None),
    };
    let stops = stops
        .into_iter()
        .map(|(t, mut s)| {
            s.position = Some(Length::Percent(t * 100.0));
            s
        })
        .collect();
    Some(ParsedGradient::Linear { angle_deg, corner, stops, repeating: false })
}
