//! Style cascade с поддержкой compound и complex selectors, attribute и
//! pseudo-class matching, specificity по CSS Selectors Level 3.
//!
//! Алгоритм каскада: для каждого правила в stylesheet проверяем, матчит ли оно
//! целевой элемент. Если матчит — для каждой декларации записываем «применять с
//! приоритетом (specificity, source_order)». В конце сортируем все
//! применимые декларации по этому ключу (по возрастанию) и применяем — так
//! правило с большей specificity перекрывает меньшую, а при равенстве выигрывает
//! более позднее.
//!
//! Matching complex selector-а — справа налево, жадно: для каждого combinator-а
//! берём первого подходящего предка/sibling-а без back-tracking. Для большинства
//! реальных страниц этого достаточно; патологические случаи `a b c` с
//! вложенными `a`-предками могут промахнуться — это известное упрощение, до
//! фазы со «честным» Selectors-движком.

use lumen_css_parser::{
    AttrOp, AttrSelector, Combinator, CompoundSelector, ComplexSelector, Declaration, PseudoClass,
    SimpleSelector, Specificity, Stylesheet,
};
use lumen_dom::{Attribute, Document, NodeData, NodeId};

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

#[derive(Debug, Clone)]
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

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        return style;
    }

    // Собираем все matched declarations с их sort key:
    // (specificity, rule_order, decl_index). Затем применяем в этом порядке —
    // более поздние/более специфичные перекрывают предыдущие.
    let mut matched: Vec<(Specificity, usize, usize, &Declaration)> = Vec::new();
    for (rule_idx, rule) in sheet.rules.iter().enumerate() {
        let mut best: Option<Specificity> = None;
        for complex in &rule.selectors {
            if matches_complex(complex, doc, node) {
                let spec = complex.specificity();
                best = Some(match best {
                    Some(prev) if prev >= spec => prev,
                    _ => spec,
                });
            }
        }
        if let Some(spec) = best {
            for (decl_idx, decl) in rule.declarations.iter().enumerate() {
                matched.push((spec, rule_idx, decl_idx, decl));
            }
        }
    }
    matched.sort_by_key(|&(spec, rule_idx, decl_idx, _)| (spec, rule_idx, decl_idx));
    for (_, _, _, decl) in &matched {
        apply_declaration(&mut style, decl);
    }

    style
}

// ──────────────── selector matching ────────────────

fn matches_complex(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    // Справа налево: последний compound матчит `node`, дальше идём
    // по combinator-ам в обратную сторону, прыгая по предкам/sibling-ам.
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }

    let n = compounds.len();
    if !matches_compound(compounds[n - 1], doc, node) {
        return false;
    }
    let mut current = node;
    for i in (0..n - 1).rev() {
        let comb = combinators[i];
        let target = compounds[i];
        match comb {
            Combinator::Descendant => {
                let Some(found) = find_ancestor(doc, current, |n| matches_compound(target, doc, n))
                else {
                    return false;
                };
                current = found;
            }
            Combinator::Child => {
                let Some(parent) = doc.get(current).parent else {
                    return false;
                };
                if !is_element(doc, parent) || !matches_compound(target, doc, parent) {
                    return false;
                }
                current = parent;
            }
            Combinator::NextSibling => {
                let Some(prev) = previous_element_sibling(doc, current) else {
                    return false;
                };
                if !matches_compound(target, doc, prev) {
                    return false;
                }
                current = prev;
            }
            Combinator::LaterSibling => {
                let mut sib = previous_element_sibling(doc, current);
                let mut found = None;
                while let Some(s) = sib {
                    if matches_compound(target, doc, s) {
                        found = Some(s);
                        break;
                    }
                    sib = previous_element_sibling(doc, s);
                }
                let Some(f) = found else {
                    return false;
                };
                current = f;
            }
        }
    }
    true
}

fn matches_compound(compound: &CompoundSelector, doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, attrs } = &doc.get(node).data else {
        return false;
    };
    for part in &compound.parts {
        if !matches_simple(part, doc, node, &name.local, attrs) {
            return false;
        }
    }
    true
}

