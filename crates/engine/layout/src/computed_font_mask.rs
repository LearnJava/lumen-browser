//! `getComputedStyle()` для шрифтовых и масочных свойств, которых раньше не было в карте
//! [`crate::selector_query::computed_style_to_map`] (BUG-1407): `font`-шорткод,
//! `font-feature-settings`, `font-variation-settings`, `font-size-adjust`,
//! `font-optical-sizing`, `font-palette`, `clip-path`, `clip-rule`, `mask*`.

use std::collections::HashMap;

use crate::selector_query::{
    background_origin_to_css, background_repeat_to_css, background_size_to_css, fill_rule_to_css,
    position_component_to_css, px_str,
};
use crate::style::{
    ClipPath, ComputedStyle, FontOpticalSizing, FontPalette, FontSizeAdjust, FontStyle, FontWeight,
    FontVariantCaps, FontVariantEmoji, FontVariantLigatures, FontVariantNumeric,
    FontVariantPosition, GeometryBox, MaskClip, MaskComposite, MaskLayer, MaskMode, ShapeValue,
};
use crate::style::{BackgroundImage, BackgroundOrigin};

/// Добавляет в `m` шрифтовые и масочные свойства. `font_size_px` — computed `font-size` без
/// зума (для шорткода `font`).
pub(crate) fn insert_font_mask_computed(style: &ComputedStyle, font_size_px: f32, m: &mut HashMap<String, String>) {
    // CSS Fonts L4 §11.1: дубли тегов — побеждает последний; сериализация по возрастанию тега.
    m.insert(
        "font-feature-settings".into(),
        tag_list(style.font_feature_settings.iter().map(|f| (f.tag, f.value.to_string())), true),
    );
    m.insert(
        "font-variation-settings".into(),
        tag_list(style.font_variation_settings.iter().map(|f| (f.tag, f.value.to_string())), false),
    );
    m.insert("font-optical-sizing".into(), match style.font_optical_sizing {
        FontOpticalSizing::Auto => "auto",
        FontOpticalSizing::None => "none",
    }.into());
    m.insert("font-palette".into(), match &style.font_palette {
        FontPalette::Normal => "normal".into(),
        FontPalette::Light => "light".into(),
        FontPalette::Dark => "dark".into(),
        FontPalette::Custom(name) => name.clone(),
    });
    m.insert("font-size-adjust".into(), match style.font_size_adjust {
        FontSizeAdjust::None => "none".into(),
        FontSizeAdjust::Auto => "from-font".into(),
        FontSizeAdjust::Value(v) => v.to_string(),
    });
    m.insert("font".into(), font_shorthand(style, font_size_px));

    m.insert("clip-path".into(), style.clip_path.as_ref().map_or("none".into(), clip_path_to_css));
    m.insert("clip-rule".into(), fill_rule_to_css(style.svg_clip_rule).into());
    insert_mask(&style.mask_layers, m);
}

