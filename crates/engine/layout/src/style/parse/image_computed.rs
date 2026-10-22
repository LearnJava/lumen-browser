//! Computed-value запись `<image>` для `getComputedStyle().backgroundImage` (BUG-1428).
//!
//! `BackgroundImage` хранит разобранную геометрию (проценты от бокса, радианы, `Url(raw)` для
//! `image-set()`), а CSSOM Images L4 §«Serialization» требует вернуть функцию целиком: цвета —
//! в `rgb()`/своём пространстве, длины — в px, позиции — парой `<lp> <lp>`, углы — в `deg`,
//! `image-set()` — с `<resolution>` в `dppx`, `cross-fade()` — с процентами. Эта запись строится
//! из исходного текста один раз при применении декларации и лежит в `BackgroundLayer::image_css`;
//! отрисовка её не читает.
//!
//! Тот же разбор в режиме `specified` ([`specified_gradient_css`]) даёт каноническую *заданную*
//! запись градиента для `el.style.backgroundImage`: значения остаются как написаны, но
//! `in <space>` стоит после направления, метод по умолчанию и `shorter hue` опущены, `xyz` —
//! `xyz-d65`, а недопустимая конфигурация (`in` без пространства, метод после остановок)
//! отвергается.
//!
//! `None` — форма не распознана (префиксные `-webkit-*-gradient`, `calc()` в позициях): вызывающий
//! остаётся при прежней записи (`url("…")` / `none`).

use lumen_core::geom::Size;

use crate::selector_query::color_to_css;
use crate::style::parse::box_sides::resolve_box_length;
use crate::style::parse::color::{canonical_specified_color, parse_css_color_legacy};
use crate::style::parse::color_serialize::computed_color_css;
use crate::style::values::length::split_top_level_ws;
use crate::style::values::text_cssom::sort_calc_sum;
use crate::style::{CssColor, split_top_level_commas};

/// Контекст: `em` и размер вьюпорта для длин; `specified` — не приводить значения к computed.
#[derive(Clone, Copy)]
struct Ctx {
    em: f32,
    viewport: Size,
    specified: bool,
}

/// Каноническая запись одного `<image>`; см. модульный комментарий.
pub(in crate::style) fn computed_image_css(s: &str, em: f32, viewport: Size) -> Option<String> {
    image(s, &Ctx { em, viewport, specified: false })
}

/// Каноническая заданная запись `*-gradient()` для CSSOM (`el.style.backgroundImage`). `None` —
/// конфигурация недопустима либо `s` не градиент.
pub(in crate::style) fn specified_gradient_css(s: &str) -> Option<String> {
    let t = s.trim();
    let open = t.find('(')?;
    let name = t[..open].to_ascii_lowercase();
    if !is_gradient_name(&name) || !t.ends_with(')') {
        return None;
    }
    let c = Ctx { em: 16.0, viewport: Size::new(0.0, 0.0), specified: true };
    gradient(&name, &t[open + 1..t.len() - 1], &c)
}

fn is_gradient_name(name: &str) -> bool {
    matches!(
        name,
        "linear-gradient" | "repeating-linear-gradient" | "radial-gradient"
            | "repeating-radial-gradient" | "conic-gradient" | "repeating-conic-gradient"
    )
}

fn image(s: &str, c: &Ctx) -> Option<String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "none" {
        return Some("none".into());
    }
    if lower.starts_with("url(") && t.ends_with(')') {
        let (url, mods) = super::image::parse_url_with_modifiers(t)?;
        let text = url_text(&url);
        return Some(match mods.serialize() {
            m if m.is_empty() => text,
            m => format!("{}{m})", text.strip_suffix(')').unwrap_or(&text)),
        });
    }
    let open = lower.find('(')?;
    if !t.ends_with(')') {
        return None;
    }
    let name = &lower[..open];
    let inner = &t[open + 1..t.len() - 1];
    match name {
        "image-set" | "-webkit-image-set" => image_set(inner, c),
        "cross-fade" => cross_fade(inner, c),
        n if is_gradient_name(n) => gradient(n, inner, c),
        _ => None,
    }
}

