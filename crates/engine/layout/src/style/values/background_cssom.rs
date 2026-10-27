//! Грамматика CSS Backgrounds для inline-`style` CSSOM (BUG-1297).
//!
//! `element.style.<p> = v` проверяет значение по грамматике свойства и хранит каноническую
//! запись (CSSOM §serialize a CSS value); недопустимое значение отбрасывается. Покрыто:
//! `background-{image,size,repeat,attachment,clip,origin,position,position-x,position-y}`,
//! `box-shadow`, `border-radius` и четыре `border-*-radius`. Вызывается из JS-шима
//! (`_lumen_css_canonical_background`); каскад этим кодом не пользуется (его разборщики мягкие).

use super::length::split_top_level_ws;
use super::text_cssom::canonical_length_sorted;
use crate::style::canonical_specified_color;
use crate::style::parse::image_computed::{specified_gradient_css, specified_image_set_css};

/// Каноническая запись `value` свойства `prop` либо `None`, если значение не по грамматике
/// (присваивание игнорируется). `None` и для свойства, которого здесь нет.
pub fn canonical_specified_background(prop: &str, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match prop {
        "background-image" => list(v, image),
        "background-size" => list(v, size),
        "background-repeat" => list(v, repeat),
        "background-attachment" => list(v, |s| one_of(s, &["scroll", "fixed", "local"])),
        "background-origin" => list(v, |s| one_of(s, BOXES)),
        "background-clip" => list(v, clip),
        "background-position" => list(v, position),
        "background-position-x" => list(v, |s| axis(s, false)),
        "background-position-y" => list(v, |s| axis(s, true)),
        "box-shadow" => {
            if v.eq_ignore_ascii_case("none") {
                Some("none".into())
            } else {
                list(v, shadow)
            }
        }
        "border-radius" => border_radius(v),
        p if p.starts_with("border-image") => super::border_image::canonical_specified_border_image(p, v),
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius" => radius_pair(v),
        _ => None,
    }
}

const BOXES: &[&str] = &["border-box", "padding-box", "content-box"];

/// Список через запятую: каждый элемент обязан пройти `item`, пустой элемент недопустим.
fn list(v: &str, item: impl Fn(&str) -> Option<String>) -> Option<String> {
    let parts: Vec<String> = crate::style::split_top_level_commas(v)
        .into_iter()
        .map(|p| item(p.trim()))
        .collect::<Option<_>>()?;
    Some(parts.join(", "))
}

fn one_of(s: &str, words: &[&str]) -> Option<String> {
    let l = s.to_ascii_lowercase();
    words.contains(&l.as_str()).then_some(l)
}

/// `<length-percentage>` (при `non_negative` — `[0,∞]`), без `auto`.
fn lp(tok: &str, non_negative: bool) -> Option<String> {
    canonical_length_sorted(tok, false, non_negative)
}

pub(super) fn canonical_specified_image(s: &str) -> Option<String> {
    image(s)
}

fn image(s: &str) -> Option<String> {
    if s.eq_ignore_ascii_case("none") {
        return Some("none".into());
    }
    // `<image>` — всегда функция (`url()`, `*-gradient()`, `image-set()`, …): голое слово
    // (`auto`, `red`) значением не является.
    let open = s.find('(')?;
    let name = &s[..open];
    let ident = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !(ident && s.ends_with(')')) {
        return None;
    }
    // BUG-1428: градиент проверяется по грамматике и пишется канонически (`in <space>` после
    // направления). `var()`/`env()`/`attr()`/`sign()` разбор не берёт — такая запись остаётся как есть.
    if let Some(c) = specified_gradient_css(s).or_else(|| specified_image_set_css(s)) {
        return Some(c);
    }
    let lower = s.to_ascii_lowercase();
    let name_lower = name.to_ascii_lowercase();
    // BUG-1551: `url(a)` / `url('a')` → `url("a")`.
    if name_lower == "url" {
        return Some(super::misc_cssom::canonical_url(s).unwrap_or_else(|| s.to_string()));
    }
    let is_gradient = (name_lower.ends_with("-gradient") && !name.starts_with('-'))
        || matches!(name_lower.as_str(), "image-set" | "-webkit-image-set");
    let opaque = ["var(", "env(", "attr(", "sign("].iter().any(|f| lower.contains(f));
    (!is_gradient || opaque).then(|| s.to_string())
}