/// `"tag" value, …` — по возрастанию тега, повторы схлопнуты (побеждает последний);
/// пусто → `normal`. У feature-тега значение `1` — умолчание и не пишется (`"dlig"`),
/// у variation-оси значение обязательно.
fn tag_list(items: impl Iterator<Item = ([u8; 4], String)>, omit_one: bool) -> String {
    let mut out: Vec<([u8; 4], String)> = Vec::new();
    for (tag, value) in items {
        match out.iter_mut().find(|(t, _)| *t == tag) {
            Some(slot) => slot.1 = value,
            None => out.push((tag, value)),
        }
    }
    if out.is_empty() {
        return "normal".into();
    }
    out.sort_by_key(|(t, _)| *t);
    out.iter()
        .map(|(tag, v)| {
            let tag = String::from_utf8_lossy(tag);
            if omit_one && v == "1" { format!("\"{tag}\"") } else { format!("\"{tag}\" {v}") }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn font_shorthand(style: &ComputedStyle, font_size_px: f32) -> String {
    // Шорткод невыразим, если хоть один скрытый им лонгхенд не в initial (CSSOM §6.7.2 → "").
    let plain = style.font_variant_caps == FontVariantCaps::Normal
        && style.font_variant_ligatures == FontVariantLigatures::default()
        && style.font_variant_numeric == FontVariantNumeric::default()
        && style.font_variant_position == FontVariantPosition::Normal
        && style.font_variant_emoji == FontVariantEmoji::Normal
        && style.font_stretch.0 == 1000
        && style.font_size_adjust == FontSizeAdjust::None
        && style.font_feature_settings.is_empty()
        && style.font_variation_settings.is_empty()
        && style.font_optical_sizing == FontOpticalSizing::Auto
        && style.font_palette == FontPalette::Normal;
    if !plain || style.font_family.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    match style.font_style {
        FontStyle::Normal => {}
        FontStyle::Italic => parts.push("italic".into()),
        FontStyle::Oblique => parts.push(crate::style::font_style_to_css(
            FontStyle::Oblique,
            style.font_oblique_angle,
        )),
    }
    if style.font_weight != FontWeight::NORMAL {
        parts.push(style.font_weight.serialize());
    }
    let mut size = px_str(font_size_px);
    // `line-height: normal` хранится как относительные 1.2 — в шорткод не выводится.
    if !(style.line_height_is_relative && (style.line_height - 1.2).abs() < f32::EPSILON) {
        let lh = if style.line_height_is_relative {
            style.line_height.to_string()
        } else {
            px_str(style.line_height * style.font_size / style.effective_zoom.max(f32::EPSILON))
        };
        size = format!("{size}/{lh}");
    }
    parts.push(size);
    parts.push(
        style
            .font_family
            .iter()
            .map(|s| if s.contains(' ') { format!("\"{s}\"") } else { s.clone() })
            .collect::<Vec<_>>()
            .join(", "),
    );
    parts.join(" ")
}

fn shape_value_to_css(v: ShapeValue) -> String {
    match v {
        ShapeValue::Px(p) => px_str(p),
        ShapeValue::Pct(p) if p.fract() == 0.0 => format!("{}%", p as i64),
        ShapeValue::Pct(p) => format!("{p}%"),
        ShapeValue::ClosestSide => "closest-side".into(),
        ShapeValue::FarthestSide => "farthest-side".into(),
        ShapeValue::Lin { px, pct: 0.0 } => px_str(px),
        ShapeValue::Lin { px, pct } => {
            let pct = if pct.fract() == 0.0 { format!("{}", pct as i64) } else { format!("{pct}") };
            let sign = if px < 0.0 { '-' } else { '+' };
            format!("calc({pct}% {sign} {})", px_str(px.abs()))
        }
    }
}

/// 1..=4 значений в порядке top right bottom left (или углов) → кратчайшая запись.
fn four_values(vals: &[ShapeValue]) -> [String; 4] {
    let s: Vec<String> = vals.iter().map(|v| shape_value_to_css(*v)).collect();
    match s.len() {
        0 => std::array::from_fn(|_| "0px".to_string()),
        1 => std::array::from_fn(|_| s[0].clone()),
        2 => [s[0].clone(), s[1].clone(), s[0].clone(), s[1].clone()],
        3 => [s[0].clone(), s[1].clone(), s[2].clone(), s[1].clone()],
        _ => [s[0].clone(), s[1].clone(), s[2].clone(), s[3].clone()],
    }
}

fn collapse4(a: &[String; 4]) -> String {
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

fn center_to_css(c: &Option<(ShapeValue, ShapeValue)>) -> String {
    c.map_or(String::new(), |(x, y)| format!(" at {} {}", shape_value_to_css(x), shape_value_to_css(y)))
}

fn geometry_box_to_css(g: GeometryBox) -> &'static str {
    match g {
        GeometryBox::BorderBox => "border-box",
        GeometryBox::PaddingBox => "padding-box",
        GeometryBox::ContentBox => "content-box",
        GeometryBox::MarginBox => "margin-box",
        GeometryBox::FillBox => "fill-box",
        GeometryBox::StrokeBox => "stroke-box",
        GeometryBox::ViewBox => "view-box",
    }
}

/// Радиус `circle()`/`ellipse()`: `closest-side` — умолчание и не пишется.
fn radius_to_css(r: ShapeValue) -> Option<String> {
    (r != ShapeValue::ClosestSide).then(|| shape_value_to_css(r))
}

fn clip_path_to_css(cp: &ClipPath) -> String {
    match cp {
        ClipPath::Inset(sides) => format!("inset({})", collapse4(&four_values(sides))),
        ClipPath::InsetRound { sides, rx, ry } => {
            let h = four_values(rx);
            let v = if ry.is_empty() { h.clone() } else { four_values(ry) };
            let radii = crate::style::serialize_border_radius(
                [&h[0], &h[1], &h[2], &h[3]],
                [&v[0], &v[1], &v[2], &v[3]],
            );
            format!("inset({} round {radii})", collapse4(&four_values(sides)))
        }
        ClipPath::Boxed { shape, geometry } => match shape {
            Some(s) => format!("{} {}", clip_path_to_css(s), geometry_box_to_css(*geometry)),
            None => geometry_box_to_css(*geometry).into(),
        },
        ClipPath::Circle { radius, center } => {
            let r = radius_to_css(*radius).unwrap_or_default();
            let c = center_to_css(center);
            format!("circle({}{})", r, if r.is_empty() { c.trim_start() } else { c.as_str() })
        }
        ClipPath::Ellipse { rx, ry, center } => {
            let radii = match (radius_to_css(*rx), radius_to_css(*ry)) {
                (None, None) => String::new(),
                (a, b) => format!(
                    "{} {}",
                    a.unwrap_or_else(|| "closest-side".into()),
                    b.unwrap_or_else(|| "closest-side".into())
                ),
            };
            let c = center_to_css(center);
            format!("ellipse({}{})", radii, if radii.is_empty() { c.trim_start() } else { c.as_str() })
        }
        ClipPath::Polygon(pts, rule) => {
            let list = pts
                .iter()
                .map(|(x, y)| format!("{} {}", shape_value_to_css(*x), shape_value_to_css(*y)))
                .collect::<Vec<_>>()
                .join(", ");
            match rule {
                crate::style::FillRule::EvenOdd => format!("polygon(evenodd, {list})"),
                crate::style::FillRule::NonZero => format!("polygon({list})"),
            }
        }
        ClipPath::Path(pts, rule) => {
            // Исходная строка пути не хранится — отдаём флэттенный полигон как `path()`.
            let d = pts
                .iter()
                .enumerate()
                .map(|(i, (x, y))| format!("{}{} {}", if i == 0 { "M" } else { "L" }, x, y))
                .collect::<Vec<_>>()
                .join(" ");
            match rule {
                crate::style::FillRule::EvenOdd => format!("path(evenodd, \"{d} Z\")"),
                crate::style::FillRule::NonZero => format!("path(\"{d} Z\")"),
            }
        }
        ClipPath::Url(id) => format!("url(\"#{id}\")"),
        ClipPath::Svg(def) => format!("url(\"#{}\")", def.id),
    }
}

/// Список слоёв по каждому `mask-*`-лонгхенду; при пустом списке — initial.
fn insert_mask(layers: &[MaskLayer], m: &mut HashMap<String, String>) {
    let join = |f: &dyn Fn(&MaskLayer) -> String, initial: &MaskLayer| -> String {
        if layers.is_empty() {
            f(initial)
        } else {
            layers.iter().map(f).collect::<Vec<_>>().join(", ")
        }
    };
    let init = MaskLayer::default();
    let image = |l: &MaskLayer| match &l.image {
        _ if l.image_css.is_some() => l.image_css.as_deref().unwrap_or("none").to_string(),
        BackgroundImage::Url(u) => format!("url(\"{}\")", u.replace('\\', "\\\\").replace('"', "\\\"")),
        _ => "none".to_string(),
    };
    let repeat = |l: &MaskLayer| background_repeat_to_css(l.repeat);
    let size = |l: &MaskLayer| background_size_to_css(l.size);
    let position = |l: &MaskLayer| {
        format!("{} {}", position_component_to_css(l.position.x), position_component_to_css(l.position.y))
    };
    let origin = |l: &MaskLayer| mask_origin_to_css(l.origin).to_string();
    let clip = |l: &MaskLayer| mask_clip_to_css(l.clip).to_string();
    let mode = |l: &MaskLayer| match l.mode {
        // `match-source` сводится к `Alpha` при разборе — отличить его от явного `alpha` нельзя.
        MaskMode::Alpha => "match-source",
        MaskMode::Luminance => "luminance",
    }
    .to_string();
    let composite = |l: &MaskLayer| match l.composite {
        MaskComposite::Add => "add",
        MaskComposite::Subtract => "subtract",
        MaskComposite::Intersect => "intersect",
        MaskComposite::Exclude => "exclude",
    }
    .to_string();

    m.insert("mask-image".into(), join(&image, &init));
    m.insert("mask-repeat".into(), join(&repeat, &init));
    m.insert("mask-size".into(), join(&size, &init));
    m.insert("mask-position".into(), join(&position, &init));
    m.insert("mask-origin".into(), join(&origin, &init));
    m.insert("mask-clip".into(), join(&clip, &init));
    m.insert("mask-mode".into(), join(&mode, &init));
    m.insert("mask-composite".into(), join(&composite, &init));
    // CSS Masking L1 §4.8: `<image> <position> / <size> <repeat> <origin> <clip> <composite> <mode>`
    // на слой, значения по умолчанию опускаются; все слои в initial → `none`.
    let one = |l: &MaskLayer| -> String {
        let d = MaskLayer::default();
        let mut p: Vec<String> = vec![image(l)];
        let pos_size_default = l.position == d.position && l.size == d.size;
        if !pos_size_default {
            let mut ps = position(l);
            if l.size != d.size {
                ps = format!("{ps} / {}", size(l));
            }
            p.push(ps);
        }
        if l.repeat != d.repeat {
            p.push(repeat(l));
        }
        if l.origin != d.origin || l.clip != d.clip {
            p.push(origin(l));
            if l.clip != l.origin_as_clip() {
                p.push(clip(l));
            }
        }
        if l.composite != d.composite {
            p.push(composite(l));
        }
        if l.mode != d.mode {
            p.push(mode(l));
        }
        p.join(" ")
    };
    m.insert("mask".into(), join(&one, &init));
}

fn mask_origin_to_css(o: BackgroundOrigin) -> &'static str {
    background_origin_to_css(o)
}

fn mask_clip_to_css(c: MaskClip) -> &'static str {
    match c {
        MaskClip::BorderBox => "border-box",
        MaskClip::PaddingBox => "padding-box",
        MaskClip::ContentBox => "content-box",
        MaskClip::FillBox => "fill-box",
        MaskClip::StrokeBox => "stroke-box",
        MaskClip::ViewBox => "view-box",
        MaskClip::NoClip => "no-clip",
    }
}

trait OriginAsClip {
    fn origin_as_clip(&self) -> MaskClip;
}

impl OriginAsClip for MaskLayer {
    /// `mask-clip`, который шорткод выводит из одного `<geometry-box>` (origin и clip совпадают).
    fn origin_as_clip(&self) -> MaskClip {
        match self.origin {
            BackgroundOrigin::BorderBox => MaskClip::BorderBox,
            BackgroundOrigin::PaddingBox => MaskClip::PaddingBox,
            BackgroundOrigin::ContentBox => MaskClip::ContentBox,
        }
    }
}