fn url_text(path: &str) -> String {
    format!("url(\"{}\")", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Число без хвостовых нулей, шесть знаков после запятой (`63.5`, `1`, `0.333333`).
fn fmt_num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

// ── цвет ────────────────────────────────────────────────────────────────────────────────────

/// Computed `<color>` и признак «legacy-синтаксис» (`rgb()`/`hsl()`/имя/hex — для выбора
/// пространства интерполяции по умолчанию, CSS Color 4 §12.1).
fn color_text(tok: &str, c: &Ctx) -> Option<(String, bool)> {
    let (css, legacy) = computed_color_text(tok)?;
    if c.specified {
        return Some((canonical_specified_color(tok)?, legacy));
    }
    Some((css, legacy))
}

fn computed_color_text(tok: &str) -> Option<(String, bool)> {
    let css = computed_color_css(tok).or_else(|| match parse_css_color_legacy(tok.trim(), false)? {
        CssColor::Rgba(col) => Some(color_to_css(col)),
        CssColor::CurrentColor => Some("currentcolor".into()),
        CssColor::Wide(f) => Some(color_to_css(f.to_srgb_color())),
        CssColor::System(sc) => Some(color_to_css(sc.resolve_color(false))),
    })?;
    let legacy = css.starts_with("rgb") || css == "currentcolor";
    Some((css, legacy))
}

// ── длины, углы, позиции ─────────────────────────────────────────────────────────────────────

/// `<length-percentage>` → `N%` либо `Npx`; `calc()` и прочие функции не берём.
fn lp(tok: &str, c: &Ctx) -> Option<String> {
    let t = tok.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains('(') {
        let lower = t.to_ascii_lowercase();
        let math = ["calc(", "min(", "max(", "clamp("].iter().any(|f| lower.starts_with(f));
        // Выражение не разворачиваем: computed-запись остаётся `calc()`, и её же читает round-trip.
        return math.then(|| sort_calc_sum(&lower).unwrap_or(lower));
    }
    if let Some(n) = t.strip_suffix('%') {
        let v = n.parse::<f64>().ok()?;
        return Some(if c.specified { t.to_string() } else { format!("{}%", fmt_num(v)) });
    }
    if t.parse::<f64>().ok() == Some(0.0) {
        return Some(if c.specified { t.to_string() } else { "0px".into() });
    }
    let px = resolve_box_length(t, c.em, c.viewport, false)?;
    Some(if c.specified { t.to_string() } else { format!("{}px", fmt_num(f64::from(px))) })
}

/// `<angle>` → `Ndeg` (`0` без единицы допустим).
fn angle(tok: &str, c: &Ctx) -> Option<String> {
    let t = tok.trim().to_ascii_lowercase();
    if t.parse::<f64>().ok() == Some(0.0) {
        return Some(if c.specified { t } else { "0deg".into() });
    }
    if t.starts_with("calc(") {
        // `<angle>` без процентов (CSS Images 4 §«conic-gradient», `from`).
        return (!t.contains('%')).then(|| sort_calc_sum(&t).unwrap_or(t));
    }
    // `grad` раньше `rad`: иначе `10grad` режется по суффиксу `rad`.
    let units = [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f64::consts::PI), ("turn", 360.0)];
    let (num, k) = units.iter().find_map(|(u, k)| t.strip_suffix(u).map(|n| (n, *k)))?;
    let v = num.parse::<f64>().ok()?;
    Some(if c.specified { t.clone() } else { format!("{}deg", fmt_num(v * k)) })
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

fn edge(t: &str) -> Option<Edge> {
    match t.to_ascii_lowercase().as_str() {
        "left" => Some(Edge::Left),
        "right" => Some(Edge::Right),
        "top" => Some(Edge::Top),
        "bottom" => Some(Edge::Bottom),
        "center" => Some(Edge::Center),
        _ => None,
    }
}

fn is_horizontal(e: Edge) -> bool {
    matches!(e, Edge::Left | Edge::Right)
}

fn is_vertical(e: Edge) -> bool {
    matches!(e, Edge::Top | Edge::Bottom)
}

fn edge_pct(e: Edge) -> &'static str {
    match e {
        Edge::Left | Edge::Top => "0%",
        Edge::Right | Edge::Bottom => "100%",
        Edge::Center => "50%",
    }
}

/// Смещение от дальнего края: `100% - off` (проценты складываются, длины — `calc()`).
fn from_far_edge(off: &str) -> String {
    if let Some(p) = off.strip_suffix('%')
        && let Ok(v) = p.parse::<f64>()
    {
        return format!("{}%", fmt_num(100.0 - v));
    }
    match off.strip_prefix('-') {
        Some(rest) => format!("calc(100% + {rest})"),
        None => format!("calc(100% - {off})"),
    }
}

/// `<position>` (CSS Values 4 §«position») → computed `(x, y)`.
fn position(tokens: &[&str], c: &Ctx) -> Option<(String, String)> {
    match tokens {
        [] => None,
        [t] => match edge(t) {
            Some(e) if is_vertical(e) => Some(("50%".into(), edge_pct(e).into())),
            Some(e) => Some((edge_pct(e).into(), "50%".into())),
            None => Some((lp(t, c)?, "50%".into())),
        },
        [a, b] => match (edge(a), edge(b)) {
            (Some(ea), Some(eb)) => {
                let (x, y) = if is_vertical(ea) || is_horizontal(eb) { (eb, ea) } else { (ea, eb) };
                if is_vertical(x) || is_horizontal(y) {
                    return None;
                }
                Some((edge_pct(x).into(), edge_pct(y).into()))
            }
            (Some(ea), None) if !is_vertical(ea) => Some((edge_pct(ea).into(), lp(b, c)?)),
            (None, Some(eb)) if !is_horizontal(eb) => Some((lp(a, c)?, edge_pct(eb).into())),
            (None, None) => Some((lp(a, c)?, lp(b, c)?)),
            _ => None,
        },
        [e1, o1, e2, o2] => {
            let (e1, e2) = (edge(e1)?, edge(e2)?);
            let (xe, xo, ye, yo) = if is_horizontal(e1) && is_vertical(e2) {
                (e1, o1, e2, o2)
            } else if is_vertical(e1) && is_horizontal(e2) {
                (e2, o2, e1, o1)
            } else {
                return None;
            };
            let side = |e: Edge, o: &str| -> Option<String> {
                let off = lp(o, c)?;
                Some(if matches!(e, Edge::Left | Edge::Top) { off } else { from_far_edge(&off) })
            };
            Some((side(xe, xo)?, side(ye, yo)?))
        }
        _ => None,
    }
}

/// Заданная запись `<position>`: слова в нижнем регистре, горизонтальная ось первой
/// (`bottom right` → `right bottom`, `bottom 10% right 20%` → `right 20% bottom 10%`).
fn specified_position_words(tokens: &[&str]) -> String {
    let w: Vec<String> = tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
    let swap = match tokens {
        [a, b] => matches!((edge(a), edge(b)), (Some(ea), Some(eb)) if is_vertical(ea) || is_horizontal(eb)),
        [a, _, b, _] => matches!((edge(a), edge(b)), (Some(ea), Some(eb)) if is_vertical(ea) && is_horizontal(eb)),
        _ => false,
    };
    match (swap, w.as_slice()) {
        (true, [a, b]) => format!("{b} {a}"),
        (true, [a, o1, b, o2]) => format!("{b} {o2} {a} {o1}"),
        _ => w.join(" "),
    }
}

/// `at <x> <y>`; центр (`50% 50%`) — значение по умолчанию и не пишется.
fn at_part(tokens: &[&str], c: &Ctx) -> Option<Option<String>> {
    let (x, y) = position(tokens, c)?;
    if c.specified {
        return Some(Some(format!("at {}", specified_position_words(tokens))));
    }
    Some(if x == "50%" && y == "50%" { None } else { Some(format!("at {x} {y}")) })
}

// ── градиенты ────────────────────────────────────────────────────────────────────────────────

/// `in <space> [<hue> hue]?` из списка токенов: остаток и запись метода (без `in`).
fn split_interp<'a>(tokens: &[&'a str]) -> Option<(Vec<&'a str>, Option<String>)> {
    let Some(i) = tokens.iter().position(|t| t.eq_ignore_ascii_case("in")) else {
        return Some((tokens.to_vec(), None));
    };
    let space = tokens.get(i + 1)?.to_ascii_lowercase();
    let polar = matches!(space.as_str(), "hsl" | "hwb" | "lch" | "oklch");
    let rect = matches!(
        space.as_str(),
        "srgb" | "srgb-linear" | "display-p3" | "display-p3-linear" | "a98-rgb" | "prophoto-rgb" | "rec2020" | "lab"
            | "oklab" | "xyz" | "xyz-d50" | "xyz-d65"
    );
    if !polar && !rect {
        return None;
    }
    let mut used = 2;
    let mut text = if space == "xyz" { "xyz-d65".to_string() } else { space };
    if polar
        && let (Some(h), Some(w)) = (tokens.get(i + 2), tokens.get(i + 3))
        && w.eq_ignore_ascii_case("hue")
    {
        let h = h.to_ascii_lowercase();
        if !matches!(h.as_str(), "shorter" | "longer" | "increasing" | "decreasing") {
            return None;
        }
        used = 4;
        if h != "shorter" {
            text = format!("{text} {h} hue");
        }
    }
    let mut rest = tokens[..i].to_vec();
    rest.extend_from_slice(&tokens[i + used..]);
    Some((rest, Some(text)))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Linear,
    Radial,
    Conic,
}

