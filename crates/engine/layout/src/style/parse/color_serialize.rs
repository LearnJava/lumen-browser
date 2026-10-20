//! CSS Color L4 §15 — сериализация computed/specified значения `<color>`, у которого цветовое
//! пространство не сводится к `rgb()`: `lab()`/`lch()`/`oklab()`/`oklch()`, `color(<space> …)`,
//! `color-mix()` и относительные цвета (BUG-1425).
//!
//! Каскад хранит `style.color` как sRGB-байты, поэтому исходное пространство теряется;
//! [`computed_color_css`] строит строку, которую `getComputedStyle().color` и `el.style.color`
//! отдают вместо `rgb(…)`. Разбор каналов — общий с относительными цветами
//! ([`eval_color_component`]): `calc()`, проценты, углы, `none`.

use super::color::{eval_color_component, parse_color_mix_f32};
use crate::color_mix::{self, MixColorSpace};

/// Канал после разбора: число либо `none`.
type Chan = Option<f32>;

/// Что хранить в `ComputedStyle::color_css` для объявления `color: <val>`: `Some(строка)` для
/// форм, чьё пространство нельзя восстановить из `rgb()`, иначе `None`.
pub fn computed_color_css(val: &str) -> Option<String> {
    let val = super::color::strip_comments(val);
    let s = val.trim();
    let lower = s.to_ascii_lowercase();
    if let Some(css) = super::color::relative_color_css(&lower) {
        return Some(css);
    }
    if lower.starts_with("color-mix(") && lower.ends_with(')') {
        return serialize_color_mix(&lower["color-mix(".len()..lower.len() - 1]);
    }
    if let Some(rest) = lower.strip_prefix("color(").and_then(|t| t.strip_suffix(')')) {
        return serialize_color_function(rest);
    }
    for (name, kind) in [("oklab(", LabFn::Oklab), ("oklch(", LabFn::Oklch), ("lab(", LabFn::Lab), ("lch(", LabFn::Lch)] {
        if let Some(rest) = lower.strip_prefix(name).and_then(|t| t.strip_suffix(')')) {
            if rest.trim_start().starts_with("from ") {
                return None;
            }
            return serialize_lab_family(kind, rest);
        }
    }
    None
}

/// Четвёрка lab-подобных функций.
#[derive(Clone, Copy)]
pub(super) enum LabFn {
    Lab,
    Lch,
    Oklab,
    Oklch,
}

impl LabFn {
    fn name(self) -> &'static str {
        match self {
            LabFn::Lab => "lab",
            LabFn::Lch => "lch",
            LabFn::Oklab => "oklab",
            LabFn::Oklch => "oklch",
        }
    }

    /// 100% каждого канала (CSS Color L4 §9.2–9.4); `0` — канал-оттенок, где % недопустим.
    fn pct_basis(self) -> [f32; 3] {
        match self {
            LabFn::Lab => [100.0, 125.0, 125.0],
            LabFn::Lch => [100.0, 150.0, 0.0],
            LabFn::Oklab => [1.0, 0.4, 0.4],
            LabFn::Oklch => [1.0, 0.4, 0.0],
        }
    }

    fn l_max(self) -> f32 {
        if matches!(self, LabFn::Lab | LabFn::Lch) { 100.0 } else { 1.0 }
    }

    fn polar(self) -> bool {
        matches!(self, LabFn::Lch | LabFn::Oklch)
    }

    pub(super) fn from_mix_space(space: MixColorSpace) -> Option<Self> {
        Some(match space {
            MixColorSpace::Lab => LabFn::Lab,
            MixColorSpace::Lch => LabFn::Lch,
            MixColorSpace::Oklab => LabFn::Oklab,
            MixColorSpace::Oklch => LabFn::Oklch,
            _ => return None,
        })
    }
}

/// Тело `lab(…)`/`lch(…)`/`oklab(…)`/`oklch(…)` (без имени и скобок, в нижнем регистре).
pub(super) fn serialize_lab_family(kind: LabFn, body: &str) -> Option<String> {
    if body.contains(',') {
        return None;
    }
    let (parts, alpha_tok) = split_args(body)?;
    if parts.len() != 3 {
        return None;
    }
    let basis = kind.pct_basis();
    let mut c: [Chan; 3] = [None; 3];
    for ((slot, tok), pct) in c.iter_mut().zip(&parts).zip(basis) {
        *slot = channel(tok, pct)?;
    }
    let alpha = match alpha_tok {
        Some(a) => channel(&a, 1.0)?,
        None => Some(1.0),
    };
    // §9.2/§9.4: lightness клампится в диапазон, chroma — снизу, hue нормируется в [0, 360).
    Some(finish_lab(kind, c, alpha))
}

