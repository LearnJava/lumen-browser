//! Минимальный style cascade: для каждого элемента собираем подходящие правила,
//! применяем декларации. Без specificity — last-rule-wins. Без `!important`-разделения.

use lumen_css_parser::{Declaration, Selector, Stylesheet};
use lumen_dom::{Document, NodeData, NodeId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Color,
    pub background_color: Option<Color>,
    pub font_size: f32,
    pub line_height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
}

impl ComputedStyle {
    /// Два стиля рендерят текст одинаково (цвет, размер и интерлиньяж).
    /// Используется для слияния inline-фрагментов в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
    }

    /// Стартовые значения для корня документа.
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            color: Color::BLACK,
            background_color: None,
            font_size: 16.0,
            line_height: 1.2,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
        }
    }
}

pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
) -> ComputedStyle {
    let mut style = ComputedStyle {
        display: default_display(doc, node),
        // Наследуемые свойства (CSS inherited properties).
        color: inherited.color,
        font_size: inherited.font_size,
        line_height: inherited.line_height,
        // Ненаследуемые — сброс.
        background_color: None,
        margin_top: 0.0,
        margin_right: 0.0,
        margin_bottom: 0.0,
        margin_left: 0.0,
        padding_top: 0.0,
        padding_right: 0.0,
        padding_bottom: 0.0,
        padding_left: 0.0,
    };

    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return style;
    };

    let id_attr = attrs.iter().find(|a| a.name.local == "id").map(|a| a.value.as_str());
    let class_attr = attrs
        .iter()
        .find(|a| a.name.local == "class")
        .map(|a| a.value.as_str())
        .unwrap_or("");
    let classes: Vec<&str> = class_attr.split_whitespace().collect();

    for rule in &sheet.rules {
        let matched = rule
            .selectors
            .iter()
            .any(|s| matches_selector(s, &name.local, &classes, id_attr));
        if matched {
            for decl in &rule.declarations {
                apply_declaration(&mut style, decl);
            }
        }
    }

    style
}

fn matches_selector(sel: &Selector, tag: &str, classes: &[&str], id: Option<&str>) -> bool {
    match sel {
        Selector::Type(name) => name == tag,
        Selector::Class(name) => classes.contains(&name.as_str()),
        Selector::Id(name) => id == Some(name.as_str()),
        Selector::Universal => true,
    }
}

fn default_display(doc: &Document, node: NodeId) -> Display {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return Display::Block;
    };
    match name.local.as_str() {
        // <head> и его метаданные никогда не рендерятся как видимый контент.
        // В реальных браузерах это поведение через user-agent stylesheet
        // (`head { display: none; }` и т.д.). У нас встроено в layout-default-ы
        // до появления полноценного UA stylesheet.
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript" => {
            Display::None
        }
        // Inline-уровневые элементы. Phase 0: пока трактуем как block до
        // появления inline-flow с line boxes — текст внутри `<a>`/`<span>`
        // будет на своей строке. Это известное ограничение.
        "a" | "span" | "b" | "i" | "em" | "strong" | "code" | "small" | "sub" | "sup"
        | "label" | "abbr" | "cite" | "q" | "mark" | "u" => Display::Inline,
        _ => Display::Block,
    }
}

fn apply_declaration(style: &mut ComputedStyle, decl: &Declaration) {
    let prop = decl.property.as_str();
    let val = decl.value.as_str();
    match prop {
        "display" => {
            style.display = match val {
                "block" => Display::Block,
                "inline" => Display::Inline,
                "none" => Display::None,
                _ => style.display,
            };
        }
        "color" => {
            if let Some(c) = parse_color(val) {
                style.color = c;
            }
        }
        "background-color" | "background" => {
            if let Some(c) = parse_color(val) {
                style.background_color = Some(c);
            }
        }
        "font-size" => {
            if let Some(v) = parse_length_px(val) {
                style.font_size = v;
            }
        }
        "line-height" => {
            if let Ok(v) = val.parse::<f32>() {
                style.line_height = v;
            } else if let Some(v) = parse_length_px(val) {
                // Если задано в px — переведём в коэффициент относительно font-size.
                style.line_height = v / style.font_size;
            }
        }
        "margin" => {
            if let Some(v) = parse_length_px(val) {
                style.margin_top = v;
                style.margin_right = v;
                style.margin_bottom = v;
                style.margin_left = v;
            }
        }
        "margin-top" => set_px(&mut style.margin_top, val),
        "margin-right" => set_px(&mut style.margin_right, val),
        "margin-bottom" => set_px(&mut style.margin_bottom, val),
        "margin-left" => set_px(&mut style.margin_left, val),
        "padding" => {
            if let Some(v) = parse_length_px(val) {
                style.padding_top = v;
                style.padding_right = v;
                style.padding_bottom = v;
                style.padding_left = v;
            }
        }
        "padding-top" => set_px(&mut style.padding_top, val),
        "padding-right" => set_px(&mut style.padding_right, val),
        "padding-bottom" => set_px(&mut style.padding_bottom, val),
        "padding-left" => set_px(&mut style.padding_left, val),
        _ => {}
    }
}

fn set_px(target: &mut f32, val: &str) {
    if let Some(v) = parse_length_px(val) {
        *target = v;
    }
}

fn parse_length_px(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s.strip_suffix("px").unwrap_or(s);
    s.parse::<f32>().ok()
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    match s.to_ascii_lowercase().as_str() {
        "black" => return Some(Color::BLACK),
        "white" => return Some(Color::WHITE),
        "transparent" => return Some(Color::TRANSPARENT),
        "red" => return Some(Color { r: 255, g: 0, b: 0, a: 255 }),
        "green" => return Some(Color { r: 0, g: 128, b: 0, a: 255 }),
        "blue" => return Some(Color { r: 0, g: 0, b: 255, a: 255 }),
        "gray" | "grey" => return Some(Color { r: 128, g: 128, b: 128, a: 255 }),
        "yellow" => return Some(Color { r: 255, g: 255, b: 0, a: 255 }),
        "orange" => return Some(Color { r: 255, g: 165, b: 0, a: 255 }),
        "purple" => return Some(Color { r: 128, g: 0, b: 128, a: 255 }),
        _ => {}
    }
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            // #RGB → #RRGGBB: каждый ниббл дублируется.
            Some(Color {
                r: r * 17,
                g: g * 17,
                b: b * 17,
                a: 255,
            })
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color { r, g, b, a: 255 })
        }
        _ => None,
    }
}