fn linear_config(tokens: &[&str], c: &Ctx) -> Option<Vec<String>> {
    match tokens {
        [] => Some(vec![]),
        [a] => Some(vec![angle(a, c)?]),
        [to, sides @ ..] if to.eq_ignore_ascii_case("to") && matches!(sides.len(), 1 | 2) => {
            let es: Vec<Edge> = sides.iter().map(|s| edge(s)).collect::<Option<_>>()?;
            if es.contains(&Edge::Center) || (es.len() == 2 && is_vertical(es[0]) == is_vertical(es[1])) {
                return None;
            }
            let words: Vec<String> = sides.iter().map(|s| s.to_ascii_lowercase()).collect();
            Some(vec![format!("to {}", words.join(" "))])
        }
        _ => None,
    }
}

fn radial_config(tokens: &[&str], c: &Ctx) -> Option<Vec<String>> {
    let mut shape: Option<String> = None;
    let mut size_kw: Option<String> = None;
    let mut lens: Vec<String> = Vec::new();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].to_ascii_lowercase();
        match t.as_str() {
            "at" => {
                if let Some(at) = at_part(&tokens[i + 1..], c)? {
                    parts.push(at);
                }
                i = tokens.len();
                continue;
            }
            "circle" | "ellipse" if shape.is_none() => shape = Some(t),
            "closest-side" | "closest-corner" | "farthest-side" | "farthest-corner"
                if size_kw.is_none() && lens.is_empty() =>
            {
                size_kw = Some(t)
            }
            _ if size_kw.is_none() && lens.len() < 2 => lens.push(lp(tokens[i], c)?),
            _ => return None,
        }
        i += 1;
    }
    let mut out = Vec::new();
    if !lens.is_empty() {
        let circle_ok = lens.len() == 1 && !lens[0].ends_with('%') && shape.as_deref() != Some("ellipse");
        let ellipse_ok = lens.len() == 2 && shape.as_deref() != Some("circle");
        if !circle_ok && !ellipse_ok {
            return None;
        }
        out.push(lens.join(" "));
    } else {
        out.extend(shape);
        out.extend(size_kw);
    }
    out.extend(parts);
    Some(vec![out.join(" ")].into_iter().filter(|s| !s.is_empty()).collect())
}