/// Нормирует каналы (§9.2/§9.4: lightness клампится, chroma — снизу, hue — в [0, 360)) и печатает.
pub(super) fn finish_lab(kind: LabFn, mut c: [Chan; 3], alpha: Chan) -> String {
    c[0] = c[0].map(|l| l.clamp(0.0, kind.l_max()));
    if kind.polar() {
        c[1] = c[1].map(|v| v.max(0.0));
        c[2] = c[2].map(|h| h.rem_euclid(360.0));
    }
    format_function(kind.name(), &c, alpha)
}

/// Тело `color(<space> c1 c2 c3 [/ a])`. Пространство-псевдоним `xyz` → `xyz-d65`.
fn serialize_color_function(body: &str) -> Option<String> {
    let (parts, alpha_tok) = split_args(body)?;
    // Первый токен — пространство.
    let (space, rest) = parts.split_first()?;
    let space = match space.as_str() {
        "xyz" => "xyz-d65",
        "srgb" | "srgb-linear" | "display-p3" | "display-p3-linear" | "a98-rgb" | "prophoto-rgb" | "rec2020"
        | "xyz-d50" | "xyz-d65" => space.as_str(),
        _ => return None,
    };
    if rest.len() != 3 {
        return None;
    }
    let mut c: [Chan; 3] = [None; 3];
    for (slot, tok) in c.iter_mut().zip(rest) {
        *slot = channel(tok, 1.0)?;
    }
    let alpha = match alpha_tok {
        Some(a) => channel(&a, 1.0)?,
        None => Some(1.0),
    };
    Some(format_function(&format!("color({space}"), &c, alpha))
}

/// `color-mix(in <space>, a, b)`: результат живёт в пространстве интерполяции (CSS Color L5 §3.3);
/// lab-подобные пространства дают `lab()`/…, прочие — `color(srgb …)`.
fn serialize_color_mix(body: &str) -> Option<String> {
    let (out, space) = parse_color_mix_f32(body)?;
    let alpha = Some(out[3].clamp(0.0, 1.0));
    if let Some(kind) = LabFn::from_mix_space(space) {
        let ch = color_mix::relative_origin_channels(space, out);
        return Some(finish_lab(kind, [Some(ch[0]), Some(ch[1]), Some(ch[2])], alpha));
    }
    let c = [Some(out[0]), Some(out[1]), Some(out[2])];
    Some(format_function("color(srgb", &c, alpha))
}

/// Тот же `color(srgb r g b / a)` для уже посчитанных float-каналов (относительный `rgb(from …)`).
pub(super) fn srgb_function(rgb: [f32; 3], alpha: f32) -> String {
    format_function("color(srgb", &[Some(rgb[0]), Some(rgb[1]), Some(rgb[2])], Some(alpha.clamp(0.0, 1.0)))
}

