//! CSS Backgrounds L3 §6 — `border-image` и пять его лонгхендов.
//!
//! Значение лежит в `ComputedStyle::border_image` одной структурой: шорткат ставит все
//! пять частей разом, лонгхенд меняет одну. Длины `border-image-width`/`-outset` приводятся к
//! px при применении декларации; проценты `border-image-width` и числа остаются как есть —
//! их база (размер области border-image, ширина рамки) известна только эмиттеру.
//!
//! Тот же разбор обслуживает `el.style.borderImage*` ([`canonical_specified_border_image`]) и
//! запись computed-значения ([`BorderImage::longhand_css`], [`BorderImage::shorthand_css`]).

use std::sync::Arc;

use lumen_core::geom::Size;

use crate::style::parse::image::{
    is_gradient_function, is_image_set_value, parse_background_gradient, parse_cross_fade, parse_paint_function,
    parse_url_value,
};
use crate::style::parse::image_computed::computed_image_css;
use crate::style::values::length::split_top_level_ws;
use crate::style::{BackgroundImage, Length, parse_length_q};

/// Одна сторона `border-image-slice`: число — пиксели картинки, процент — доля её размера.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageSlice {
    Number(f32),
    Percent(f32),
}

/// Одна сторона `border-image-width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageWidth {
    /// Множитель ширины соответствующей стороны `border-width`.
    Number(f32),
    /// Длина, уже в px.
    Length(f32),
    /// Доля размера области border-image по этой оси.
    Percent(f32),
    /// Размер соответствующего среза картинки.
    Auto,
}

/// Одна сторона `border-image-outset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageOutset {
    /// Множитель ширины соответствующей стороны `border-width`.
    Number(f32),
    /// Длина, уже в px.
    Length(f32),
}

/// `border-image-repeat` по одной оси.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderImageRepeat {
    #[default]
    Stretch,
    Repeat,
    Round,
    Space,
}

/// Значение `border-image`: источник и четыре геометрических параметра.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderImage {
    pub source: BackgroundImage,
    /// Computed-запись источника (градиент / `image-set()` / `cross-fade()`) — для `getComputedStyle`.
    pub source_css: Option<Arc<str>>,
    /// Срезы в порядке top, right, bottom, left.
    pub slice: [BorderImageSlice; 4],
    /// Ключевое слово `fill` у `border-image-slice`: средняя клетка тоже рисуется.
    pub fill: bool,
    pub width: [BorderImageWidth; 4],
    pub outset: [BorderImageOutset; 4],
    /// `[horizontal, vertical]`.
    pub repeat: [BorderImageRepeat; 2],
}

impl Default for BorderImage {
    fn default() -> Self {
        Self {
            source: BackgroundImage::None,
            source_css: None,
            slice: [BorderImageSlice::Percent(100.0); 4],
            fill: false,
            width: [BorderImageWidth::Number(1.0); 4],
            outset: [BorderImageOutset::Number(0.0); 4],
            repeat: [BorderImageRepeat::Stretch; 2],
        }
    }
}

impl BorderImage {
    /// Есть ли источник — рамку тогда рисует border-image, а не `border-style`.
    pub fn is_active(&self) -> bool {
        !matches!(self.source, BackgroundImage::None)
    }

    /// Computed-запись лонгхенда `prop`; `None` — это не лонгхенд border-image.
    pub fn longhand_css(&self, prop: &str) -> Option<String> {
        Some(match prop {
            "border-image-source" => self.source_text(),
            "border-image-slice" => self.slice_text(),
            "border-image-width" => shorten4(self.width.iter().map(width_text).collect()),
            "border-image-outset" => shorten4(self.outset.iter().map(outset_text).collect()),
            "border-image-repeat" => self.repeat_text(),
            _ => return None,
        })
    }

    /// Computed-запись шортката: `<source> <slice> / <width> / <outset> <repeat>`.
    pub fn shorthand_css(&self) -> String {
        format!(
            "{} {} / {} / {} {}",
            self.source_text(),
            self.slice_text(),
            shorten4(self.width.iter().map(width_text).collect()),
            shorten4(self.outset.iter().map(outset_text).collect()),
            self.repeat_text()
        )
    }