/// `contain | cover | [ <length-percentage [0,∞]> | auto ]{1,2}`.
fn size(s: &str) -> Option<String> {
    let l = s.to_ascii_lowercase();
    if l == "cover" || l == "contain" {
        return Some(l);
    }
    let toks = split_top_level_ws(s);
    if toks.is_empty() || toks.len() > 2 {
        return None;
    }
    let comp = |t: &str| -> Option<String> {
        if t.eq_ignore_ascii_case("auto") { Some("auto".into()) } else { lp(t, true) }
    };
    let w = comp(toks[0])?;
    let h = match toks.get(1) {
        Some(t) => comp(t)?,
        None => "auto".into(),
    };
    Some(if w == "auto" && h == "auto" { "auto".into() } else { format!("{w} {h}") })
}

/// `repeat-x | repeat-y | [ repeat | space | round | no-repeat ]{1,2}`.
fn repeat(s: &str) -> Option<String> {
    const R: &[&str] = &["repeat", "space", "round", "no-repeat"];
    let toks: Vec<String> = s.split_whitespace().map(str::to_ascii_lowercase).collect();
    match toks.as_slice() {
        [t] if t == "repeat-x" || t == "repeat-y" => Some(t.clone()),
        [t] if R.contains(&t.as_str()) => Some(t.clone()),
        [a, b] if R.contains(&a.as_str()) && R.contains(&b.as_str()) => {
            Some(match (a.as_str(), b.as_str()) {
                _ if a == b => a.clone(),
                ("repeat", "no-repeat") => "repeat-x".into(),
                ("no-repeat", "repeat") => "repeat-y".into(),
                _ => format!("{a} {b}"),
            })
        }
        _ => None,
    }
}

