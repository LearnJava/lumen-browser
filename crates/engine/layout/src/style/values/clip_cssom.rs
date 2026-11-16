//! Грамматика CSS Masking `clip-path`/`clip` для inline-`style` CSSOM (BUG-1408).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Вызывается из
//! JS-шима через `_lumen_css_canonical_mask` (таблица `_LUMEN_MASK_PROPERTIES`); каскад этим
//! кодом не пользуется — его разборщик (`style::parse::shape::parse_clip_path`) мягкий и
//! сводит формы к модели раскладки (`rect()`/`xywh()` → `inset()`, `path()` → полигон).
//!
//! Покрыто: `clip-path: none | <clip-source> | [ <basic-shape> || <geometry-box> ]` и
//! `clip: auto | rect(<top>, <right>, <bottom>, <left>)` (CSS 2.1 §11.1).

use super::background_cssom::position;
use super::length::split_top_level_ws;
use super::text_cssom::canonical_length_sorted;

/// `<geometry-box>` (CSS Masking L1 §4.5).
const GEOMETRY_BOXES: &[&str] = &[
    "border-box",
    "padding-box",
    "content-box",
    "margin-box",
    "fill-box",
    "stroke-box",
    "view-box",
];

/// Каноническая запись `value` свойства `prop` либо `None`, если значение не по грамматике
/// (присваивание игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_clip(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "clip" => clip(v),
        "clip-path" => clip_path(v),
        _ => None,
    }
}

/// CSS 2.1 §11.1: `auto | rect(<top>, <right>, <bottom>, <left>)`; аргументы — `auto` или
/// `<length>` (проценты недопустимы), отрицательные значения разрешены.
fn clip(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("auto") {
        return Some("auto".into());
    }
    let (name, inner) = func(v)?;
    if name != "rect" {
        return None;
    }
    let parts = crate::style::split_top_level_commas(inner);
    if parts.len() != 4 {
        return None;
    }
    let edges: Vec<String> = parts.iter().map(|p| rect_edge(p.trim())).collect::<Option<_>>()?;
    Some(format!("rect({})", edges.join(", ")))
}

/// `auto | <length>` без процентов (аргумент `clip: rect()`).
fn rect_edge(t: &str) -> Option<String> {
    if t.is_empty() {
        return None;
    }
    if t.eq_ignore_ascii_case("auto") {
        return Some("auto".into());
    }
    if t.contains('%') {
        return None;
    }
    lp(t, false)
}

/// `auto | <length-percentage>` (аргумент `rect()` basic-shape, CSS Shapes L1 §3.2).
fn shape_rect_edge(t: &str) -> Option<String> {
    if t.eq_ignore_ascii_case("auto") {
        return Some("auto".into());
    }
    lp(t, false)
}

/// `none | <clip-source> | [ <basic-shape> || <geometry-box> ]` (CSS Masking L1 §3).
fn clip_path(v: &str) -> Option<String> {
    if v.eq_ignore_ascii_case("none") {
        return Some("none".into());
    }
    // `<clip-source>` — единственное значение, бокс с ним не сочетается.
    if func(v).is_some_and(|(name, _)| name == "url") {
        return Some(v.to_string());
    }
    let mut shape: Option<String> = None;
    let mut geometry: Option<String> = None;
    for tok in split_top_level_ws(v) {
        if let Some(g) = GEOMETRY_BOXES.iter().find(|b| tok.eq_ignore_ascii_case(b)) {
            if geometry.replace((*g).to_string()).is_some() {
                return None;
            }
            continue;
        }
        let s = basic_shape(tok)?;
        if shape.replace(s).is_some() {
            return None;
        }
    }
    match (shape, geometry) {
        (None, None) => None,
        (None, Some(g)) => Some(g),
        // `border-box` — начальное значение, в записи опускается.
        (Some(s), Some(g)) if g != "border-box" => Some(format!("{s} {g}")),
        (Some(s), _) => Some(s),
    }
}