    /// Specified-запись шортката (`el.style.borderImage`): компоненты по умолчанию опускаются,
    /// `/ width` и `/ outset` печатаются только когда отличаются от начальных.
    pub fn specified_shorthand_css(&self) -> String {
        let d = Self::default();
        let mut parts: Vec<String> = Vec::new();
        if self.is_active() {
            parts.push(self.source_text());
        }
        if self.slice != d.slice || self.fill {
            parts.push(self.slice_text());
        }
        let width = (self.width != d.width).then(|| shorten4(self.width.iter().map(width_text).collect()));
        let outset = (self.outset != d.outset).then(|| shorten4(self.outset.iter().map(outset_text).collect()));
        if width.is_some() || outset.is_some() {
            let mut t = width.map_or_else(|| "/".to_string(), |w| format!("/ {w}"));
            if let Some(o) = outset {
                t = format!("{t} / {o}");
            }
            parts.push(t);
        }
        if self.repeat != d.repeat {
            parts.push(self.repeat_text());
        }
        if parts.is_empty() { "none".into() } else { parts.join(" ") }
    }

    fn source_text(&self) -> String {
        match &self.source {
            _ if self.source_css.is_some() => self.source_css.as_deref().unwrap_or("none").to_string(),
            BackgroundImage::Url(u) => format!("url(\"{}\")", u.replace('\\', "\\\\").replace('"', "\\\"")),
            _ => "none".to_string(),
        }
    }

    fn slice_text(&self) -> String {
        let s = shorten4(self.slice.iter().map(slice_text).collect());
        if self.fill { format!("{s} fill") } else { s }
    }

    fn repeat_text(&self) -> String {
        let [h, v] = self.repeat;
        if h == v { repeat_kw(h).to_string() } else { format!("{} {}", repeat_kw(h), repeat_kw(v)) }
    }
}

fn num(v: f32) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

fn slice_text(s: &BorderImageSlice) -> String {
    match s {
        BorderImageSlice::Number(n) => num(*n),
        BorderImageSlice::Percent(p) => format!("{}%", num(*p)),
    }
}

fn width_text(w: &BorderImageWidth) -> String {
    match w {
        BorderImageWidth::Number(n) => num(*n),
        BorderImageWidth::Length(l) => format!("{}px", num(*l)),
        BorderImageWidth::Percent(p) => format!("{}%", num(*p)),
        BorderImageWidth::Auto => "auto".into(),
    }
}

fn outset_text(o: &BorderImageOutset) -> String {
    match o {
        BorderImageOutset::Number(n) => num(*n),
        BorderImageOutset::Length(l) => format!("{}px", num(*l)),
    }
}

fn repeat_kw(r: BorderImageRepeat) -> &'static str {
    match r {
        BorderImageRepeat::Stretch => "stretch",
        BorderImageRepeat::Repeat => "repeat",
        BorderImageRepeat::Round => "round",
        BorderImageRepeat::Space => "space",
    }
}

/// CSSOM «serialize a CSS value» для `{1,4}`-списка: повторяющиеся стороны опускаются
/// (`a b a b` → `a b`, `a b c b` → `a b c`, все равны → `a`).
fn shorten4(v: Vec<String>) -> String {
    let [t, r, b, l] = [&v[0], &v[1], &v[2], &v[3]];
    if l != r {
        format!("{t} {r} {b} {l}")
    } else if b != t {
        format!("{t} {r} {b}")
    } else if r != t {
        format!("{t} {r}")
    } else {
        t.clone()
    }
}

/// Раскрытие `{1,4}` по правилу margin: top, right, bottom, left.
fn expand4<T: Copy>(v: &[T]) -> Option<[T; 4]> {
    Some(match v {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    })
}

/// Неотрицательное число без единицы.
fn parse_number(t: &str) -> Option<f32> {
    let first = t.chars().next()?;
    if !(first.is_ascii_digit() || matches!(first, '.' | '+' | '-')) {
        return None;
    }
    t.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

fn parse_percent(t: &str) -> Option<f32> {
    let p = t.strip_suffix('%')?.trim().parse::<f32>().ok()?;
    (p.is_finite() && p >= 0.0).then_some(p)
}

/// `border-image-source`: `none | <image>`; неизвестная функция — недопустимо.
pub(in crate::style) fn parse_border_image_source(
    s: &str,
    em_basis: f32,
    viewport: Size,
) -> Option<(BackgroundImage, Option<Arc<str>>)> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some((BackgroundImage::None, None));
    }
    if is_image_set_value(s) {
        let css = computed_image_css(s, em_basis, viewport).map(Arc::<str>::from);
        return Some((BackgroundImage::Url(s.to_string()), css));
    }
    if let Some(u) = parse_url_value(s) {
        return Some((BackgroundImage::Url(u), None));
    }
    // CSS Paint API (Houdini): the worklet draws the border image (`lumen_paint::collect_paint_requests`).
    if let Some((name, args)) = parse_paint_function(s) {
        return Some((BackgroundImage::Paint { name, args }, Some(Arc::from(s))));
    }
    if is_gradient_function(s) {
        let css = computed_image_css(s, em_basis, viewport).map(Arc::<str>::from);
        return Some((BackgroundImage::Gradient(parse_background_gradient(s)), css));
    }
    let (a, b, t) = parse_cross_fade(s)?;
    let css = computed_image_css(s, em_basis, viewport).map(Arc::<str>::from);
    Some((BackgroundImage::CrossFade { a: Box::new(a), b: Box::new(b), t }, css))
}