/// `<bg-clip> = <visual-box> | border-area | text`, `border-area text` — в любом порядке.
fn clip(s: &str) -> Option<String> {
    let toks: Vec<String> = s.split_whitespace().map(str::to_ascii_lowercase).collect();
    match toks.as_slice() {
        [t] if BOXES.contains(&t.as_str()) || t == "text" || t == "border-area" => Some(t.clone()),
        [a, b] if (a == "text" && b == "border-area") || (a == "border-area" && b == "text") => {
            Some("border-area text".into())
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn parse(t: &str) -> Option<Side> {
        match t {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            "top" => Some(Side::Top),
            "bottom" => Some(Side::Bottom),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
    fn horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// Одна ось `<position>`: `center`, длина или (сторона + необязательное смещение).
#[derive(Clone)]
enum Comp {
    Center,
    Len(String),
    Edge(Side, Option<String>),
}

impl Comp {
    fn text(&self) -> String {
        match self {
            Comp::Center => "center".into(),
            Comp::Len(l) => l.clone(),
            Comp::Edge(s, None) => s.name().into(),
            Comp::Edge(s, Some(o)) => format!("{} {o}", s.name()),
        }
    }
}

/// `<position>` целиком: 1–4 значения (CSS Values 4 §6.4.1) → `x y` в каноническом порядке.
fn position(s: &str) -> Option<String> {
    let toks: Vec<String> = split_top_level_ws(s).iter().map(|t| t.to_ascii_lowercase()).collect();
    let is_kw = |t: &str| t == "center" || Side::parse(t).is_some();
    let (x, y) = match toks.len() {
        1 => {
            let t = &toks[0];
            match Side::parse(t) {
                Some(sd) if sd.horizontal() => (Comp::Edge(sd, None), Comp::Center),
                Some(sd) => (Comp::Center, Comp::Edge(sd, None)),
                None if t == "center" => (Comp::Center, Comp::Center),
                None => (Comp::Len(lp(t, false)?), Comp::Center),
            }
        }
        2 => {
            let (a, b) = (&toks[0], &toks[1]);
            if is_kw(a) && is_kw(b) {
                keyword_pair(a, b)?
            } else {
                // Хотя бы одна длина: первая — по X, вторая — по Y, стороны на своих осях.
                let x = match Side::parse(a) {
                    Some(sd) if sd.horizontal() => Comp::Edge(sd, None),
                    Some(_) => return None,
                    None if a == "center" => Comp::Center,
                    None => Comp::Len(lp(a, false)?),
                };
                let y = match Side::parse(b) {
                    Some(sd) if !sd.horizontal() => Comp::Edge(sd, None),
                    Some(_) => return None,
                    None if b == "center" => Comp::Center,
                    None => Comp::Len(lp(b, false)?),
                };
                (x, y)
            }
        }
        3 | 4 => {
            let groups = edge_groups(&toks)?;
            let [g1, g2] = groups.as_slice() else { return None };
            // Группа — `center` либо `сторона [смещение]`; оси различаются по стороне.
            let (a, b) = (g1.clone(), g2.clone());
            match (&a, &b) {
                (Comp::Center, Comp::Center) => return None,
                (Comp::Center, Comp::Edge(sd, _)) => {
                    if sd.horizontal() { (b, a) } else { (a, b) }
                }
                (Comp::Edge(sd, _), Comp::Center) => {
                    if sd.horizontal() { (a, b) } else { (b, a) }
                }
                (Comp::Edge(s1, _), Comp::Edge(s2, _)) => {
                    if s1.horizontal() == s2.horizontal() {
                        return None;
                    }
                    if s1.horizontal() { (a, b) } else { (b, a) }
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(format!("{} {}", x.text(), y.text()))
}

/// Две ключевые позиции в любом порядке: `left top`, `top left`, `center right`, …
fn keyword_pair(a: &str, b: &str) -> Option<(Comp, Comp)> {
    let ka = Side::parse(a);
    let kb = Side::parse(b);
    match (ka, kb) {
        (Some(p), Some(q)) => match (p.horizontal(), q.horizontal()) {
            (true, false) => Some((Comp::Edge(p, None), Comp::Edge(q, None))),
            (false, true) => Some((Comp::Edge(q, None), Comp::Edge(p, None))),
            _ => None,
        },
        (Some(p), None) => Some(if p.horizontal() {
            (Comp::Edge(p, None), Comp::Center)
        } else {
            (Comp::Center, Comp::Edge(p, None))
        }),
        (None, Some(q)) => Some(if q.horizontal() {
            (Comp::Edge(q, None), Comp::Center)
        } else {
            (Comp::Center, Comp::Edge(q, None))
        }),
        (None, None) => Some((Comp::Center, Comp::Center)),
    }
}

/// Делит 3–4 токена на две группы `center | сторона [смещение]`; ровно две группы.
fn edge_groups(toks: &[String]) -> Option<Vec<Comp>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if t == "center" {
            out.push(Comp::Center);
            i += 1;
            continue;
        }
        let sd = Side::parse(t)?;
        match toks.get(i + 1) {
            Some(n) if n != "center" && Side::parse(n).is_none() => {
                out.push(Comp::Edge(sd, Some(lp(n, false)?)));
                i += 2;
            }
            _ => {
                out.push(Comp::Edge(sd, None));
                i += 1;
            }
        }
    }
    (out.len() == 2).then_some(out)
}

/// Элемент `background-position-x`/`-y`: `center | [ <side> ]? <length-percentage>?`, где
/// сторона — `left|right|x-start|x-end` (или `top|bottom|y-start|y-end` для `vertical`).
fn axis(s: &str, vertical: bool) -> Option<String> {
    let toks: Vec<String> = split_top_level_ws(s).iter().map(|t| t.to_ascii_lowercase()).collect();
    let sides: [&str; 4] = if vertical {
        ["top", "bottom", "y-start", "y-end"]
    } else {
        ["left", "right", "x-start", "x-end"]
    };
    let is_side = |t: &str| sides.contains(&t);
    match toks.as_slice() {
        [t] if t == "center" || is_side(t) => Some(t.clone()),
        [t] => lp(t, false),
        [sd, off] if is_side(sd) => Some(format!("{sd} {}", lp(off, false)?)),
        _ => None,
    }
}

/// Одна тень `box-shadow`: `<color>? && <length>{2,4} && inset?` — длины подряд, цвет и
/// `inset` до или после них. Канонически: `<color> <длины> inset`.
fn shadow(s: &str) -> Option<String> {
    let toks = split_top_level_ws(s);
    let mut color: Option<String> = None;
    let mut inset = false;
    let mut lens: Vec<String> = Vec::new();
    let mut first_len = None;
    let mut last_len = None;
    for (i, t) in toks.iter().enumerate() {
        if t.eq_ignore_ascii_case("inset") {
            if inset {
                return None;
            }
            inset = true;
        } else if let Some(l) = shadow_length(t, lens.len() == 2) {
            // Отрицательный размытия (третья длина) отвергается `shadow_length`.
            first_len.get_or_insert(i);
            last_len = Some(i);
            lens.push(l);
        } else {
            let c = canonical_specified_color(t).filter(|c| !is_css_wide(c))?;
            if color.replace(c).is_some() {
                return None;
            }
        }
    }
    let (first, last) = (first_len?, last_len?);
    // Между длинами не должно быть ни цвета, ни `inset`; число длин — 2…4.
    if last - first + 1 != lens.len() || !(2..=4).contains(&lens.len()) {
        return None;
    }
    let mut out = String::new();
    if let Some(c) = color {
        out.push_str(&c);
        out.push(' ');
    }
    out.push_str(&lens.join(" "));
    if inset {
        out.push_str(" inset");
    }
    Some(out)
}

fn is_css_wide(c: &str) -> bool {
    matches!(c, "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "revert-rule")
}

/// Длина тени: без процентов; третья (радиус размытия) неотрицательна.
fn shadow_length(tok: &str, blur: bool) -> Option<String> {
    if tok.contains('%') {
        return None;
    }
    canonical_length_sorted(tok, false, blur)
}

/// `border-*-radius`: `<length-percentage [0,∞]>{1,2}`; равные значения схлопываются.
fn radius_pair(v: &str) -> Option<String> {
    let toks = split_top_level_ws(v);
    if toks.is_empty() || toks.len() > 2 {
        return None;
    }
    let a = lp(toks[0], true)?;
    let b = match toks.get(1) {
        Some(t) => lp(t, true)?,
        None => a.clone(),
    };
    Some(if a == b { a } else { format!("{a} {b}") })
}

/// `border-radius: <lp [0,∞]>{1,4} [ / <lp [0,∞]>{1,4} ]?`.
fn border_radius(v: &str) -> Option<String> {
    let mut halves = v.split('/');
    let h = radius_side(halves.next()?)?;
    let vert = match halves.next() {
        Some(p) => radius_side(p)?,
        None => h.clone(),
    };
    if halves.next().is_some() {
        return None;
    }
    fn refs(a: &[String; 4]) -> [&str; 4] {
        [a[0].as_str(), a[1].as_str(), a[2].as_str(), a[3].as_str()]
    }
    Some(serialize_border_radius(refs(&h), refs(&vert)))
}

/// Одна сторона `/`: 1–4 значения, раскрытые правилом margin до четырёх углов.
fn radius_side(s: &str) -> Option<[String; 4]> {
    let toks = split_top_level_ws(s);
    if toks.is_empty() || toks.len() > 4 {
        return None;
    }
    let vals: Vec<String> = toks.iter().map(|t| lp(t, true)).collect::<Option<_>>()?;
    let at = |i: usize| vals[i].clone();
    Some(match vals.len() {
        1 => [at(0), at(0), at(0), at(0)],
        2 => [at(0), at(1), at(0), at(1)],
        3 => [at(0), at(1), at(2), at(1)],
        _ => [at(0), at(1), at(2), at(3)],
    })
}

/// Запись `border-radius` по восьми радиусам углов (`top-left, top-right, bottom-right,
/// bottom-left`; горизонтальные и вертикальные): каждая сторона укорачивается как у
/// `margin`, часть после `/` опускается, когда совпадает с горизонтальной.
pub fn serialize_border_radius(h: [&str; 4], v: [&str; 4]) -> String {
    fn short(a: [&str; 4]) -> String {
        let mut n = 4;
        if a[3] == a[1] {
            n = 3;
            if a[2] == a[0] {
                n = 2;
                if a[1] == a[0] {
                    n = 1;
                }
            }
        }
        a[..n].join(" ")
    }
    if h == v { short(h) } else { format!("{} / {}", short(h), short(v)) }
}

#[cfg(test)]
mod tests {
    use super::canonical_specified_background as c;

    #[test]
    fn size_repeat_clip() {
        assert_eq!(c("background-size", "1px").as_deref(), Some("1px auto"));
        assert_eq!(c("background-size", "auto auto").as_deref(), Some("auto"));
        assert_eq!(c("background-size", "auto 1px, 2% 3%, contain").as_deref(), Some("auto 1px, 2% 3%, contain"));
        assert_eq!(c("background-size", "-1px"), None);
        assert_eq!(c("background-size", "1px 2px 3px"), None);
        assert_eq!(c("background-repeat", "repeat repeat").as_deref(), Some("repeat"));
        assert_eq!(c("background-repeat", "repeat no-repeat").as_deref(), Some("repeat-x"));
        assert_eq!(c("background-repeat", "repeat repeat-x"), None);
        assert_eq!(c("background-repeat", "repeat space round"), None);
        assert_eq!(c("background-clip", "text border-area").as_deref(), Some("border-area text"));
        assert_eq!(c("background-clip", "fill-box"), None);
        assert_eq!(c("background-attachment", "local, none"), None);
        assert_eq!(c("background-attachment", "scroll, fixed").as_deref(), Some("scroll, fixed"));
        assert_eq!(c("background-origin", "margin-box"), None);
    }

    #[test]
    fn position_forms() {
        assert_eq!(c("background-position", "center left").as_deref(), Some("left center"));
        assert_eq!(c("background-position", "top left").as_deref(), Some("left top"));
        assert_eq!(c("background-position", "bottom").as_deref(), Some("center bottom"));
        assert_eq!(c("background-position", "5% top").as_deref(), Some("5% top"));
        assert_eq!(c("background-position", "top 17px right 18px").as_deref(), Some("right 18px top 17px"));
        assert_eq!(c("background-position", "bottom 16% left").as_deref(), Some("left bottom 16%"));
        assert_eq!(c("background-position", "center right 7%").as_deref(), Some("right 7% center"));
        assert_eq!(c("background-position", "left right"), None);
        assert_eq!(c("background-position", "1% center 2px"), None);
        assert_eq!(c("background-position", "right 7% 50%"), None);
        assert_eq!(c("background-position", "50% top 8px"), None);
        assert_eq!(c("background-position-x", "left -20%").as_deref(), Some("left -20%"));
        assert_eq!(c("background-position-x", "left, center right"), None);
        assert_eq!(c("background-position-x", "top"), None);
        assert_eq!(c("background-position-y", "x-start"), None);
        assert_eq!(c("background-position-y", "bottom 10px").as_deref(), Some("bottom 10px"));
        assert_eq!(c("background-position-x", "calc(10px - 0.5em)").as_deref(), Some("calc(-0.5em + 10px)"));
        assert_eq!(c("background-position-x", "calc(10px - 0.5em)").as_deref(), Some("calc(-0.5em + 10px)"));
    }

    #[test]
    fn box_shadow() {
        assert_eq!(c("box-shadow", "inset 1px 2px red").as_deref(), Some("red 1px 2px inset"));
        assert_eq!(c("box-shadow", "inset 1px -2px, -3px 4px red").as_deref(), Some("1px -2px inset, red -3px 4px"));
        assert_eq!(c("box-shadow", "-4px 4px 0 0 green").as_deref(), Some("green -4px 4px 0px 0px"));
        assert_eq!(c("box-shadow", "none").as_deref(), Some("none"));
        assert_eq!(c("box-shadow", "1px 1px calc(1em - 2px)").as_deref(), Some("1px 1px calc(1em - 2px)"));
        for bad in [
            "auto", "1 2", "1% 2%", "1px calc(2px + 2%)", "1px", "1px 2px 3px 4px 5px", "red 1px 2px blue",
            "red", "-4px red 4px", "inset", "4px inset -4px", "inset 4px -4px inset", "4px -4px 0 inset 0",
            "1px 1px -1px",
        ] {
            assert_eq!(c("box-shadow", bad), None, "{bad}");
        }
    }

    #[test]
    fn border_radius() {
        assert_eq!(c("border-radius", "1px 1px 1px 2% / 1px 2% 1px 2%").as_deref(), Some("1px 1px 1px 2% / 1px 2%"));
        assert_eq!(c("border-radius", "1px 1px 1px 1px / 1px 1px 2% 1px").as_deref(), Some("1px / 1px 1px 2%"));
        assert_eq!(c("border-radius", "1px 2% 2% 2% / 1px 2% 3px 2%").as_deref(), Some("1px 2% 2% / 1px 2% 3px"));
        assert_eq!(c("border-radius", "5em / 1px 2% 3px 4%").as_deref(), Some("5em / 1px 2% 3px 4%"));
        for bad in ["auto", "none", "1px 2px 3px 4px 5px", "-1px", "1px -2px", "1em /", "1px / 2px / 3px", "4 / 5"] {
            assert_eq!(c("border-radius", bad), None, "{bad}");
        }
        assert_eq!(c("border-top-left-radius", "10px 20px 30px"), None);
        assert_eq!(c("border-top-left-radius", "40px 40px").as_deref(), Some("40px"));
    }

    #[test]
    fn image_needs_function() {
        assert_eq!(c("background-image", "none, auto"), None);
        assert_eq!(c("background-image", "red"), None);
        assert_eq!(c("background-image", "url(a.png), none").as_deref(), Some("url(\"a.png\"), none"));
    }
}