fn matches_simple(
    sel: &SimpleSelector,
    doc: &Document,
    node: NodeId,
    tag: &str,
    attrs: &[Attribute],
) -> bool {
    match sel {
        SimpleSelector::Type(t) => t == tag,
        SimpleSelector::Class(c) => attrs
            .iter()
            .find(|a| a.name.local == "class")
            .map(|a| a.value.split_whitespace().any(|w| w == c))
            .unwrap_or(false),
        SimpleSelector::Id(i) => attrs
            .iter()
            .find(|a| a.name.local == "id")
            .map(|a| a.value == *i)
            .unwrap_or(false),
        SimpleSelector::Universal => true,
        SimpleSelector::Attribute(a) => matches_attribute(a, attrs),
        SimpleSelector::PseudoClass(p) => matches_pseudo_class(p, doc, node),
        SimpleSelector::PseudoElement(_) => false,
    }
}

fn matches_attribute(sel: &AttrSelector, attrs: &[Attribute]) -> bool {
    let Some(attr) = attrs.iter().find(|a| a.name.local == sel.name) else {
        return false;
    };
    match (sel.op, sel.value.as_deref()) {
        (None, _) => true,
        (Some(AttrOp::Equals), Some(v)) => attr.value == v,
        (Some(AttrOp::Includes), Some(v)) => {
            !v.is_empty() && attr.value.split_whitespace().any(|w| w == v)
        }
        (Some(AttrOp::DashMatch), Some(v)) => {
            attr.value == v || attr.value.starts_with(&format!("{v}-"))
        }
        (Some(AttrOp::Prefix), Some(v)) => !v.is_empty() && attr.value.starts_with(v),
        (Some(AttrOp::Suffix), Some(v)) => !v.is_empty() && attr.value.ends_with(v),
        (Some(AttrOp::Substring), Some(v)) => !v.is_empty() && attr.value.contains(v),
        _ => false,
    }
}

fn matches_pseudo_class(p: &PseudoClass, doc: &Document, node: NodeId) -> bool {
    match p {
        PseudoClass::FirstChild => is_first_element_child(doc, node),
        PseudoClass::LastChild => is_last_element_child(doc, node),
        PseudoClass::OnlyChild => {
            is_first_element_child(doc, node) && is_last_element_child(doc, node)
        }
        PseudoClass::Empty => is_empty_element(doc, node),
        PseudoClass::Root => is_root_element(doc, node),
        PseudoClass::Unsupported(_) => false,
    }
}

// ──────────────── DOM-traversal хелперы ────────────────

fn is_element(doc: &Document, node: NodeId) -> bool {
    matches!(doc.get(node).data, NodeData::Element { .. })
}

fn find_ancestor<F: Fn(NodeId) -> bool>(
    doc: &Document,
    node: NodeId,
    pred: F,
) -> Option<NodeId> {
    let mut p = doc.get(node).parent;
    while let Some(pid) = p {
        if is_element(doc, pid) && pred(pid) {
            return Some(pid);
        }
        p = doc.get(pid).parent;
    }
    None
}

fn previous_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[..idx]
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
}

fn is_first_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_last_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_empty_element(doc: &Document, node: NodeId) -> bool {
    // `:empty` — нет ни элементов-детей, ни текстовых узлов с непустым контентом.
    doc.get(node).children.iter().all(|&cid| {
        matches!(
            doc.get(cid).data,
            NodeData::Comment(_) | NodeData::Doctype { .. }
        ) || matches!(&doc.get(cid).data, NodeData::Text(t) if t.is_empty())
    })
}

fn is_root_element(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    matches!(doc.get(parent).data, NodeData::Document)
}

// ──────────────── default display / declarations ────────────────

fn default_display(doc: &Document, node: NodeId) -> Display {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return Display::Block;
    };
    match name.local.as_str() {
        // <head> и его метаданные никогда не рендерятся как видимый контент.
        "head" | "title" | "style" | "script" | "meta" | "link" | "base" | "noscript" => {
            Display::None
        }
        // Inline-уровневые элементы. Phase 0: пока трактуем как block — текст
        // внутри `<a>`/`<span>` будет на своей строке. Это известное ограничение.
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