fn is_source_token(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l == "none" || l.contains('(')
}

fn slice_token(t: &str) -> bool {
    t.eq_ignore_ascii_case("fill") || parse_number(t).is_some() || parse_percent(t).is_some()
}

fn repeat_token(t: &str) -> Option<BorderImageRepeat> {
    match t.to_ascii_lowercase().as_str() {
        "stretch" => Some(BorderImageRepeat::Stretch),
        "repeat" => Some(BorderImageRepeat::Repeat),
        "round" => Some(BorderImageRepeat::Round),
        "space" => Some(BorderImageRepeat::Space),
        _ => None,
    }
}

/// `border-image-slice`: `[<number> | <percentage>]{1,4} && fill?` → (стороны, fill).
pub(in crate::style) fn parse_border_image_slice(s: &str) -> Option<([BorderImageSlice; 4], bool)> {
    let mut fill = false;
    let mut vals: Vec<BorderImageSlice> = Vec::new();
    for t in split_top_level_ws(s.trim()) {
        if t.eq_ignore_ascii_case("fill") {
            if fill {
                return None;
            }
            fill = true;
        } else if let Some(n) = parse_number(t) {
            vals.push(BorderImageSlice::Number(n));
        } else {
            vals.push(BorderImageSlice::Percent(parse_percent(t)?));
        }
    }
    Some((expand4(&vals)?, fill))
}