fn conic_config(tokens: &[&str], c: &Ctx) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0;
    if tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("from")) {
        out.push(format!("from {}", angle(tokens.get(1)?, c)?));
        i = 2;
    }
    if let Some(t) = tokens.get(i) {
        if !t.eq_ignore_ascii_case("at") {
            return None;
        }
        out.extend(at_part(&tokens[i + 1..], c)?);
    }
    Some(vec![out.join(" ")].into_iter().filter(|s| !s.is_empty()).collect())
}

/// Позиция остановки: `<length-percentage>`, у конического — ещё и `<angle>`.
fn stop_pos(tok: &str, kind: Kind, c: &Ctx) -> Option<String> {
    // Угол в `calc()` допустим только у конического градиента.
    if kind != Kind::Conic && tok.contains('(') {
        let l = tok.to_ascii_lowercase();
        if ["deg", "grad", "rad", "turn"].iter().any(|u| l.contains(u)) {
            return None;
        }
    }
    lp(tok, c).or_else(|| if kind == Kind::Conic { angle(tok, c) } else { None })
}

fn gradient(name: &str, inner: &str, c: &Ctx) -> Option<String> {
    let kind = if name.contains("linear") {
        Kind::Linear
    } else if name.contains("radial") {
        Kind::Radial
    } else {
        Kind::Conic
    };
    let args = split_top_level_commas(inner);
    let first = split_top_level_ws(args.first()?.trim());
    let has_config = color_text(first.first()?, c).is_none();
    let (config_tokens, stop_args) = if has_config { (first, &args[1..]) } else { (vec![], &args[..]) };
    let (rest, interp) = split_interp(&config_tokens)?;
    let mut config = match kind {
        Kind::Linear => linear_config(&rest, c)?,
        Kind::Radial => radial_config(&rest, c)?,
        Kind::Conic => conic_config(&rest, c)?,
    };
    let mut stops = Vec::new();
    let mut all_legacy = true;
    for arg in stop_args {
        let toks = split_top_level_ws(arg.trim());
        match toks.as_slice() {
            [] => return None,
            [one] => match color_text(one, c) {
                Some((col, legacy)) => {
                    all_legacy &= legacy;
                    stops.push(col);
                }
                None => stops.push(stop_pos(one, kind, c)?),
            },
            [col, pos @ ..] if pos.len() <= 2 => {
                let (col, legacy) = color_text(col, c)?;
                all_legacy &= legacy;
                let mut s = col;
                for p in pos {
                    s.push(' ');
                    s.push_str(&stop_pos(p, kind, c)?);
                }
                stops.push(s);
            }
            _ => return None,
        }
    }
    if stops.is_empty() {
        return None;
    }
    let default = if all_legacy { "srgb" } else { "oklab" };
    if let Some(m) = interp.filter(|m| m != default) {
        match config.first_mut() {
            Some(first) => {
                first.push_str(" in ");
                first.push_str(&m);
            }
            None => config.push(format!("in {m}")),
        }
    }
    config.extend(stops);
    Some(format!("{name}({})", config.join(", ")))
}