/// Делит тело функции на токены верхнего уровня; `/` — отдельный токен, `calc(…)` остаётся целым.
/// Возвращает каналы и необязательный alpha-токен; `None` — если `/` не на месте.
fn split_args(body: &str) -> Option<(Vec<String>, Option<String>)> {
    let mut toks: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in body.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            c if depth == 0 && (c.is_whitespace() || c == '/') => {
                if !buf.is_empty() {
                    toks.push(std::mem::take(&mut buf));
                }
                if c == '/' {
                    toks.push("/".into());
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        toks.push(buf);
    }
    match toks.iter().position(|t| t == "/") {
        None => Some((toks, None)),
        Some(i) if i + 2 == toks.len() => {
            let alpha = toks.pop();
            toks.pop();
            Some((toks, alpha))
        }
        Some(_) => None,
    }
}

/// Канал: `none` → `None`; иначе значение через общий вычислитель `calc()`/%/углов.
/// `calc(NaN)` и деление на ноль сворачиваются в `0` (CSS Values L4 §10.9).
fn channel(raw: &str, pct_basis: f32) -> Option<Chan> {
    let t = raw.trim();
    if t == "none" {
        return Some(None);
    }
    if t.contains("nan") {
        return Some(Some(0.0));
    }
    match eval_color_component(t, &[], pct_basis) {
        Some(v) if v.is_finite() => Some(Some(v)),
        Some(_) => Some(Some(0.0)),
        None if t.starts_with("calc(") && t.contains("/ 0") => Some(Some(0.0)),
        None => None,
    }
}

fn format_function_body(c: &[Chan; 3], alpha: Chan) -> String {
    let mut s = format!("{} {} {}", num(c[0]), num(c[1]), num(c[2]));
    match alpha.map(|a| a.clamp(0.0, 1.0)) {
        Some(a) if a >= 1.0 => {}
        Some(a) => {
            s.push_str(" / ");
            s.push_str(&num(Some(a)));
        }
        None => s.push_str(" / none"),
    }
    s
}

/// `prefix` — `lab`, либо `color(<space>` (скобка и пространство уже внутри).
fn format_function(prefix: &str, c: &[Chan; 3], alpha: Chan) -> String {
    let body = format_function_body(c, alpha);
    if prefix.starts_with("color(") {
        format!("{prefix} {body})")
    } else {
        format!("{prefix}({body})")
    }
}

/// Число в записи CSSOM: до 6 значащих цифр, без хвостовых нулей (`73.3386`, `0.5`).
fn num(v: Chan) -> String {
    let Some(v) = v else { return "none".into() };
    let v = f64::from(v);
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let int_digits = v.abs().log10().floor() as i32 + 1;
    let scale = 10f64.powi((6 - int_digits).max(0));
    let r = (v * scale).round() / scale;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

/// Текст для обычного парсера `<color>`: `none` → `0` (CSS Color L4 §4.4 — отсутствующий канал
/// при конвертации в sRGB трактуется как 0).
pub fn none_to_zero(css: &str) -> String {
    // В канонической записи `none` встречается только как ключевое слово канала.
    css.replace("none", "0")
}

#[cfg(test)]
mod tests {
    use super::computed_color_css as css;

    #[test]
    fn lab_family_keeps_function() {
        assert_eq!(css("lab(46 -50 50)").as_deref(), Some("lab(46 -50 50)"));
        assert_eq!(css("LAB(0 0 0 / 1)").as_deref(), Some("lab(0 0 0)"));
        assert_eq!(css("lab(400 0 10/50%)").as_deref(), Some("lab(100 0 10 / 0.5)"));
        assert_eq!(css("lab(-40 0 0)").as_deref(), Some("lab(0 0 0)"));
        assert_eq!(css("lab(20% -50% 90%/0.5)").as_deref(), Some("lab(20 -62.5 112.5 / 0.5)"));
        assert_eq!(css("lab(none none none / none)").as_deref(), Some("lab(none none none / none)"));
        assert_eq!(css("lab(0 0 0 / -10%)").as_deref(), Some("lab(0 0 0 / 0)"));
        assert_eq!(css("lab(calc(50 * 3) calc(0.5 - 1) calc(1.5) / calc(-0.5 + 1))").as_deref(), Some("lab(100 -0.5 1.5 / 0.5)"));
    }

    #[test]
    fn polar_hue_and_chroma_normalised() {
        assert_eq!(css("lch(10 20 1.28rad)").as_deref(), Some("lch(10 20 73.3386)"));
        assert_eq!(css("lch(10 20 -340deg)").as_deref(), Some("lch(10 20 20)"));
        assert_eq!(css("lch(20 -20 0)").as_deref(), Some("lch(20 0 0)"));
        assert_eq!(css("oklch(0.5 0.1 90)").as_deref(), Some("oklch(0.5 0.1 90)"));
        assert_eq!(css("oklab(4 0 0.1/50%)").as_deref(), Some("oklab(1 0 0.1 / 0.5)"));
        assert_eq!(css("oklch(20% 60% 10/0.5)").as_deref(), Some("oklch(0.2 0.24 10 / 0.5)"));
    }

    #[test]
    fn color_function_keeps_space() {
        assert_eq!(css("color(srgb 0 0.5 0)").as_deref(), Some("color(srgb 0 0.5 0)"));
        assert_eq!(css("color(display-p3 100% none 20% / 23.7%)").as_deref(), Some("color(display-p3 1 none 0.2 / 0.237)"));
        assert_eq!(css("color(xyz 1 1 1)").as_deref(), Some("color(xyz-d65 1 1 1)"));
        assert_eq!(css("color(srgb 200% 0 0 / 200)").as_deref(), Some("color(srgb 2 0 0)"));
    }

    #[test]
    fn non_wide_forms_are_none() {
        assert_eq!(css("rgb(0 128 0)"), None);
        assert_eq!(css("red"), None);
        assert_eq!(css("hwb(120 0% 50%)"), None);
        assert_eq!(css("lab(20 20 20 / calc(1"), None);
    }

    #[test]
    fn relative_colors_keep_function() {
        assert_eq!(css("rgb(from rebeccapurple r g b)").as_deref(), Some("color(srgb 0.4 0.2 0.6)"));
        assert_eq!(css("rgb(from rgb(20%, 40%, 60%, 80%) r g b / alpha)").as_deref(), Some("color(srgb 0.2 0.4 0.6 / 0.8)"));
        assert!(css("lab(from red l a b)").unwrap().starts_with("lab("));
    }

    #[test]
    fn color_mix_serialises_in_interpolation_space() {
        assert_eq!(css("color-mix(in srgb, red, blue)").as_deref(), Some("color(srgb 0.5 0 0.5)"));
        assert!(css("color-mix(in oklab, red, blue)").unwrap().starts_with("oklab("));
    }
}