/// Имя функции (в нижнем регистре) и её аргументы; `None`, если токен — не целый вызов
/// функции с парными скобками.
fn func(s: &str) -> Option<(String, &str)> {
    let open = s.find('(')?;
    if !s.ends_with(')') {
        return None;
    }
    let name = s[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let inner = &s[open + 1..s.len() - 1];
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then(|| (name.to_ascii_lowercase(), inner))
}

/// Одна `<basic-shape>` (CSS Shapes L1 §3, CSS Shapes 2 §2) → каноническая запись.
fn basic_shape(tok: &str) -> Option<String> {
    let (name, inner) = func(tok)?;
    let body = match name.as_str() {
        "inset" => inset(inner)?,
        "circle" => circle(inner)?,
        "ellipse" => ellipse(inner)?,
        "polygon" => polygon(inner)?,
        "shape" => shape(inner)?,
        "path" => path(inner)?,
        "rect" => {
            let toks = split_top_level_ws(inner);
            let (args, round) = split_round(&toks)?;
            rounded(args, &round, shape_rect_edge)?
        }
        "xywh" => {
            let toks = split_top_level_ws(inner);
            let (args, round) = split_round(&toks)?;
            rounded(args, &round, |t| lp(t, false))?
        }
        _ => return None,
    };
    Some(format!("{name}({body})"))
}

/// `<length-percentage>` (`non_negative` — `[0,∞]`).
fn lp(tok: &str, non_negative: bool) -> Option<String> {
    let tok = tok.trim();
    // Единицы регистронезависимы (`0Px`), а каноническая запись — в нижнем регистре.
    let lower = tok.to_ascii_lowercase();
    let canon = canonical_length_sorted(tok, false, non_negative)
        .or_else(|| canonical_length_sorted(&lower, false, non_negative))?;
    // CSSOM пишет указанное значение: абсолютные единицы (`pt`, `cm`, …) движок сводит к px,
    // поэтому единицу берём из исходного токена (`polygon(… 5pt 6%)` остаётся `5pt`).
    match literal_unit(&lower) {
        Some((n, unit)) => Some(format!("{n}{unit}")),
        None => Some(canon),
    }
}

/// Число и единица литерала `<length-percentage>` (`5pt` → `(5.0, "pt")`); `None` — не литерал
/// (`calc(…)`, `0`).
fn literal_unit(tok: &str) -> Option<(f32, String)> {
    let i = tok.find(|c: char| c.is_ascii_alphabetic() || c == '%')?;
    let n: f32 = tok[..i].parse().ok()?;
    let unit = tok[i..].to_ascii_lowercase();
    let valid = unit == "%" || unit.chars().all(|c| c.is_ascii_alphabetic());
    valid.then_some((n, unit))
}

/// Хвост `round <radii>{1,4} [/ <radii>{1,4}]?`; `''` — все радиусы нулевые, `round`
/// в записи опускается.
fn round_radii(toks: &[&str]) -> Option<String> {
    fn side(ts: &[&str]) -> Option<Vec<String>> {
        if ts.is_empty() || ts.len() > 4 {
            return None;
        }
        ts.iter().map(|t| lp(t, true)).collect()
    }
    let (h, v) = match toks.iter().position(|t| *t == "/") {
        Some(j) => (side(&toks[..j])?, Some(side(&toks[j + 1..])?)),
        None => (side(toks)?, None),
    };
    let zero = |xs: &[String]| xs.iter().all(|x| x == "0px");
    if zero(&h) && v.as_deref().is_none_or(zero) {
        return Some(String::new());
    }
    Some(match v {
        Some(v) => format!("{} / {}", h.join(" "), v.join(" ")),
        None => h.join(" "),
    })
}

/// Делит токены на аргументы до `round` и сам `round`-хвост.
fn split_round<'a>(toks: &'a [&'a str]) -> Option<(&'a [&'a str], Option<String>)> {
    match toks.iter().position(|t| t.eq_ignore_ascii_case("round")) {
        Some(i) => Some((&toks[..i], Some(round_radii(&toks[i + 1..])?))),
        None => Some((toks, None)),
    }
}

/// Четыре аргумента формы + необязательный `round` (`rect()`, `xywh()`).
fn rounded(args: &[&str], round: &Option<String>, edge: impl Fn(&str) -> Option<String>) -> Option<String> {
    let [a, b, c, d] = args else { return None };
    let mut out = [edge(a)?, edge(b)?, edge(c)?, edge(d)?].join(" ");
    if let Some(r) = round.as_ref().filter(|r| !r.is_empty()) {
        out.push_str(" round ");
        out.push_str(r);
    }
    Some(out)
}

/// `inset( <length-percentage>{1,4} [round <radii>]? )` (CSS Shapes L1 §3.3).
fn inset(inner: &str) -> Option<String> {
    let toks = split_top_level_ws(inner);
    let (sides, round) = split_round(&toks)?;
    if sides.is_empty() || sides.len() > 4 {
        return None;
    }
    let sides: Vec<String> = sides.iter().map(|t| lp(t, false)).collect::<Option<_>>()?;
    let mut body = sides.join(" ");
    if let Some(r) = round.as_ref().filter(|r| !r.is_empty()) {
        body.push_str(" round ");
        body.push_str(r);
    }
    Some(body)
}

/// Делит токены формы на радиусы до `at` и `<position>` после него.
fn split_at<'a>(toks: &'a [&'a str]) -> Option<(&'a [&'a str], Option<String>)> {
    match toks.iter().position(|t| t.eq_ignore_ascii_case("at")) {
        Some(i) => Some((&toks[..i], Some(position(&toks[i + 1..].join(" "))?))),
        None => Some((toks, None)),
    }
}

/// `<shape-radius>`: `closest-side | farthest-side | <length-percentage [0,∞]>`.
fn radius_value(t: &str) -> Option<String> {
    if t.eq_ignore_ascii_case("closest-side") {
        return Some("closest-side".into());
    }
    if t.eq_ignore_ascii_case("farthest-side") {
        return Some("farthest-side".into());
    }
    lp(t, true)
}

/// `circle( <shape-radius>? [at <position>]? )`; `closest-side` опускается.
fn circle(inner: &str) -> Option<String> {
    let toks = split_top_level_ws(inner);
    let (radii, center) = split_at(&toks)?;
    let radius = match radii {
        [] => None,
        [r] => Some(radius_value(r)?),
        _ => return None,
    };
    let mut parts: Vec<String> = radius.filter(|r| r != "closest-side").into_iter().collect();
    if let Some(c) = center {
        parts.push(format!("at {c}"));
    }
    Some(parts.join(" "))
}

/// `ellipse( [<shape-radius>{2}]? [at <position>]? )`; два `closest-side` опускаются.
fn ellipse(inner: &str) -> Option<String> {
    let toks = split_top_level_ws(inner);
    let (radii, center) = split_at(&toks)?;
    let (rx, ry) = match radii {
        [] => ("closest-side".to_string(), "closest-side".to_string()),
        [a, b] => (radius_value(a)?, radius_value(b)?),
        _ => return None,
    };
    let mut parts: Vec<String> = Vec::new();
    if rx != "closest-side" || ry != "closest-side" {
        parts.push(rx);
        parts.push(ry);
    }
    if let Some(c) = center {
        parts.push(format!("at {c}"));
    }
    Some(parts.join(" "))
}

/// `polygon( <fill-rule>? [round <length>]? , <length-percentage>{2} # )` (CSS Shapes 2 §2.1):
/// `nonzero` и нулевой радиус в записи опускаются.
fn polygon(inner: &str) -> Option<String> {
    let parts = crate::style::split_top_level_commas(inner);
    let mut iter = parts.iter();
    let toks = split_top_level_ws(iter.next()?.trim());
    let mut prelude: Vec<String> = Vec::new();
    let mut i = 0;
    if let Some(t) = toks.first() {
        if t.eq_ignore_ascii_case("nonzero") {
            i = 1;
        } else if t.eq_ignore_ascii_case("evenodd") {
            prelude.push("evenodd".into());
            i = 1;
        }
    }
    if toks.get(i).is_some_and(|t| t.eq_ignore_ascii_case("round")) {
        let radius = toks.get(i + 1)?;
        // `<length>`, не `<length-percentage>`: `polygon(round 1%, …)` невалиден.
        if radius.contains('%') {
            return None;
        }
        let radius = lp(radius, true)?;
        if radius != "0px" {
            prelude.push(format!("round {radius}"));
        }
        i += 2;
    }
    let mut vertices: Vec<String> = Vec::new();
    if i < toks.len() {
        // Хвост после `fill-rule`/`round` в том же куске до запятой — не по грамматике:
        // прелюдия отделяется от первой вершины запятой.
        if i > 0 {
            return None;
        }
        vertices.push(vertex(&toks[i..])?);
    }
    for part in iter {
        vertices.push(vertex(&split_top_level_ws(part.trim()))?);
    }
    if vertices.is_empty() {
        return None;
    }
    let mut body = String::new();
    if !prelude.is_empty() {
        body.push_str(&prelude.join(" "));
        body.push_str(", ");
    }
    body.push_str(&vertices.join(", "));
    Some(body)
}

fn vertex(toks: &[&str]) -> Option<String> {
    let [x, y] = toks else { return None };
    Some(format!("{} {}", lp(x, false)?, lp(y, false)?))
}

/// `shape( <fill-rule>? from <coordinate-pair> , <shape-command># )` (CSS Shapes 2 §2.1):
/// `nonzero` опускается, команды пишутся в исходном порядке через `, `.
fn shape(inner: &str) -> Option<String> {
    let parts = crate::style::split_top_level_commas(inner);
    let mut iter = parts.iter();
    let head = split_top_level_ws(iter.next()?.trim());
    let (fill, rest) = match head.first() {
        Some(t) if t.eq_ignore_ascii_case("evenodd") => (Some("evenodd"), &head[1..]),
        Some(t) if t.eq_ignore_ascii_case("nonzero") => (None, &head[1..]),
        _ => (None, &head[..]),
    };
    let [from, x, y] = rest else { return None };
    if !from.eq_ignore_ascii_case("from") {
        return None;
    }
    let mut commands: Vec<String> = Vec::new();
    for part in iter {
        commands.push(command(&split_top_level_ws(part.trim()))?);
    }
    if commands.is_empty() {
        return None;
    }
    let fill = fill.map_or(String::new(), |f| format!("{f} "));
    Some(format!("{fill}from {}, {}", vertex(&[x, y])?, commands.join(", ")))
}

/// Одна `<shape-command>`; `line`/`move` — `to|by` + пара, `hline`/`vline` — `to|by` + длина,
/// `curve`/`smooth` — пара и необязательный `with` (одна или две контрольные точки через `/`),
/// `arc` — пара, `of <radii>`, флаги `cw|ccw`/`large|small`/`rotate <angle>` в любом порядке.
fn command(toks: &[&str]) -> Option<String> {
    let name = toks.first()?.to_ascii_lowercase();
    let args = &toks[1..];
    match name.as_str() {
        "close" if args.is_empty() => Some("close".into()),
        "move" | "line" => {
            let [dir, x, y] = args else { return None };
            Some(format!("{name} {} {}", direction(dir)?, vertex(&[x, y])?))
        }
        "hline" | "vline" => {
            let [dir, v] = args else { return None };
            Some(format!("{name} {} {}", direction(dir)?, lp(v, false)?))
        }
        "curve" | "smooth" => {
            let [dir, x, y, rest @ ..] = args else { return None };
            let mut out = format!("{name} {} {}", direction(dir)?, vertex(&[x, y])?);
            match rest {
                [] => {}
                [with, cx, cy] if with.eq_ignore_ascii_case("with") => {
                    out.push_str(" with ");
                    out.push_str(&vertex(&[cx, cy])?);
                }
                [with, cx, cy, slash, dx, dy]
                    if with.eq_ignore_ascii_case("with") && *slash == "/" =>
                {
                    out.push_str(" with ");
                    out.push_str(&vertex(&[cx, cy])?);
                    out.push_str(" / ");
                    out.push_str(&vertex(&[dx, dy])?);
                }
                _ => return None,
            }
            Some(out)
        }
        "arc" => arc(args),
        _ => None,
    }
}

fn direction(t: &str) -> Option<&'static str> {
    if t.eq_ignore_ascii_case("to") {
        Some("to")
    } else if t.eq_ignore_ascii_case("by") {
        Some("by")
    } else {
        None
    }
}

fn arc(args: &[&str]) -> Option<String> {
    let [dir, x, y, of, radius, rest @ ..] = args else { return None };
    if !of.eq_ignore_ascii_case("of") {
        return None;
    }
    let mut out = format!("arc {} {} of {}", direction(dir)?, vertex(&[x, y])?, lp(radius, true)?);
    // Необязательный второй радиус — первый токен, не являющийся флагом дуги.
    let rest = match rest.first() {
        Some(t) if !is_arc_flag(t) => {
            out.push(' ');
            out.push_str(&lp(t, true)?);
            &rest[1..]
        }
        _ => rest,
    };
    let (mut sweep, mut size, mut angle) = (None, None, None);
    let mut i = 0;
    while i < rest.len() {
        let t = rest[i];
        if t.eq_ignore_ascii_case("cw") || t.eq_ignore_ascii_case("ccw") {
            sweep = Some(t.to_ascii_lowercase());
        } else if t.eq_ignore_ascii_case("large") || t.eq_ignore_ascii_case("small") {
            size = Some(t.to_ascii_lowercase());
        } else if t.eq_ignore_ascii_case("rotate") {
            i += 1;
            angle = Some(angle_value(rest.get(i)?)?);
        } else {
            return None;
        }
        i += 1;
    }
    // `small` и нулевой угол — начальные значения, в записи опускаются.
    let parts = [sweep, size.filter(|s| s != "small"), angle.map(|(_, a)| format!("rotate {a}"))];
    for part in parts.into_iter().flatten().filter(|p| p != "rotate ") {
        out.push(' ');
        out.push_str(&part);
    }
    Some(out)
}

fn is_arc_flag(t: &str) -> bool {
    ["cw", "ccw", "large", "small", "rotate"].iter().any(|f| t.eq_ignore_ascii_case(f))
}

/// `<angle>` в записи `rotate <angle>`: литерал с прежней единицей, угол `0` — начальное
/// значение (тогда строка пустая).
fn angle_value(t: &str) -> Option<(f32, String)> {
    let lower = t.to_ascii_lowercase();
    crate::style::parse::transform::parse_angle_to_radians(&lower)?;
    let i = lower.find(|c: char| c.is_ascii_alphabetic())?;
    let n: f32 = lower[..i].parse().ok()?;
    Some((n, if n == 0.0 { String::new() } else { format!("{n}{}", &lower[i..]) }))
}


/// `path( [<fill-rule>,]? <string> )` (CSS Shapes L1 §4): `nonzero` опускается, данные пути
/// приводятся к одной записи (`M20,20h60` → `M 20 20 h 60`).
fn path(inner: &str) -> Option<String> {
    let (fill, rest) = match inner.split_once(',') {
        Some((head, rest)) if head.trim().eq_ignore_ascii_case("evenodd") => (Some("evenodd"), rest),
        Some((head, rest)) if head.trim().eq_ignore_ascii_case("nonzero") => (None, rest),
        _ => (None, inner),
    };
    let data = unquote(rest)?;
    let mut out = String::new();
    if let Some(f) = fill {
        out.push_str(f);
        out.push_str(", ");
    }
    out.push('"');
    out.push_str(&path_data(data)?);
    out.push('"');
    Some(out)
}

/// Строка в одинарных или двойных кавычках, занимающая весь `s`.
fn unquote(s: &str) -> Option<&str> {
    let s = s.trim();
    let q = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &s[1..];
    let end = rest.rfind(q)?;
    rest[end + 1..].trim().is_empty().then(|| &rest[..end])
}

/// Данные SVG-пути: команды и числа через один пробел, `z` → `Z` (CSS Shapes L1 §4).
/// Запятая перед командой (`L0 0, L0 0`) — не по грамматике пути: `None`.
fn path_data(data: &str) -> Option<String> {
    fn push(out: &mut String, t: &str) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
    }
    let b = data.as_bytes();
    let mut out = String::new();
    let (mut i, mut after_comma) = (0usize, false);
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c => i += 1,
            b',' => {
                after_comma = true;
                i += 1;
            }
            c if c.is_ascii_alphabetic() => {
                let valid = matches!(
                    c.to_ascii_lowercase(),
                    b'm' | b'l' | b'h' | b'v' | b'c' | b's' | b'q' | b't' | b'a' | b'z'
                );
                if after_comma || !valid {
                    return None;
                }
                push(&mut out, if c.eq_ignore_ascii_case(&b'z') { "Z" } else { &data[i..i + 1] });
                after_comma = false;
                i += 1;
            }
            _ => {
                let start = i;
                if matches!(b[i], b'+' | b'-') {
                    i += 1;
                }
                let mut digits = 0;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                    digits += 1;
                }
                if i < b.len() && b[i] == b'.' {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                        digits += 1;
                    }
                }
                if digits == 0 {
                    return None;
                }
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    i += 1;
                    if i < b.len() && matches!(b[i], b'+' | b'-') {
                        i += 1;
                    }
                    let mut exp = 0;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                        exp += 1;
                    }
                    if exp == 0 {
                        return None;
                    }
                }
                push(&mut out, std::str::from_utf8(&b[start..i]).ok()?);
                after_comma = false;
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::canonical_specified_clip as c;

    #[test]
    fn clip_rect() {
        assert_eq!(c("clip", "auto").as_deref(), Some("auto"));
        assert_eq!(c("clip", "rect(10px, 20px, -30px, 40px)").as_deref(), Some("rect(10px, 20px, -30px, 40px)"));
        assert_eq!(c("clip", "rect(10px,-20px,auto,0)").as_deref(), Some("rect(10px, -20px, auto, 0px)"));
        assert_eq!(c("clip", "rect(5pt, 2cm, auto, 0)").as_deref(), Some("rect(5pt, 2cm, auto, 0px)"));
        for bad in ["none", "rect(10px, 20px, 30px)", "rect(10%, -20%, auto, auto)", "rect(10px 20px, 30px 40px)"] {
            assert_eq!(c("clip", bad), None, "{bad}");
        }
    }

    #[test]
    fn clip_path_shapes() {
        assert_eq!(c("clip-path", "none").as_deref(), Some("none"));
        assert_eq!(c("clip-path", "inset(0 1px)").as_deref(), Some("inset(0px 1px)"));
        assert_eq!(c("clip-path", "inset(0px round 0 1px)").as_deref(), Some("inset(0px round 0px 1px)"));
        assert_eq!(c("clip-path", "inset(10px round 20% / 0 1px)").as_deref(), Some("inset(10px round 20% / 0px 1px)"));
        assert_eq!(c("clip-path", "circle()").as_deref(), Some("circle()"));
        assert_eq!(c("clip-path", "circle(closest-side)").as_deref(), Some("circle()"));
        assert_eq!(c("clip-path", "circle(4% at top right)").as_deref(), Some("circle(4% at right top)"));
        assert_eq!(c("clip-path", "circle(at 10% 20%)").as_deref(), Some("circle(at 10% 20%)"));
        assert_eq!(c("clip-path", "ellipse(closest-side closest-side at 10% 20%)").as_deref(), Some("ellipse(at 10% 20%)"));
        assert_eq!(c("clip-path", "ellipse(farthest-side 4% at bottom left)").as_deref(), Some("ellipse(farthest-side 4% at left bottom)"));
        assert_eq!(c("clip-path", "polygon(nonzero, 1px 2px, 3em 4em)").as_deref(), Some("polygon(1px 2px, 3em 4em)"));
        assert_eq!(c("clip-path", "polygon(round 0px, 1px 2px, 3px 4px)").as_deref(), Some("polygon(1px 2px, 3px 4px)"));
        assert_eq!(
            c("clip-path", "polygon(nonzero round 1px, 1px 2px, 3em 4em)").as_deref(),
            Some("polygon(round 1px, 1px 2px, 3em 4em)")
        );
        assert_eq!(c("clip-path", "polygon(1% 2%)").as_deref(), Some("polygon(1% 2%)"));
        assert_eq!(
            c("clip-path", "polygon(evenodd round 1px, 1px 2px, 3em 4em, 5pt 6%)").as_deref(),
            Some("polygon(evenodd round 1px, 1px 2px, 3em 4em, 5pt 6%)")
        );
        assert_eq!(c("clip-path", "xywh(0 1% 2px 3em)").as_deref(), Some("xywh(0px 1% 2px 3em)"));
        assert_eq!(c("clip-path", "xywh(0px 1% 2px 3em round 0)").as_deref(), Some("xywh(0px 1% 2px 3em)"));
        assert_eq!(c("clip-path", "rect(0 100% 200px 4em)").as_deref(), Some("rect(0px 100% 200px 4em)"));
        assert_eq!(c("clip-path", "rect(auto auto auto auto)").as_deref(), Some("rect(auto auto auto auto)"));
        assert_eq!(c("clip-path", "circle(7% at 8% 9%) border-box").as_deref(), Some("circle(7% at 8% 9%)"));
        assert_eq!(c("clip-path", "padding-box circle(7% at 8% 9%)").as_deref(), Some("circle(7% at 8% 9%) padding-box"));
    }

    #[test]
    fn clip_path_path_data() {
        assert_eq!(c("clip-path", "path(\"m 20 0 h -100\")").as_deref(), Some("path(\"m 20 0 h -100\")"));
        assert_eq!(
            c("clip-path", "path(nonzero, \"M20,20h60 v60 h-60z M30,30 h40 v40 h-40z\")").as_deref(),
            Some("path(\"M 20 20 h 60 v 60 h -60 Z M 30 30 h 40 v 40 h -40 Z\")")
        );
        assert_eq!(
            c("clip-path", "path(evenodd, \"M0 0L1 1\")").as_deref(),
            Some("path(evenodd, \"M 0 0 L 1 1\")")
        );
        for bad in [
            "path(\" \")",
            "path(evenodd, \"\")",
            "path(abc, \"m 20 0 h -100 z\")",
            "path(nonzero)",
            "path(\"m 20 0 h -100\", nonzero)",
            "path(nonzero, 'M0 0, L100 0, L0 100, L 0 0')",
        ] {
            assert_eq!(c("clip-path", bad), None, "{bad}");
        }
    }

    #[test]
    fn clip_path_shape_function() {
        for (v, want) in [
            ("shape(from 0px 0px, line to 10px 10px)", "shape(from 0px 0px, line to 10px 10px)"),
            ("shape(evenodd from 0px 0px, line to 10px 10px)", "shape(evenodd from 0px 0px, line to 10px 10px)"),
            ("shape(nonzero from 0px 0px, line to 10px 10px)", "shape(from 0px 0px, line to 10px 10px)"),
            (
                "shape(  from 0px    0px, line  to 10px  10px     )",
                "shape(from 0px 0px, line to 10px 10px)",
            ),
            ("shape(EvenOdd from 0px 0Px, CLOSE)", "shape(evenodd from 0px 0px, close)"),
            ("shape(from 1ch -50px, line to -10% 12px)", "shape(from 1ch -50px, line to -10% 12px)"),
            (
                "shape(from 10px 10px, move by 10px 5px, line by 20px 40%, close)",
                "shape(from 10px 10px, move by 10px 5px, line by 20px 40%, close)",
            ),
            (
                "shape(from 10px 10px, curve to 50px 20px with 10rem 1px / 20vh 1ch)",
                "shape(from 10px 10px, curve to 50px 20px with 10rem 1px / 20vh 1ch)",
            ),
            ("shape(from 10px 10px, smooth to 50px 1pt)", "shape(from 10px 10px, smooth to 50px 1pt)"),
            (
                "shape(from 10px 10px, arc to 50px 1pt of 10px 10px small rotate 0deg)",
                "shape(from 10px 10px, arc to 50px 1pt of 10px 10px)",
            ),
            (
                "shape(from 10% 1rem, arc to 50px 1pt of 20% cw large rotate 25deg)",
                "shape(from 10% 1rem, arc to 50px 1pt of 20% cw large rotate 25deg)",
            ),
        ] {
            assert_eq!(c("clip-path", v).as_deref(), Some(want), "{v}");
        }
        for bad in [
            "shape(from 10% 1rem, arc of 20% 1pt to 50px cw large rotate 25deg)",
            "shape(from 10% 1rem, arc cw to 50px of 20% 1pt large rotate 25deg)",
            "shape(from 10% 1rem, arc rotate 25deg to 50px 1pt of 20% cw large )",
            "shape(evenodd from 0px 0px, close path)",
            "shape(nonzero, from 0px 0px, line to 10px 10px)",
            "shape(from 10px 10px, curve to 50px 20px via 10rem)",
            "shape(from 10px 10px, hline byy 10px, vline to 5rem)",
            "shape(from 10px 10px, vline by 5% hline by 1vw",
            "shape()",
            "shape(from)",
            "shape(from 0px)",
            "shape(from 0px 20px,)",
            "shape(close)",
            "shape(nonzero, close)",
            "shape(from 0px 10px)",
            "shape(allkindsofnonsense)",
            "shape(arc)",
        ] {
            assert_eq!(c("clip-path", bad), None, "{bad}");
        }
    }

    #[test]
    fn clip_path_invalid() {
        for bad in [
            "auto",
            "ray(0deg)",
            "inset()",
            "inset(123)",
            "inset(1% 2% 3% 4% 5%)",
            "inset(round 0)",
            "inset(0px round)",
            "inset(0px round 123)",
            "inset(0px round 1% 2% 3% 4% 5%)",
            "inset(0px round / 1px)",
            "inset(10px round -20px)",
            "inset(30% round -40%)",
            "circle(123)",
            "circle(at)",
            "circle(10% 20%)",
            "circle(-10px at 20px 30px)",
            "circle(1% 2% at 0% 100%)",
            "ellipse(farthest-side at)",
            "ellipse(1% 2% top right)",
            "ellipse(3%)",
            "ellipse(closest-side)",
            "ellipse(10% -20% at 30% 40%)",
            "polygon(1%)",
            "polygon(round, 1px 2px)",
            "polygon(round -1px, 1px 2px)",
            "polygon(round 1%, 1px 2px)",
            "polygon(round 45deg, 1px 2px)",
            "polygon(round 1px 1px 2px)",
            "polygon(1px 2px, round 1px)",
            "polygon(round 1px round 2px, 1px 2px)",
            "polygon(round 1px nonzero, 1px 2px)",
            "unknown-box",
            "xywh(0px)",
            "xywh(0px 1%)",
            "xywh(0px 1% 2em)",
            "rect(0px)",
            "rect(0px 1%)",
            "rect(0px 1% auto)",
        ] {
            assert_eq!(c("clip-path", bad), None, "{bad}");
        }
    }
}