// ── calc() над разрешениями и процентами ────────────────────────────────────────────────────

/// Вычисляет `calc(…)`/голую размерность; `unit` переводит единицу в базовую (`%` → 1).
/// Размерность отслеживается: сумма требует одинаковых, итог обязан быть размерным значением
/// (`calc(3 * 4)`, `calc(2x - 1)` — `None`).
fn eval_calc(expr: &str, unit: fn(&str) -> Option<f64>) -> Option<f64> {
    /// Значение и степень размерности (0 — число, 1 — разрешение / процент).
    type Val = (f64, i8);
    struct P<'a> {
        s: &'a [u8],
        i: usize,
        unit: fn(&str) -> Option<f64>,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.s.get(self.i).is_some_and(|b| b.is_ascii_whitespace()) {
                self.i += 1;
            }
        }
        fn sum(&mut self) -> Option<Val> {
            let mut v = self.term()?;
            loop {
                self.ws();
                let sign = match self.s.get(self.i) {
                    Some(b'+') => 1.0,
                    Some(b'-') => -1.0,
                    _ => return Some(v),
                };
                self.i += 1;
                let r = self.term()?;
                if r.1 != v.1 {
                    return None;
                }
                v.0 += sign * r.0;
            }
        }
        fn term(&mut self) -> Option<Val> {
            let mut v = self.factor()?;
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some(b'*') => {
                        self.i += 1;
                        let r = self.factor()?;
                        v = (v.0 * r.0, v.1 + r.1);
                    }
                    Some(b'/') => {
                        self.i += 1;
                        let d = self.factor()?;
                        if d.0 == 0.0 || d.1 != 0 {
                            return None;
                        }
                        v.0 /= d.0;
                    }
                    _ => return Some(v),
                }
            }
        }
        fn factor(&mut self) -> Option<Val> {
            self.ws();
            if self.s[self.i..].len() >= 5 && self.s[self.i..self.i + 5].eq_ignore_ascii_case(b"calc(") {
                self.i += 4;
            }
            if self.s.get(self.i) == Some(&b'(') {
                self.i += 1;
                let v = self.sum()?;
                self.ws();
                (self.s.get(self.i) == Some(&b')')).then(|| self.i += 1)?;
                return Some(v);
            }
            let start = self.i;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            while self.s.get(self.i).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
                self.i += 1;
            }
            let num: f64 = std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()?;
            let ustart = self.i;
            while self.s.get(self.i).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'%') {
                self.i += 1;
            }
            let u = std::str::from_utf8(&self.s[ustart..self.i]).ok()?;
            if u.is_empty() {
                return Some((num, 0));
            }
            Some((num * (self.unit)(&u.to_ascii_lowercase())?, 1))
        }
    }
    let mut p = P { s: expr.trim().as_bytes(), i: 0, unit };
    let (v, dim) = p.sum()?;
    p.ws();
    (p.i == p.s.len() && dim == 1 && v.is_finite()).then_some(v)
}

fn resolution_unit(u: &str) -> Option<f64> {
    match u {
        "x" | "dppx" => Some(1.0),
        "dpi" => Some(1.0 / 96.0),
        "dpcm" => Some(2.54 / 96.0),
        _ => None,
    }
}

fn percent_unit(u: &str) -> Option<f64> {
    (u == "%").then_some(1.0)
}

// ── image-set(), cross-fade() ────────────────────────────────────────────────────────────────

/// Строка CSS (`'a'`/`"a"`) → её содержимое.
fn unquote(t: &str) -> Option<&str> {
    let b = t.as_bytes();
    let ok = b.len() >= 2 && matches!(b[0], b'"' | b'\'') && b[b.len() - 1] == b[0];
    if !ok {
        return None;
    }
    // Кавычка того же вида внутри допустима только после обратной косой черты.
    let inner = &b[1..b.len() - 1];
    let mut i = 0;
    while i < inner.len() {
        match inner[i] {
            b'\\' => i += 1,
            q if q == b[0] => return None,
            _ => {}
        }
        i += 1;
    }
    Some(&t[1..t.len() - 1])
}

/// `<resolution>` опции `image-set()`: значение в `dppx` и запись для заданного значения.
/// Простое значение неотрицательно; `calc()` может уйти в минус (в computed зажимается).
fn resolution(tok: &str) -> Option<(f64, String)> {
    let lower = tok.to_ascii_lowercase();
    if lower.starts_with("calc(") {
        let v = eval_calc(&lower, resolution_unit)?;
        return Some((v, format!("calc({}dppx)", fmt_num(v))));
    }
    let split = lower.find(|ch: char| ch.is_ascii_alphabetic())?;
    lower[..split].parse::<f64>().ok().filter(|n| *n >= 0.0)?;
    let v = eval_calc(&lower, resolution_unit)?;
    Some((v, lower))
}