/// `border-image-width`: `[<length-percentage> | <number> | auto]{1,4}`.
pub(in crate::style) fn parse_border_image_width(
    s: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<[BorderImageWidth; 4]> {
    let mut vals: Vec<BorderImageWidth> = Vec::new();
    for t in split_top_level_ws(s.trim()) {
        vals.push(if t.eq_ignore_ascii_case("auto") {
            BorderImageWidth::Auto
        } else if let Some(p) = parse_percent(t) {
            BorderImageWidth::Percent(p)
        } else if let Some(n) = parse_number(t) {
            BorderImageWidth::Number(n)
        } else {
            match parse_length_q(t, is_quirks)? {
                Length::Percent(_) => return None,
                len => BorderImageWidth::Length(len.resolve(em_basis, None, viewport).filter(|v| *v >= 0.0)?),
            }
        });
    }
    expand4(&vals)
}

/// `border-image-outset`: `[<length> | <number>]{1,4}`.
pub(in crate::style) fn parse_border_image_outset(
    s: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<[BorderImageOutset; 4]> {
    let mut vals: Vec<BorderImageOutset> = Vec::new();
    for t in split_top_level_ws(s.trim()) {
        vals.push(if let Some(n) = parse_number(t) {
            BorderImageOutset::Number(n)
        } else {
            match parse_length_q(t, is_quirks)? {
                Length::Percent(_) => return None,
                len => BorderImageOutset::Length(len.resolve(em_basis, None, viewport).filter(|v| *v >= 0.0)?),
            }
        });
    }
    expand4(&vals)
}

/// `border-image-repeat`: `[stretch | repeat | round | space]{1,2}`.
pub(in crate::style) fn parse_border_image_repeat(s: &str) -> Option<[BorderImageRepeat; 2]> {
    let v: Vec<BorderImageRepeat> = split_top_level_ws(s.trim()).into_iter().map(repeat_token).collect::<Option<_>>()?;
    match v.as_slice() {
        [a] => Some([*a, *a]),
        [a, b] => Some([*a, *b]),
        _ => None,
    }
}

/// Токены шортката: пробелы на верхнем уровне разделяют, `/` вне скобок — отдельный токен.
fn tokenize(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut quote) = (0usize, None::<char>);
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '/' if depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                out.push("/".into());
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Шорткат: `<source> || <slice> [/ <width>? [/ <outset>]?]? || <repeat>`; опущенные части — initial.
pub(in crate::style) fn parse_border_image(
    s: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> Option<BorderImage> {
    let toks = tokenize(s.trim());
    let mut out = BorderImage::default();
    let (mut have_src, mut have_slice, mut have_repeat) = (false, false, false);
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        if !have_src && is_source_token(t) {
            let (img, css) = parse_border_image_source(t, em_basis, viewport)?;
            out.source = img;
            out.source_css = css;
            have_src = true;
            i += 1;
        } else if !have_repeat && repeat_token(t).is_some() {
            let mut kws: Vec<&str> = Vec::new();
            while i < toks.len() && repeat_token(&toks[i]).is_some() && kws.len() < 2 {
                kws.push(toks[i].as_str());
                i += 1;
            }
            out.repeat = parse_border_image_repeat(&kws.join(" "))?;
            have_repeat = true;
        } else if !have_slice && slice_token(t) {
            let start = i;
            while i < toks.len() && slice_token(&toks[i]) {
                i += 1;
            }
            (out.slice, out.fill) = parse_border_image_slice(&toks[start..i].join(" "))?;
            have_slice = true;
            if toks.get(i).map(String::as_str) == Some("/") {
                i += 1;
                let w_start = i;
                while i < toks.len() && toks[i] != "/" && repeat_token(&toks[i]).is_none() && !is_source_token(&toks[i]) {
                    i += 1;
                }
                let has_width = i > w_start;
                if has_width {
                    out.width = parse_border_image_width(&toks[w_start..i].join(" "), em_basis, viewport, is_quirks)?;
                }
                if toks.get(i).map(String::as_str) == Some("/") {
                    i += 1;
                    let o_start = i;
                    while i < toks.len() && repeat_token(&toks[i]).is_none() && !is_source_token(&toks[i]) && toks[i] != "/" {
                        i += 1;
                    }
                    if i == o_start {
                        return None;
                    }
                    out.outset = parse_border_image_outset(&toks[o_start..i].join(" "), em_basis, viewport, is_quirks)?;
                } else if !has_width {
                    return None;
                }
            }
        } else {
            return None;
        }
    }
    (have_src || have_slice || have_repeat).then_some(out)
}

/// Каноническая заданная запись `border-image*` для `el.style` (CSSOM): значение проверяется
/// грамматикой свойства, повторы в `{1,4}` сворачиваются, регистр ключевых слов приводится к нижнему.
/// `None` — присваивание отбрасывается.
pub fn canonical_specified_border_image(prop: &str, value: &str) -> Option<String> {
    let vp = Size { width: 0.0, height: 0.0 };
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    let join4 = |toks: Vec<&str>| -> String {
        let t: Vec<String> = toks.iter().map(|t| t.to_ascii_lowercase()).collect();
        match t.len() {
            4 => shorten4(t),
            3 => shorten4(vec![t[0].clone(), t[1].clone(), t[2].clone(), t[1].clone()]),
            2 => shorten4(vec![t[0].clone(), t[1].clone(), t[0].clone(), t[1].clone()]),
            _ => t.join(" "),
        }
    };
    match prop {
        "border-image-source" => {
            let (img, _) = parse_border_image_source(v, 16.0, vp)?;
            Some(match img {
                BackgroundImage::None => "none".into(),
                BackgroundImage::Url(_) if is_image_set_value(v) => v.to_string(),
                BackgroundImage::Url(u) => format!("url(\"{}\")", u.replace('\\', "\\\\").replace('"', "\\\"")),
                _ => v.to_string(),
            })
        }
        "border-image-slice" => {
            let fill = parse_border_image_slice(v)?.1;
            let toks: Vec<&str> = split_top_level_ws(v).into_iter().filter(|t| !t.eq_ignore_ascii_case("fill")).collect();
            let s = join4(toks);
            Some(if fill { format!("{s} fill") } else { s })
        }
        "border-image-width" => {
            parse_border_image_width(v, 16.0, vp, false)?;
            Some(join4(split_top_level_ws(v)))
        }
        "border-image-outset" => {
            parse_border_image_outset(v, 16.0, vp, false)?;
            Some(join4(split_top_level_ws(v)))
        }
        "border-image-repeat" => {
            let [h, w] = parse_border_image_repeat(v)?;
            Some(if h == w { repeat_kw(h).into() } else { format!("{} {}", repeat_kw(h), repeat_kw(w)) })
        }
        "border-image" => {
            Some(parse_border_image(v, 16.0, vp, false)?.specified_shorthand_css())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    #[test]
    fn source_accepts_a_paint_function() {
        let (img, css) = parse_border_image_source("paint(geometry, 3px)", 16.0, VP).unwrap();
        assert_eq!(img, BackgroundImage::Paint { name: "geometry".into(), args: vec!["3px".into()] });
        assert_eq!(css.as_deref(), Some("paint(geometry, 3px)"));
        assert!(parse_border_image_source("paint(", 16.0, VP).is_none(), "an unclosed paint() is invalid");
    }

    #[test]
    fn slice_forms() {
        let (s, f) = parse_border_image_slice("fill 30% 10").unwrap();
        assert!(f);
        assert_eq!(s[0], BorderImageSlice::Percent(30.0));
        assert_eq!(s[1], BorderImageSlice::Number(10.0));
        assert_eq!(s[2], s[0]);
        assert_eq!(s[3], s[1]);
        assert!(parse_border_image_slice("1 2 3 4 5").is_none());
        assert!(parse_border_image_slice("-1").is_none());
        assert!(parse_border_image_slice("fill fill").is_none());
        assert!(parse_border_image_slice("10px").is_none());
    }

    #[test]
    fn width_outset_repeat() {
        let w = parse_border_image_width("auto 10px 2 5%", 16.0, VP, false).unwrap();
        assert_eq!(w, [BorderImageWidth::Auto, BorderImageWidth::Length(10.0), BorderImageWidth::Number(2.0), BorderImageWidth::Percent(5.0)]);
        assert!(parse_border_image_width("-1", 16.0, VP, false).is_none());
        let o = parse_border_image_outset("2em 1", 16.0, VP, false).unwrap();
        assert_eq!(o[0], BorderImageOutset::Length(32.0));
        assert_eq!(o[1], BorderImageOutset::Number(1.0));
        assert!(parse_border_image_outset("5%", 16.0, VP, false).is_none());
        assert_eq!(parse_border_image_repeat("round space"), Some([BorderImageRepeat::Round, BorderImageRepeat::Space]));
        assert!(parse_border_image_repeat("round space repeat").is_none());
    }

    #[test]
    fn shorthand() {
        let b = parse_border_image("url(a.png) 30 fill / 10px / 5px round", 16.0, VP, false).unwrap();
        assert_eq!(b.source, BackgroundImage::Url("a.png".into()));
        assert!(b.fill);
        assert_eq!(b.width[0], BorderImageWidth::Length(10.0));
        assert_eq!(b.outset[2], BorderImageOutset::Length(5.0));
        assert_eq!(b.repeat, [BorderImageRepeat::Round; 2]);
        let b = parse_border_image("repeat 30%/ / 3 url(a.png)", 16.0, VP, false).unwrap();
        assert_eq!(b.width, BorderImage::default().width);
        assert_eq!(b.outset[0], BorderImageOutset::Number(3.0));
        assert!(parse_border_image("url(a.png) / 3", 16.0, VP, false).is_none());
        assert!(parse_border_image("url(a.png) 30 /", 16.0, VP, false).is_none());
        assert!(parse_border_image("url(a.png) url(b.png)", 16.0, VP, false).is_none());
        assert!(parse_border_image("", 16.0, VP, false).is_none());
    }

    #[test]
    fn computed_text() {
        let b = parse_border_image("url(a.png) 30 / 10px 1 / 2 stretch repeat", 16.0, VP, false).unwrap();
        assert_eq!(b.shorthand_css(), "url(\"a.png\") 30 / 10px 1 / 2 stretch repeat");
        assert_eq!(BorderImage::default().shorthand_css(), "none 100% / 1 / 0 stretch");
        assert_eq!(b.longhand_css("border-image-slice").as_deref(), Some("30"));
    }

    #[test]
    fn canonical() {
        let c = canonical_specified_border_image;
        assert_eq!(c("border-image-slice", "10 20 10 20 FILL").as_deref(), Some("10 20 fill"));
        assert_eq!(c("border-image-width", "1 1 1 1").as_deref(), Some("1"));
        assert_eq!(c("border-image-repeat", "round round").as_deref(), Some("round"));
        assert_eq!(c("border-image-source", "url(x)").as_deref(), Some("url(\"x\")"));
        assert!(c("border-image-outset", "10%").is_none());
        assert_eq!(c("border-image", "fill 1 2% 3 4% / auto").as_deref(), Some("1 2% 3 4% fill / auto"));
        assert_eq!(c("border-image", "1 / / 1px").as_deref(), Some("1 / / 1px"));
        assert_eq!(c("border-image", "none 100% / 1 / 0 stretch").as_deref(), Some("none"));
        assert_eq!(c("border-image", "none space space").as_deref(), Some("space"));
        assert_eq!(c("border-image", "url(a) 1 / 1px 2% 3 auto / 2 round").as_deref(), Some("url(\"a\") 1 / 1px 2% 3 auto / 2 round"));
    }
}