fn image_set(inner: &str, c: &Ctx) -> Option<String> {
    let mut options = Vec::new();
    for opt in split_top_level_commas(inner) {
        let toks = split_top_level_ws(opt.trim());
        let (src, rest) = toks.split_first()?;
        let img = match unquote(src) {
            Some(path) => url_text(path),
            None => {
                let img = image(src, c)?;
                if img == "none" || img.starts_with("image-set(") {
                    return None;
                }
                img
            }
        };
        let mut res: Option<(f64, String)> = None;
        let mut ty: Option<String> = None;
        for t in rest {
            let lower = t.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("type(").and_then(|v| v.strip_suffix(')')) {
                let orig = &t[t.len() - v.len() - 1..t.len() - 1];
                if ty.is_some() {
                    return None;
                }
                ty = Some(format!("type(\"{}\")", unquote(orig.trim())?));
            } else if res.is_none() {
                res = Some(resolution(t)?);
            } else {
                return None;
            }
        }
        let (v, written) = res.unwrap_or((1.0, "1x".into()));
        let res_text = if c.specified { written } else { format!("{}dppx", fmt_num(v.max(0.0))) };
        let mut s = format!("{img} {res_text}");
        if let Some(ty) = ty {
            s.push(' ');
            s.push_str(&ty);
        }
        options.push(s);
    }
    (!options.is_empty()).then(|| format!("image-set({})", options.join(", ")))
}

/// Каноническая заданная запись `image-set()` / `-webkit-image-set()` для CSSOM.
/// `None` — запись недопустима либо `s` не `image-set()`.
pub(in crate::style) fn specified_image_set_css(s: &str) -> Option<String> {
    let t = s.trim();
    let open = t.find('(')?;
    let name = t[..open].to_ascii_lowercase();
    if !matches!(name.as_str(), "image-set" | "-webkit-image-set") || !t.ends_with(')') {
        return None;
    }
    let c = Ctx { em: 16.0, viewport: Size::new(0.0, 0.0), specified: true };
    image_set(&t[open + 1..t.len() - 1], &c)
}

fn cross_fade(inner: &str, c: &Ctx) -> Option<String> {
    let mut items = Vec::new();
    for arg in split_top_level_commas(inner) {
        let toks = split_top_level_ws(arg.trim());
        let mut pct: Option<f64> = None;
        let mut img: Option<String> = None;
        for t in &toks {
            let is_pct = t.ends_with('%') || (t.to_ascii_lowercase().starts_with("calc(") && t.contains('%'));
            if is_pct && pct.is_none() {
                pct = Some(eval_calc(t, percent_unit)?.clamp(0.0, 100.0));
            } else if img.is_none() {
                img = Some(image(t, c).or_else(|| color_text(t, c).map(|(s, _)| s))?);
            } else {
                return None;
            }
        }
        let mut s = img?;
        if let Some(p) = pct {
            s.push_str(&format!(" {}%", fmt_num(p)));
        }
        items.push(s);
    }
    (items.len() >= 2).then(|| format!("cross-fade({})", items.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn css(s: &str) -> Option<String> {
        computed_image_css(s, 16.0, Size::new(800.0, 600.0))
    }

    #[test]
    fn linear_plain() {
        assert_eq!(
            css("linear-gradient(red, blue)").as_deref(),
            Some("linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("linear-gradient(to right bottom, red 10px, blue 50%)").as_deref(),
            Some("linear-gradient(to right bottom, rgb(255, 0, 0) 10px, rgb(0, 0, 255) 50%)")
        );
    }

    #[test]
    fn interpolation_default_omitted() {
        assert_eq!(
            css("linear-gradient(30deg in srgb, red, blue)").as_deref(),
            Some("linear-gradient(30deg, rgb(255, 0, 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("linear-gradient(in oklab 30deg, red, blue)").as_deref(),
            Some("linear-gradient(30deg in oklab, rgb(255, 0, 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("linear-gradient(in oklab, color(srgb 1 0 0), blue)").as_deref(),
            Some("linear-gradient(color(srgb 1 0 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("conic-gradient(in hsl shorter hue, red, blue)").as_deref(),
            Some("conic-gradient(in hsl, rgb(255, 0, 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("conic-gradient(in hsl longer hue, red, blue)").as_deref(),
            Some("conic-gradient(in hsl longer hue, rgb(255, 0, 0), rgb(0, 0, 255))")
        );
        assert_eq!(
            css("conic-gradient(from 30deg in xyz, red, blue)").as_deref(),
            Some("conic-gradient(from 30deg in xyz-d65, rgb(255, 0, 0), rgb(0, 0, 255))")
        );
    }

    #[test]
    fn radial_and_conic_geometry() {
        let st = "rgb(255, 0, 0), rgb(0, 0, 255)";
        assert_eq!(
            css("radial-gradient(ellipse 50% 40em at right center, red, blue)").as_deref(),
            Some(format!("radial-gradient(50% 640px at 100% 50%, {st})").as_str())
        );
        assert_eq!(css("radial-gradient(at center, red, blue)").as_deref(), Some(format!("radial-gradient({st})").as_str()));
        assert_eq!(css("radial-gradient(circle closest-side, red, blue)").as_deref(), Some(format!("radial-gradient(circle closest-side, {st})").as_str()));
        assert_eq!(
            css("conic-gradient(at right 10px top 20px, red, blue)").as_deref(),
            Some(format!("conic-gradient(at calc(100% - 10px) 20px, {st})").as_str())
        );
        assert_eq!(
            css("conic-gradient(at bottom 10% right 20%, red, blue)").as_deref(),
            Some(format!("conic-gradient(at 80% 90%, {st})").as_str())
        );
        assert_eq!(
            css("conic-gradient(at left 10px top 50em, red, blue)").as_deref(),
            Some(format!("conic-gradient(at 10px 800px, {st})").as_str())
        );
        assert_eq!(css("conic-gradient(at 30%, red, blue)").as_deref(), Some(format!("conic-gradient(at 30% 50%, {st})").as_str()));
        assert_eq!(css("conic-gradient(at top 5px right 10px, red, blue)").as_deref(), Some(format!("conic-gradient(at calc(100% - 10px) 5px, {st})").as_str()));
    }

    #[test]
    fn image_set_dppx() {
        assert_eq!(
            css("image-set(url(a.png) 1x, 'b.png' 192dpi)").as_deref(),
            Some("image-set(url(\"a.png\") 1dppx, url(\"b.png\") 2dppx)")
        );
        assert_eq!(
            css("-webkit-image-set(url(a.png) type('image/png'))").as_deref(),
            Some("image-set(url(\"a.png\") 1dppx type(\"image/png\"))")
        );
        assert_eq!(
            css("image-set(url(a.png) calc(6dppx / 3))").as_deref(),
            Some("image-set(url(\"a.png\") 2dppx)")
        );
        assert_eq!(
            css("image-set(url(a.png) calc(-1 * 1x))").as_deref(),
            Some("image-set(url(\"a.png\") 0dppx)")
        );
        assert_eq!(
            css("image-set(linear-gradient(black, white) 1x)").as_deref(),
            Some("image-set(linear-gradient(rgb(0, 0, 0), rgb(255, 255, 255)) 1dppx)")
        );
    }

    #[test]
    fn cross_fade_percentages() {
        assert_eq!(
            css("cross-fade(50% red, 50% green)").as_deref(),
            Some("cross-fade(rgb(255, 0, 0) 50%, rgb(0, 128, 0) 50%)")
        );
        assert_eq!(
            css("cross-fade(calc(101%) red, green)").as_deref(),
            Some("cross-fade(rgb(255, 0, 0) 100%, rgb(0, 128, 0))")
        );
        assert_eq!(
            css("cross-fade(url(a.png), url(b.png))").as_deref(),
            Some("cross-fade(url(\"a.png\"), url(\"b.png\"))")
        );
    }

    #[test]
    fn specified_gradient_canonical_and_invalid() {
        let sp = specified_gradient_css;
        assert_eq!(sp("linear-gradient(in lab 30deg, red, blue)").as_deref(), Some("linear-gradient(30deg in lab, red, blue)"));
        assert_eq!(sp("linear-gradient(30deg in srgb, red, blue)").as_deref(), Some("linear-gradient(30deg, red, blue)"));
        assert_eq!(sp("radial-gradient(ellipse 50% 40em in xyz, red, blue)").as_deref(), Some("radial-gradient(50% 40em in xyz-d65, red, blue)"));
        assert_eq!(
            sp("conic-gradient(at left 10px top 50em in oklab, color(srgb 1 0 0), blue)").as_deref(),
            Some("conic-gradient(at left 10px top 50em, color(srgb 1 0 0), blue)")
        );
        assert_eq!(sp("linear-gradient(to right in hsl longer hue, red, 50%, blue)").as_deref(), Some("linear-gradient(to right in hsl longer hue, red, 50%, blue)"));
        for bad in [
            "linear-gradient(, red, blue)",
            "linear-gradient(red, blue, lab)",
            "linear-gradient(lab lab, red, blue)",
            "linear-gradient(lab shorter hue, red, blue)",
            "linear-gradient(in, red, blue)",
            "linear-gradient(in 45deg, red, blue)",
            "linear-gradient(90deg in hsl longer, red, blue)",
            "conic-gradient(in from 90deg, red, blue)",
            "radial-gradient(closest-side in, red, blue)",
        ] {
            assert_eq!(sp(bad), None, "{bad}");
        }
    }

    #[test]
    fn calc_positions_and_canonical_order() {
        let sp = specified_gradient_css;
        assert_eq!(
            sp("conic-gradient(red calc(0deg + 100%), blue)").as_deref(),
            Some("conic-gradient(red calc(100% + 0deg), blue)")
        );
        assert_eq!(sp("linear-gradient(red calc(50% + 30deg), blue)"), None);
        assert_eq!(sp("conic-gradient(from calc(50% + 30deg), red, blue)"), None);
        assert_eq!(
            sp("radial-gradient(at bottom 10% right 20%, red, blue)").as_deref(),
            Some("radial-gradient(at right 20% bottom 10%, red, blue)")
        );
        assert_eq!(
            sp("radial-gradient(at center left, red, blue)").as_deref(),
            Some("radial-gradient(at left center, red, blue)")
        );
        // computed-запись с `calc()` читается обратно (round-trip WPT).
        assert_eq!(
            css("conic-gradient(at right 10px top 20px, red, blue)").as_deref().and_then(css),
            css("conic-gradient(at right 10px top 20px, red, blue)")
        );
    }

    #[test]
    fn specified_image_set_canonical_and_invalid() {
        let sp = specified_image_set_css;
        assert_eq!(
            sp("-webkit-image-set(url(a.png) 1dpcm, 'b.png' 2x, url(c))").as_deref(),
            Some("image-set(url(\"a.png\") 1dpcm, url(\"b.png\") 2x, url(\"c\") 1x)")
        );
        assert_eq!(
            sp("image-set(url(a.png) type('image/png') 1x)").as_deref(),
            Some("image-set(url(\"a.png\") 1x type(\"image/png\"))")
        );
        assert_eq!(sp("image-set(url(a.png) calc(2x * 3))").as_deref(), Some("image-set(url(\"a.png\") calc(6dppx))"));
        assert_eq!(
            sp("image-set(url(a.png) calc(1dpcm * 96/2.54))").as_deref(),
            Some("image-set(url(\"a.png\") calc(1dppx))")
        );
        assert_eq!(sp("image-set(url(a.png) calc(-1 * 1x))").as_deref(), Some("image-set(url(\"a.png\") calc(-1dppx))"));
        for bad in [
            "image-set()",
            "image-set(none, url(a.png) 1x)",
            "image-set(url(a.png) -1x)",
            "image-set(url(a.png) 1x 2x)",
            "image-set(url(a.png) 1x url(b.png))",
            "image-set(url(a.png) 1invalid)",
            "image-set(url(a.png) calc(3 * 4))",
            "image-set(url(a.png) calc(2x - 1))",
            "image-set(url(a.png) type(image/png))",
            "image-set(url(a.png) type('a') type('b'))",
            "image-set(url(a.png) type('a' 'b'))",
            "image-set(type('a') url(a.png) 1x)",
            "image-set(image-set(url(a.png)) 2x)",
            "image-set(url(a.png) 1xtype('a'))",
        ] {
            assert_eq!(sp(bad), None, "{bad}");
        }
    }

    /// BUG-1433: модификаторы запроса не попадают в URL; computed — в каноническом порядке.
    #[test]
    fn url_request_modifiers() {
        assert_eq!(css("url('g.png' referrer-policy(origin))").as_deref(), Some("url(\"g.png\" referrer-policy(origin))"));
        assert_eq!(
            css("url(\"a.png\" referrer-policy(no-referrer) integrity(\"sha384-x\") cross-origin(anonymous))").as_deref(),
            Some("url(\"a.png\" cross-origin(anonymous) integrity(\"sha384-x\") referrer-policy(no-referrer))")
        );
        assert_eq!(css("url(\"a.png\" integrity(\"\"))").as_deref(), Some("url(\"a.png\" integrity(\"\"))"));
        // неизвестный модификатор игнорируется
        assert_eq!(css("url(\"a.png\" foobar(baz))").as_deref(), Some("url(\"a.png\")"));
        for bad in [
            "url(\"a.png\" cross-origin())",
            "url(\"a.png\" cross-origin(anonymous foobar))",
            "url(\"a.png\" cross-origin(anonymous) cross-origin(use-credentials))",
            "url(\"a.png\" integrity(sha384-foobar))",
            "url(\"a.png\" integrity(\"a\",))",
            "url(\"a.png\" referrer-policy(no-referrer same-origin))",
            "url(\"a.png\" foobar foobar)",
            "url(\"a.png\" 42)",
            "url(\"a.png\" \"foobar\")",
            "url(a.png cross-origin(anonymous))",
        ] {
            assert_eq!(css(bad), None, "{bad}");
        }
    }
}
