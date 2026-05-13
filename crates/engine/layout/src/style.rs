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

use lumen_core::geom::Size;
use lumen_css_parser::{
    AttrOp, AttrSelector, Combinator, ComplexSelector, CompoundSelector, Declaration, PseudoClass,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// CSS Text Module L3 §3.4 — `text-transform`. Inherited.
///
/// Применяется к текстовому содержимому при сборке inline-сегментов, до
/// word-wrapping и measurer-а. Cyrillic case-folding делается через
/// `char::to_uppercase` / `to_lowercase` стандартной библиотеки, что даёт
/// правильную обработку русских букв (А↔а, Я↔я и т.д.) без сюрпризов
/// типа турецкого `i`/`I`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    /// `capitalize`: первая буква каждого «слова» (по spec — character с
    /// Unicode property Letter) в верхний регистр. Phase 0: упрощённо —
    /// первая буква каждого whitespace-разделённого токена.
    Capitalize,
}

impl TextTransform {
    /// Применяет преобразование к строке. Не аллоцирует, если transform = None.
    pub fn apply(self, s: &str) -> String {
        match self {
            TextTransform::None => s.to_string(),
            TextTransform::Uppercase => s.to_uppercase(),
            TextTransform::Lowercase => s.to_lowercase(),
            TextTransform::Capitalize => {
                let mut out = String::with_capacity(s.len());
                let mut at_word_start = true;
                for ch in s.chars() {
                    if ch.is_whitespace() {
                        out.push(ch);
                        at_word_start = true;
                    } else if at_word_start {
                        out.extend(ch.to_uppercase());
                        at_word_start = false;
                    } else {
                        out.push(ch);
                    }
                }
                out
            }
        }
    }
}

/// CSS Fonts Module L4: `font-style: normal | italic | oblique`. Inherited.
///
/// Phase 0: layout различает свойство, рендерер пока использует один
/// шрифтовой файл (Inter Regular) и не отрисовывает italic-вариант. Поле
/// нужно, чтобы `text_rendering_eq` правильно разделял inline-фрагменты
/// — это корректно подготавливает структуру под подключение Italic-fontfile
/// или affine-skew transform позже.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// CSS Fonts Module L4 §2.4 — `font-weight`. Inherited.
///
/// Хранится численно (1..1000), как в spec: `normal` = 400, `bold` = 700.
/// Ключевые слова `lighter` / `bolder` относительные — их разрешение
/// (по правилам §2.4.3) делается при парсинге: смотрим на родительский weight
/// и сдвигаем по таблице. `lighter` от 400 = 100; `bolder` от 400 = 700.
///
/// Phase 0: layout различает свойство, рендерер пока всегда Inter Regular —
/// real bold-варианта файлов нет. text_rendering_eq учитывает weight, чтобы
/// bold-фрагменты не сливались с обычными.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const NORMAL: Self = Self(400);
    pub const BOLD: Self = Self(700);

    pub fn is_bold(self) -> bool {
        self.0 >= 600
    }
}

impl Default for FontWeight {
    fn default() -> Self { Self::NORMAL }
}

/// Набор активных линий `text-decoration` для элемента.
///
/// CSS3 разделяет shorthand `text-decoration` на `-line`, `-style`, `-color`;
/// Phase 0 умеет только line (без двойных линий и кастомных цветов). Спецификация
/// CSS3 не наследует text-decoration-line, но визуально декорация всё равно
/// распространяется на потомков. Мы делаем явное наследование — это эквивалентно
/// поведению, ожидаемому от `a { text-decoration: underline }`, и при этом
/// позволяет дочернему элементу явно сбросить декорацию через
/// `text-decoration: none` (CSS3 для этого требует пересоздать stacking context,
/// но в нашей упрощённой модели достаточно перезаписать поле).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextDecorationLine {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

impl TextDecorationLine {
    pub const fn is_empty(self) -> bool {
        !self.underline && !self.overline && !self.line_through
    }
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

/// Стиль линии CSS border. None = рамка не отображается (как `display: none`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None)
    }
}

/// CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
///   - `ContentBox` (CSS default): размер контента; padding и border прибавляются сверху.
///   - `BorderBox`: размер вместе с padding и border; контент сжимается, чтобы влезть.
///
/// Свойство НЕ наследуется (CSS Basic UI 3 §4.1) — сбрасывается на default в каждом
/// `compute_style`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub text_align: TextAlign,
    pub color: Color,
    pub background_color: Option<Color>,
    pub font_size: f32,
    pub line_height: f32,
    pub font_style: FontStyle,
    pub font_weight: FontWeight,
    pub text_transform: TextTransform,
    /// CSS Text L3 §7.1: отступ перед первой строкой inline-content
    /// текущего блока (resolved px). Inherited; применяется к каждому
    /// потомку, который порождает первую строку.
    pub text_indent: f32,
    pub text_decoration_line: TextDecorationLine,
    /// Явная ширина (CSS `width: Npx`). None = auto (растягивается на контейнер).
    pub width: Option<f32>,
    /// Явная высота (CSS `height: Npx`). None = auto (по содержимому).
    pub height: Option<f32>,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
    /// None = currentColor (используется style.color при рендеринге).
    pub border_top_color: Option<Color>,
    pub border_right_color: Option<Color>,
    pub border_bottom_color: Option<Color>,
    pub border_left_color: Option<Color>,
    pub box_sizing: BoxSizing,
}

impl ComputedStyle {
    /// Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
    /// насыщенность, декорация). Используется для слияния inline-фрагментов
    /// в wrap_inline_run.
    pub fn text_rendering_eq(&self, other: &Self) -> bool {
        self.color == other.color
            && (self.font_size - other.font_size).abs() < f32::EPSILON
            && (self.line_height - other.line_height).abs() < f32::EPSILON
            && self.font_style == other.font_style
            && self.font_weight == other.font_weight
            && self.text_decoration_line == other.text_decoration_line
    }

    /// Стартовые значения для корня документа.
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            text_align: TextAlign::Left,
            color: Color::BLACK,
            background_color: None,
            font_size: 16.0,
            line_height: 1.2,
            font_style: FontStyle::Normal,
            font_weight: FontWeight::NORMAL,
            text_transform: TextTransform::None,
            text_indent: 0.0,
            text_decoration_line: TextDecorationLine::default(),
            width: None,
            height: None,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            border_top_color: None,
            border_right_color: None,
            border_bottom_color: None,
            border_left_color: None,
            box_sizing: BoxSizing::ContentBox,
        }
    }
}

pub fn compute_style(
    doc: &Document,
    node: NodeId,
    sheet: &Stylesheet,
    inherited: &ComputedStyle,
    viewport: Size,
) -> ComputedStyle {
    let mut style = ComputedStyle {
        display: default_display(doc, node),
        // Наследуемые свойства (CSS inherited properties).
        color: inherited.color,
        text_align: inherited.text_align,
        font_size: inherited.font_size,
        line_height: inherited.line_height,
        font_style: inherited.font_style,
        font_weight: inherited.font_weight,
        text_transform: inherited.text_transform,
        text_indent: inherited.text_indent,
        text_decoration_line: inherited.text_decoration_line,
        // Ненаследуемые — сброс.
        background_color: None,
        width: None,
        height: None,
        margin_top: 0.0,
        margin_right: 0.0,
        margin_bottom: 0.0,
        margin_left: 0.0,
        padding_top: 0.0,
        padding_right: 0.0,
        padding_bottom: 0.0,
        padding_left: 0.0,
        border_top_width: 0.0,
        border_right_width: 0.0,
        border_bottom_width: 0.0,
        border_left_width: 0.0,
        border_top_style: BorderStyle::None,
        border_right_style: BorderStyle::None,
        border_bottom_style: BorderStyle::None,
        border_left_style: BorderStyle::None,
        border_top_color: None,
        border_right_color: None,
        border_bottom_color: None,
        border_left_color: None,
        box_sizing: BoxSizing::ContentBox,
    };

    if !matches!(doc.get(node).data, NodeData::Element { .. }) {
        return style;
    }

    // UA stylesheet: семантические элементы получают italic / bold по
    // умолчанию, CSS-декларации ниже могут это переопределить.
    if let Some(fs) = ua_font_style(doc, node) {
        style.font_style = fs;
    }
    if let Some(fw) = ua_font_weight(doc, node) {
        style.font_weight = fw;
    }

    // Собираем все matched declarations с их sort key:
    // (important, specificity, rule_order, decl_index). `important` идёт
    // первым: после ascending sort `true > false`, поэтому !important идёт в
    // конец и побеждает normal даже при меньшей specificity (CSS Cascade L4
    // §8.1). Внутри одного origin `important = false` сначала разрешается
    // обычный каскад, потом тот же каскад применяется поверх с !important.
    let mut matched: Vec<(bool, Specificity, usize, usize, &Declaration)> = Vec::new();
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
                matched.push((decl.important, spec, rule_idx, decl_idx, decl));
            }
        }
    }
    matched.sort_by_key(|&(imp, spec, rule_idx, decl_idx, _)| (imp, spec, rule_idx, decl_idx));

    // Pre-pass: применяем font-size раньше, потому что em/% других свойств
    // считаются относительно computed font-size этого же элемента, а em для
    // самого font-size — относительно inherited (родительского) font-size.
    let parent_fs = inherited.font_size;
    for (_, _, _, _, decl) in &matched {
        apply_font_size(&mut style, decl, parent_fs, viewport);
    }

    // Main-pass: остальные декларации; em-basis теперь = current font_size.
    // Inherited font_weight нужен для разрешения `lighter`/`bolder`.
    let em_basis = style.font_size;
    let parent_weight = inherited.font_weight;
    for (_, _, _, _, decl) in &matched {
        apply_declaration(&mut style, decl, em_basis, viewport, parent_weight);
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
    let ci = sel.case_insensitive;
    match (sel.op, sel.value.as_deref()) {
        (None, _) => true,
        (Some(AttrOp::Equals), Some(v)) => str_eq(&attr.value, v, ci),
        (Some(AttrOp::Includes), Some(v)) => {
            !v.is_empty() && attr.value.split_whitespace().any(|w| str_eq(w, v, ci))
        }
        (Some(AttrOp::DashMatch), Some(v)) => {
            // Точное совпадение или префикс с разделителем `-`. `i` применяется
            // к обеим частям сравнения (CSS L4 §6.3.6).
            str_eq(&attr.value, v, ci) || str_starts_with(&attr.value, &format!("{v}-"), ci)
        }
        (Some(AttrOp::Prefix), Some(v)) => !v.is_empty() && str_starts_with(&attr.value, v, ci),
        (Some(AttrOp::Suffix), Some(v)) => !v.is_empty() && str_ends_with(&attr.value, v, ci),
        (Some(AttrOp::Substring), Some(v)) => !v.is_empty() && str_contains(&attr.value, v, ci),
        _ => false,
    }
}

/// ASCII case-insensitive (если `ci`) сравнение, иначе побайтовое. Cyrillic и
/// другой не-ASCII всегда сравнивается побайтово (`eq_ignore_ascii_case` не
/// трогает байты со старшим битом). Работа через `as_bytes()` нужна, чтобы
/// `starts_with`/`ends_with`/`contains` не упирались в char-boundary в
/// многобайтовых UTF-8 строках.
fn str_eq(a: &str, b: &str, ci: bool) -> bool {
    if ci { a.eq_ignore_ascii_case(b) } else { a == b }
}

fn str_starts_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.starts_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[..n.len()].eq_ignore_ascii_case(n)
}

fn str_ends_with(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.ends_with(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.len() >= n.len() && h[h.len() - n.len()..].eq_ignore_ascii_case(n)
}

fn str_contains(haystack: &str, needle: &str, ci: bool) -> bool {
    if !ci {
        return haystack.contains(needle);
    }
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    if h.len() < n.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| h[i..i + n.len()].eq_ignore_ascii_case(n))
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
        PseudoClass::FirstOfType => is_first_of_type(doc, node),
        PseudoClass::LastOfType => is_last_of_type(doc, node),
        PseudoClass::OnlyOfType => is_first_of_type(doc, node) && is_last_of_type(doc, node),
        PseudoClass::NthChild(spec) => match element_index(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastChild(spec) => match element_index(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthOfType(spec) => match element_index_of_type(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastOfType(spec) => match element_index_of_type(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::Not(inner) => !matches_compound(inner, doc, node),
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            // CSS4 §17: матчит, если матчит хоть один селектор из списка.
            // `:where(...)` отличается только тем, что contributes 0 specificity —
            // matching identical с `:is`.
            list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Unsupported(_) => false,
    }
}

/// 1-based индекс элемента среди element-sibling-ов. Если `from_end` —
/// считаем с конца. None — если узел не элемент или нет родителя.
fn element_index(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    if !is_element(doc, node) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов **того же тега**.
fn element_index_of_type(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    let self_name = match &doc.get(node).data {
        NodeData::Element { name, .. } => name,
        _ => return None,
    };
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        let same_type = matches!(
            &doc.get(id).data,
            NodeData::Element { name, .. } if name == self_name
        );
        if !same_type {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

fn is_first_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, false) == Some(1)
}

fn is_last_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, true) == Some(1)
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

/// Эмулирует UA stylesheet для font-style: HTML §15.3.3 рекомендует italic
/// для `<em>` / `<i>` / `<cite>` / `<dfn>` / `<address>` / `<var>`. Возвращает
/// `Some(Italic)` для них, `None` для остальных (= наследовать как обычно).
fn ua_font_style(doc: &Document, node: NodeId) -> Option<FontStyle> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "em" | "i" | "cite" | "dfn" | "address" | "var" => Some(FontStyle::Italic),
        _ => None,
    }
}

/// UA stylesheet для font-weight: `<b>`, `<strong>`, `<th>`, `<h1>`–`<h6>`
/// получают bold по умолчанию (HTML §15.3.3).
fn ua_font_weight(doc: &Document, node: NodeId) -> Option<FontWeight> {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return None;
    };
    match name.local.as_str() {
        "b" | "strong" | "th" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            Some(FontWeight::BOLD)
        }
        _ => None,
    }
}

/// Парсит CSS `font-weight`. Поддерживает:
///   - `normal` → 400, `bold` → 700;
///   - численные `100`..`900` (или любое число 1..1000 — Variable Fonts);
///   - относительные `lighter` / `bolder` — резолвятся относительно `parent`
///     по таблице из CSS Fonts L4 §2.4.3.
fn parse_font_weight(val: &str, parent: FontWeight) -> Option<FontWeight> {
    match val.trim() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        "lighter" => Some(relative_lighter(parent)),
        "bolder" => Some(relative_bolder(parent)),
        s => s.parse::<u16>().ok().filter(|&n| (1..=1000).contains(&n)).map(FontWeight),
    }
}

/// CSS Fonts L4 §2.4.3 таблица для `lighter`. Сужаем weight в сторону normal.
fn relative_lighter(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        100..=349 => 100,
        350..=549 => 100,
        550..=749 => 400,
        _ => 700, // 750..=1000
    })
}

/// CSS Fonts L4 §2.4.3 таблица для `bolder`.
fn relative_bolder(parent: FontWeight) -> FontWeight {
    let w = parent.0;
    FontWeight(match w {
        0..=349 => 400,
        350..=549 => 700,
        550..=749 => 900,
        _ => 900,
    })
}

/// Корневой font-size в CSS — 16px на момент Phase 0 (без `<html>`-стилей и
/// настроек пользователя). Используется как базис для `rem`.
pub const ROOT_FONT_SIZE: f32 = 16.0;

/// Типизированная длина CSS до резолва в пиксели.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    /// `em` — относительно font-size текущего/родительского элемента
    /// (для свойства `font-size` — родительского, для остального — текущего).
    Em(f32),
    /// `rem` — относительно font-size корня документа (ROOT_FONT_SIZE).
    Rem(f32),
    /// `%` — процент. Базис зависит от свойства: для `font-size` это
    /// `em_basis`, для `line-height` — текущий font-size, для
    /// margin/padding/width — containing block width (Phase 0 пока не считает,
    /// нужны honest contain blocks; до тех пор `%` в margin/padding
    /// игнорируется).
    Percent(f32),
    /// `vh` — 1% от высоты viewport (CSS Values L3 §6.1.2).
    Vh(f32),
    /// `vw` — 1% от ширины viewport.
    Vw(f32),
    /// `vmin` — 1% от меньшей из двух сторон viewport.
    Vmin(f32),
    /// `vmax` — 1% от большей из двух сторон viewport.
    Vmax(f32),
}

impl Length {
    /// Возвращает длину в пикселях. `em_basis` — fs, относительно которого
    /// считать `em` (родителя для font-size; текущего элемента для остального).
    /// `percent_basis` — длина, относительно которой считать `%` (None если
    /// контекст ещё не определён — тогда `%` даёт None).
    /// `viewport` — размер viewport-а для `vh`/`vw`/`vmin`/`vmax`.
    pub fn resolve(&self, em_basis: f32, percent_basis: Option<f32>, viewport: Size) -> Option<f32> {
        match *self {
            Length::Px(v) => Some(v),
            Length::Em(v) => Some(v * em_basis),
            Length::Rem(v) => Some(v * ROOT_FONT_SIZE),
            Length::Percent(v) => percent_basis.map(|b| v / 100.0 * b),
            Length::Vh(v) => Some(v / 100.0 * viewport.height),
            Length::Vw(v) => Some(v / 100.0 * viewport.width),
            Length::Vmin(v) => Some(v / 100.0 * viewport.width.min(viewport.height)),
            Length::Vmax(v) => Some(v / 100.0 * viewport.width.max(viewport.height)),
        }
    }
}

/// Парсит CSS-длину: число + опциональная единица (`px`, `em`, `rem`, `%`,
/// `vh`/`vw`/`vmin`/`vmax`). Голое число (`0`) считаем `Px(0)` — CSS позволяет
/// опускать единицу только для нуля, но мы прощаем и для других чисел.
///
/// Порядок проверки суффиксов важен: более длинные сначала (`vmin`/`vmax`
/// перед `vw`/`vh`, `rem` перед `em`).
pub fn parse_length(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("px") {
        return num.trim().parse::<f32>().ok().map(Length::Px);
    }
    if let Some(num) = s.strip_suffix("rem") {
        return num.trim().parse::<f32>().ok().map(Length::Rem);
    }
    if let Some(num) = s.strip_suffix("em") {
        return num.trim().parse::<f32>().ok().map(Length::Em);
    }
    if let Some(num) = s.strip_suffix("vmin") {
        return num.trim().parse::<f32>().ok().map(Length::Vmin);
    }
    if let Some(num) = s.strip_suffix("vmax") {
        return num.trim().parse::<f32>().ok().map(Length::Vmax);
    }
    if let Some(num) = s.strip_suffix("vh") {
        return num.trim().parse::<f32>().ok().map(Length::Vh);
    }
    if let Some(num) = s.strip_suffix("vw") {
        return num.trim().parse::<f32>().ok().map(Length::Vw);
    }
    if let Some(num) = s.strip_suffix('%') {
        return num.trim().parse::<f32>().ok().map(Length::Percent);
    }
    s.parse::<f32>().ok().map(Length::Px)
}

fn apply_declaration(
    style: &mut ComputedStyle,
    decl: &Declaration,
    em_basis: f32,
    viewport: Size,
    parent_font_weight: FontWeight,
) {
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
        "text-align" => {
            style.text_align = match val {
                "left" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => style.text_align,
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
        "width" if val != "auto" => {
            style.width = parse_length(val).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        "height" if val != "auto" => {
            style.height = parse_length(val).and_then(|l| l.resolve(em_basis, None, viewport));
        }
        "font-size" => {
            // Обрабатывается в pre-pass; в этой ветке пропускаем.
        }
        "font-style" => {
            // CSS Fonts L4 — normal | italic | oblique. Прочее (`oblique 10deg`,
            // `oblique -5deg`) пока не поддерживаем — берём как oblique.
            style.font_style = match val.split_whitespace().next() {
                Some("italic") => FontStyle::Italic,
                Some("oblique") => FontStyle::Oblique,
                Some("normal") => FontStyle::Normal,
                _ => style.font_style,
            };
        }
        "font-weight" => {
            if let Some(w) = parse_font_weight(val, parent_font_weight) {
                style.font_weight = w;
            }
        }
        "text-indent" => {
            // CSS Text L3 §7.1: <length> | <percentage>. % требует
            // containing-block-width — Phase 0 пока игнорирует, как и в
            // margin/padding. Поддерживаем px/em/rem/vh/vw.
            if let Some(len) = parse_length(val)
                && let Some(px) = match len {
                    Length::Percent(_) => None,
                    other => other.resolve(em_basis, None, viewport),
                }
            {
                style.text_indent = px;
            }
        }
        "text-transform" => {
            // CSS Text L3: none | uppercase | lowercase | capitalize.
            // `full-width` / `full-size-kana` отложены (CJK-специфика).
            style.text_transform = match val.split_whitespace().next() {
                Some("none") => TextTransform::None,
                Some("uppercase") => TextTransform::Uppercase,
                Some("lowercase") => TextTransform::Lowercase,
                Some("capitalize") => TextTransform::Capitalize,
                _ => style.text_transform,
            };
        }
        "line-height" => {
            // `1.5` (unitless) — коэффициент. `1.5em` — то же самое.
            // `150%` — то же самое. `24px` / `5vh` — конкретная высота,
            // переводим в коэффициент / font_size.
            if let Ok(v) = val.parse::<f32>() {
                style.line_height = v;
            } else if let Some(len) = parse_length(val) {
                match len {
                    Length::Px(v) => style.line_height = v / style.font_size,
                    Length::Em(v) => style.line_height = v,
                    Length::Rem(v) => style.line_height = v * ROOT_FONT_SIZE / style.font_size,
                    Length::Percent(v) => style.line_height = v / 100.0,
                    Length::Vh(_) | Length::Vw(_) | Length::Vmin(_) | Length::Vmax(_) => {
                        if let Some(px) = len.resolve(em_basis, None, viewport) {
                            style.line_height = px / style.font_size;
                        }
                    }
                }
            }
        }
        "margin" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.margin_top = v;
                style.margin_right = v;
                style.margin_bottom = v;
                style.margin_left = v;
            }
        }
        "margin-top" => set_box_length(&mut style.margin_top, val, em_basis, viewport),
        "margin-right" => set_box_length(&mut style.margin_right, val, em_basis, viewport),
        "margin-bottom" => set_box_length(&mut style.margin_bottom, val, em_basis, viewport),
        "margin-left" => set_box_length(&mut style.margin_left, val, em_basis, viewport),
        "padding" => {
            if let Some(v) = resolve_box_length(val, em_basis, viewport) {
                style.padding_top = v;
                style.padding_right = v;
                style.padding_bottom = v;
                style.padding_left = v;
            }
        }
        "padding-top" => set_box_length(&mut style.padding_top, val, em_basis, viewport),
        "padding-right" => set_box_length(&mut style.padding_right, val, em_basis, viewport),
        "padding-bottom" => set_box_length(&mut style.padding_bottom, val, em_basis, viewport),
        "padding-left" => set_box_length(&mut style.padding_left, val, em_basis, viewport),
        "text-decoration" | "text-decoration-line" => {
            if let Some(d) = parse_text_decoration(val) {
                style.text_decoration_line = d;
            }
        }
        // ── Borders ───────────────────────────────────────────────────────────
        "border" => apply_border_shorthand(style, val, em_basis, viewport),
        "border-top" => apply_border_side_shorthand(
            &mut style.border_top_width, &mut style.border_top_style,
            &mut style.border_top_color, val, em_basis, viewport),
        "border-right" => apply_border_side_shorthand(
            &mut style.border_right_width, &mut style.border_right_style,
            &mut style.border_right_color, val, em_basis, viewport),
        "border-bottom" => apply_border_side_shorthand(
            &mut style.border_bottom_width, &mut style.border_bottom_style,
            &mut style.border_bottom_color, val, em_basis, viewport),
        "border-left" => apply_border_side_shorthand(
            &mut style.border_left_width, &mut style.border_left_style,
            &mut style.border_left_color, val, em_basis, viewport),
        "border-width" => {
            let sides = expand_border_4(val);
            if let Some(v) = resolve_box_length(sides[0], em_basis, viewport) { style.border_top_width = v; }
            if let Some(v) = resolve_box_length(sides[1], em_basis, viewport) { style.border_right_width = v; }
            if let Some(v) = resolve_box_length(sides[2], em_basis, viewport) { style.border_bottom_width = v; }
            if let Some(v) = resolve_box_length(sides[3], em_basis, viewport) { style.border_left_width = v; }
        }
        "border-style" => {
            let sides = expand_border_4(val);
            style.border_top_style = parse_border_style_kw(sides[0]);
            style.border_right_style = parse_border_style_kw(sides[1]);
            style.border_bottom_style = parse_border_style_kw(sides[2]);
            style.border_left_style = parse_border_style_kw(sides[3]);
        }
        "border-color" => {
            let sides = expand_border_4(val);
            if let Some(c) = parse_color(sides[0]) { style.border_top_color = Some(c); }
            if let Some(c) = parse_color(sides[1]) { style.border_right_color = Some(c); }
            if let Some(c) = parse_color(sides[2]) { style.border_bottom_color = Some(c); }
            if let Some(c) = parse_color(sides[3]) { style.border_left_color = Some(c); }
        }
        "border-top-width" => set_box_length(&mut style.border_top_width, val, em_basis, viewport),
        "border-right-width" => set_box_length(&mut style.border_right_width, val, em_basis, viewport),
        "border-bottom-width" => set_box_length(&mut style.border_bottom_width, val, em_basis, viewport),
        "border-left-width" => set_box_length(&mut style.border_left_width, val, em_basis, viewport),
        "border-top-style" => style.border_top_style = parse_border_style_kw(val),
        "border-right-style" => style.border_right_style = parse_border_style_kw(val),
        "border-bottom-style" => style.border_bottom_style = parse_border_style_kw(val),
        "border-left-style" => style.border_left_style = parse_border_style_kw(val),
        "border-top-color" => { if let Some(c) = parse_color(val) { style.border_top_color = Some(c); } }
        "border-right-color" => { if let Some(c) = parse_color(val) { style.border_right_color = Some(c); } }
        "border-bottom-color" => { if let Some(c) = parse_color(val) { style.border_bottom_color = Some(c); } }
        "border-left-color" => { if let Some(c) = parse_color(val) { style.border_left_color = Some(c); } }
        "box-sizing" => {
            style.box_sizing = match val.trim().to_ascii_lowercase().as_str() {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => style.box_sizing,
            };
        }
        _ => {}
    }
}

/// Разбирает `text-decoration` / `text-decoration-line`. Phase 0: только
/// набор keyword-ов `underline`, `overline`, `line-through`, `none`. Цвет,
/// стиль (`solid`/`wavy`/…) и `blink` (CSS2 deprecated) тихо игнорируем.
/// `none` сбрасывает все линии, даже если вместе с ним встречены другие
/// keyword-ы (CSS3 описывает это как «none — initial value», но интуитивно
/// побеждает явный сброс).
fn parse_text_decoration(val: &str) -> Option<TextDecorationLine> {
    let mut out = TextDecorationLine::default();
    let mut any_known = false;
    let mut none_seen = false;
    for token in val.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "none" => {
                none_seen = true;
                any_known = true;
            }
            "underline" => {
                out.underline = true;
                any_known = true;
            }
            "overline" => {
                out.overline = true;
                any_known = true;
            }
            "line-through" => {
                out.line_through = true;
                any_known = true;
            }
            // Цвета, `solid`/`wavy`/`dashed`/…, `blink` — игнорируем молча.
            _ => {}
        }
    }
    if !any_known {
        return None;
    }
    if none_seen {
        return Some(TextDecorationLine::default());
    }
    Some(out)
}

/// Применяет `font-size`-декларацию, если она задана. Размер `em` берётся
/// относительно `parent_fs` (родительский font-size), `rem` — относительно
/// ROOT_FONT_SIZE, `%` — относительно `parent_fs`.
fn apply_font_size(
    style: &mut ComputedStyle,
    decl: &Declaration,
    parent_fs: f32,
    viewport: Size,
) {
    if decl.property != "font-size" {
        return;
    }
    let val = decl.value.as_str();
    let Some(len) = parse_length(val) else {
        return;
    };
    // Для font-size: em и % считаются от parent_fs; vh/vw/vmin/vmax — от viewport.
    style.font_size = match len {
        Length::Px(v) => v,
        Length::Em(v) => v * parent_fs,
        Length::Rem(v) => v * ROOT_FONT_SIZE,
        Length::Percent(v) => v / 100.0 * parent_fs,
        Length::Vh(v) => v / 100.0 * viewport.height,
        Length::Vw(v) => v / 100.0 * viewport.width,
        Length::Vmin(v) => v / 100.0 * viewport.width.min(viewport.height),
        Length::Vmax(v) => v / 100.0 * viewport.width.max(viewport.height),
    };
}

/// Резолвит длину для margin / padding / border. `%` в Phase 0 не поддержан
/// (нужна containing-block-width), возвращает None.
fn resolve_box_length(val: &str, em_basis: f32, viewport: Size) -> Option<f32> {
    let len = parse_length(val)?;
    match len {
        Length::Percent(_) => None,
        other => other.resolve(em_basis, None, viewport),
    }
}

fn set_box_length(target: &mut f32, val: &str, em_basis: f32, viewport: Size) {
    if let Some(v) = resolve_box_length(val, em_basis, viewport) {
        *target = v;
    }
}

fn is_border_style_kw(s: &str) -> bool {
    matches!(s.trim(), "none" | "solid" | "dashed" | "dotted")
}

fn parse_border_style_kw(s: &str) -> BorderStyle {
    match s.trim() {
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        _ => BorderStyle::None,
    }
}

/// Разбирает `border: <width> <style> <color>` (порядок произвольный, каждая
/// часть опциональна). Применяет найденные значения ко всем четырём сторонам.
fn apply_border_shorthand(style: &mut ComputedStyle, val: &str, em_basis: f32, viewport: Size) {
    let tokens: Vec<&str> = val.split_whitespace().collect();
    for tok in &tokens {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport) {
            style.border_top_width = v;
            style.border_right_width = v;
            style.border_bottom_width = v;
            style.border_left_width = v;
        } else if is_border_style_kw(tok) {
            let bs = parse_border_style_kw(tok);
            style.border_top_style = bs;
            style.border_right_style = bs;
            style.border_bottom_style = bs;
            style.border_left_style = bs;
        } else if let Some(c) = parse_color(tok) {
            style.border_top_color = Some(c);
            style.border_right_color = Some(c);
            style.border_bottom_color = Some(c);
            style.border_left_color = Some(c);
        }
    }
}

/// Разбирает `border-{top,right,bottom,left}: <width> <style> <color>` в одну сторону.
fn apply_border_side_shorthand(
    width: &mut f32,
    bstyle: &mut BorderStyle,
    color: &mut Option<Color>,
    val: &str,
    em_basis: f32,
    viewport: Size,
) {
    for tok in val.split_whitespace() {
        if let Some(v) = resolve_box_length(tok, em_basis, viewport) {
            *width = v;
        } else if is_border_style_kw(tok) {
            *bstyle = parse_border_style_kw(tok);
        } else if let Some(c) = parse_color(tok) {
            *color = Some(c);
        }
    }
}

/// Разворачивает 1–4 токена в 4-элементный массив по CSS-правилу:
/// 1 → (T, R, B, L) = all same
/// 2 → (T=B, R=L)
/// 3 → (T, R=L, B)
/// 4 → (T, R, B, L)
fn expand_border_4(val: &str) -> [&str; 4] {
    let parts: Vec<&str> = val.split_whitespace().collect();
    match parts.len() {
        1 => [parts[0], parts[0], parts[0], parts[0]],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        _ => {
            let t = parts[0];
            let r = parts.get(1).copied().unwrap_or(t);
            let b = parts.get(2).copied().unwrap_or(t);
            let l = parts.get(3).copied().unwrap_or(r);
            [t, r, b, l]
        }
    }
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(c) = named_color(&s.to_ascii_lowercase()) {
        return Some(c);
    }
    if let Some(c) = parse_hex_color(s) {
        return Some(c);
    }
    parse_function_color(s)
}

/// CSS Color Module Level 3 §4.3 — X11 / SVG named colors. Принимает имя
/// уже в нижнем регистре. Возвращает None для неизвестного имени.
///
/// Реализовано бинарным поиском по сортированному списку: O(log n) на
/// lookup, no allocations, читается как табличный data-driven код.
/// `transparent` (CSS Color L3) — отдельная константа, потому что у него
/// alpha = 0. `currentcolor` не реализуется здесь — это keyword уровня
/// каскада, требующий доступа к computed `color`.
fn named_color(name_lc: &str) -> Option<Color> {
    if name_lc == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    NAMED_COLORS
        .binary_search_by_key(&name_lc, |&(n, _)| n)
        .ok()
        .map(|i| {
            let (_, (r, g, b)) = NAMED_COLORS[i];
            Color { r, g, b, a: 255 }
        })
}

/// Таблица CSS3 named colors (147 имён), отсортированная по имени для
/// бинарного поиска. `grey`-варианты и `gray`-варианты — оба перечислены.
/// Имена из CSS Color Module Level 4 §6.1: `rebeccapurple` тоже включён.
#[rustfmt::skip]
const NAMED_COLORS: &[(&str, (u8, u8, u8))] = &[
    ("aliceblue",            (240, 248, 255)),
    ("antiquewhite",         (250, 235, 215)),
    ("aqua",                 (  0, 255, 255)),
    ("aquamarine",           (127, 255, 212)),
    ("azure",                (240, 255, 255)),
    ("beige",                (245, 245, 220)),
    ("bisque",               (255, 228, 196)),
    ("black",                (  0,   0,   0)),
    ("blanchedalmond",       (255, 235, 205)),
    ("blue",                 (  0,   0, 255)),
    ("blueviolet",           (138,  43, 226)),
    ("brown",                (165,  42,  42)),
    ("burlywood",            (222, 184, 135)),
    ("cadetblue",            ( 95, 158, 160)),
    ("chartreuse",           (127, 255,   0)),
    ("chocolate",            (210, 105,  30)),
    ("coral",                (255, 127,  80)),
    ("cornflowerblue",       (100, 149, 237)),
    ("cornsilk",             (255, 248, 220)),
    ("crimson",              (220,  20,  60)),
    ("cyan",                 (  0, 255, 255)),
    ("darkblue",             (  0,   0, 139)),
    ("darkcyan",             (  0, 139, 139)),
    ("darkgoldenrod",        (184, 134,  11)),
    ("darkgray",             (169, 169, 169)),
    ("darkgreen",            (  0, 100,   0)),
    ("darkgrey",             (169, 169, 169)),
    ("darkkhaki",            (189, 183, 107)),
    ("darkmagenta",          (139,   0, 139)),
    ("darkolivegreen",       ( 85, 107,  47)),
    ("darkorange",           (255, 140,   0)),
    ("darkorchid",           (153,  50, 204)),
    ("darkred",              (139,   0,   0)),
    ("darksalmon",           (233, 150, 122)),
    ("darkseagreen",         (143, 188, 143)),
    ("darkslateblue",        ( 72,  61, 139)),
    ("darkslategray",        ( 47,  79,  79)),
    ("darkslategrey",        ( 47,  79,  79)),
    ("darkturquoise",        (  0, 206, 209)),
    ("darkviolet",           (148,   0, 211)),
    ("deeppink",             (255,  20, 147)),
    ("deepskyblue",          (  0, 191, 255)),
    ("dimgray",              (105, 105, 105)),
    ("dimgrey",              (105, 105, 105)),
    ("dodgerblue",           ( 30, 144, 255)),
    ("firebrick",            (178,  34,  34)),
    ("floralwhite",          (255, 250, 240)),
    ("forestgreen",          ( 34, 139,  34)),
    ("fuchsia",              (255,   0, 255)),
    ("gainsboro",            (220, 220, 220)),
    ("ghostwhite",           (248, 248, 255)),
    ("gold",                 (255, 215,   0)),
    ("goldenrod",            (218, 165,  32)),
    ("gray",                 (128, 128, 128)),
    ("green",                (  0, 128,   0)),
    ("greenyellow",          (173, 255,  47)),
    ("grey",                 (128, 128, 128)),
    ("honeydew",             (240, 255, 240)),
    ("hotpink",              (255, 105, 180)),
    ("indianred",            (205,  92,  92)),
    ("indigo",               ( 75,   0, 130)),
    ("ivory",                (255, 255, 240)),
    ("khaki",                (240, 230, 140)),
    ("lavender",             (230, 230, 250)),
    ("lavenderblush",        (255, 240, 245)),
    ("lawngreen",            (124, 252,   0)),
    ("lemonchiffon",         (255, 250, 205)),
    ("lightblue",            (173, 216, 230)),
    ("lightcoral",           (240, 128, 128)),
    ("lightcyan",            (224, 255, 255)),
    ("lightgoldenrodyellow", (250, 250, 210)),
    ("lightgray",            (211, 211, 211)),
    ("lightgreen",           (144, 238, 144)),
    ("lightgrey",            (211, 211, 211)),
    ("lightpink",            (255, 182, 193)),
    ("lightsalmon",          (255, 160, 122)),
    ("lightseagreen",        ( 32, 178, 170)),
    ("lightskyblue",         (135, 206, 250)),
    ("lightslategray",       (119, 136, 153)),
    ("lightslategrey",       (119, 136, 153)),
    ("lightsteelblue",       (176, 196, 222)),
    ("lightyellow",          (255, 255, 224)),
    ("lime",                 (  0, 255,   0)),
    ("limegreen",            ( 50, 205,  50)),
    ("linen",                (250, 240, 230)),
    ("magenta",              (255,   0, 255)),
    ("maroon",               (128,   0,   0)),
    ("mediumaquamarine",     (102, 205, 170)),
    ("mediumblue",           (  0,   0, 205)),
    ("mediumorchid",         (186,  85, 211)),
    ("mediumpurple",         (147, 112, 219)),
    ("mediumseagreen",       ( 60, 179, 113)),
    ("mediumslateblue",      (123, 104, 238)),
    ("mediumspringgreen",    (  0, 250, 154)),
    ("mediumturquoise",      ( 72, 209, 204)),
    ("mediumvioletred",      (199,  21, 133)),
    ("midnightblue",         ( 25,  25, 112)),
    ("mintcream",            (245, 255, 250)),
    ("mistyrose",            (255, 228, 225)),
    ("moccasin",             (255, 228, 181)),
    ("navajowhite",          (255, 222, 173)),
    ("navy",                 (  0,   0, 128)),
    ("oldlace",              (253, 245, 230)),
    ("olive",                (128, 128,   0)),
    ("olivedrab",            (107, 142,  35)),
    ("orange",               (255, 165,   0)),
    ("orangered",            (255,  69,   0)),
    ("orchid",               (218, 112, 214)),
    ("palegoldenrod",        (238, 232, 170)),
    ("palegreen",            (152, 251, 152)),
    ("paleturquoise",        (175, 238, 238)),
    ("palevioletred",        (219, 112, 147)),
    ("papayawhip",           (255, 239, 213)),
    ("peachpuff",            (255, 218, 185)),
    ("peru",                 (205, 133,  63)),
    ("pink",                 (255, 192, 203)),
    ("plum",                 (221, 160, 221)),
    ("powderblue",           (176, 224, 230)),
    ("purple",               (128,   0, 128)),
    ("rebeccapurple",        (102,  51, 153)),
    ("red",                  (255,   0,   0)),
    ("rosybrown",            (188, 143, 143)),
    ("royalblue",            ( 65, 105, 225)),
    ("saddlebrown",          (139,  69,  19)),
    ("salmon",               (250, 128, 114)),
    ("sandybrown",           (244, 164,  96)),
    ("seagreen",             ( 46, 139,  87)),
    ("seashell",             (255, 245, 238)),
    ("sienna",               (160,  82,  45)),
    ("silver",               (192, 192, 192)),
    ("skyblue",              (135, 206, 235)),
    ("slateblue",            (106,  90, 205)),
    ("slategray",            (112, 128, 144)),
    ("slategrey",            (112, 128, 144)),
    ("snow",                 (255, 250, 250)),
    ("springgreen",          (  0, 255, 127)),
    ("steelblue",            ( 70, 130, 180)),
    ("tan",                  (210, 180, 140)),
    ("teal",                 (  0, 128, 128)),
    ("thistle",              (216, 191, 216)),
    ("tomato",               (255,  99,  71)),
    ("turquoise",            ( 64, 224, 208)),
    ("violet",               (238, 130, 238)),
    ("wheat",                (245, 222, 179)),
    ("white",                (255, 255, 255)),
    ("whitesmoke",           (245, 245, 245)),
    ("yellow",               (255, 255,   0)),
    ("yellowgreen",          (154, 205,  50)),
];

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            // #RGB → #RRGGBB: каждый ниббл дублируется.
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: 255 })
        }
        4 => {
            // #RGBA — CSS4: каждый ниббл дублируется.
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()?;
            Some(Color { r: r * 17, g: g * 17, b: b * 17, a: a * 17 })
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color { r, g, b, a: 255 })
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color { r, g, b, a })
        }
        _ => None,
    }
}

/// Парсит `rgb(…)`, `rgba(…)`, `hsl(…)`, `hsla(…)`. Поддерживает запятые
/// и whitespace как разделители, как `rgb`/`rgba` синонимы, так и `hsl`/`hsla`.
/// Компоненты:
///   - rgb: целое 0–255 или процент 0–100% (для каждого канала);
///   - hsl: hue в градусах (число или `<n>deg`), saturation и lightness в %;
///   - alpha (4-й компонент): float 0..1 или процент 0–100%. По умолчанию 1.
fn parse_function_color(s: &str) -> Option<Color> {
    let lower = s.to_ascii_lowercase();
    let (kind, body) = if let Some(b) = lower.strip_prefix("rgba(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("rgb(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Rgb, b)
    } else if let Some(b) = lower.strip_prefix("hsla(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else if let Some(b) = lower.strip_prefix("hsl(").and_then(|t| t.strip_suffix(')')) {
        (ColorFn::Hsl, b)
    } else {
        return None;
    };
    let parts = split_color_args(body);
    if !(parts.len() == 3 || parts.len() == 4) {
        return None;
    }
    let alpha = if parts.len() == 4 {
        parse_alpha_component(&parts[3])?
    } else {
        255
    };
    match kind {
        ColorFn::Rgb => {
            let r = parse_rgb_component(&parts[0])?;
            let g = parse_rgb_component(&parts[1])?;
            let b = parse_rgb_component(&parts[2])?;
            Some(Color { r, g, b, a: alpha })
        }
        ColorFn::Hsl => {
            let h = parse_hue_component(&parts[0])?;
            let s = parse_percent_component(&parts[1])?;
            let l = parse_percent_component(&parts[2])?;
            let (r, g, b) = hsl_to_rgb(h, s, l);
            Some(Color { r, g, b, a: alpha })
        }
    }
}

enum ColorFn {
    Rgb,
    Hsl,
    // CSS4 расширения (lab / lch / oklab / oklch / color()) — не реализуем.
}

/// Разбивает тело функции по запятой или whitespace (CSS4 разрешает оба),
/// плюс по `/` для отделения alpha в новом синтаксисе `rgb(255 0 0 / 0.5)`.
fn split_color_args(body: &str) -> Vec<String> {
    // Если есть запятые — режем по ним (legacy CSS3).
    if body.contains(',') {
        return body.split(',').map(|s| s.trim().to_string()).collect();
    }
    // Modern CSS4: `r g b` или `r g b / a`. Слэш отделяет alpha.
    let normalized = body.replace('/', " / ");
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    // Ищем `/` — разделитель alpha.
    if let Some(slash) = tokens.iter().position(|&t| t == "/") {
        let mut head: Vec<String> = tokens[..slash].iter().map(|t| t.to_string()).collect();
        if let Some(alpha) = tokens.get(slash + 1) {
            head.push((*alpha).to_string());
        }
        head
    } else {
        tokens.iter().map(|t| t.to_string()).collect()
    }
}

fn parse_rgb_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n))
}

fn parse_alpha_component(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let p = pct.trim().parse::<f32>().ok()?;
        return Some(clamp_byte((p / 100.0) * 255.0));
    }
    let n = s.parse::<f32>().ok()?;
    Some(clamp_byte(n * 255.0))
}

/// Парсит hue в градусах. Поддерживает четыре единицы CSS Color L4 §9:
///   - `deg` или без единицы — градусы (default);
///   - `turn` — оборот (1turn = 360deg, как `<a href>` в Кубе Рубика);
///   - `rad` — радианы (1rad = 180/π deg ≈ 57.296deg);
///   - `grad` — гоны (1grad = 0.9deg, full turn = 400grad).
///
/// Порядок проверки суффиксов важен: более длинные сначала, иначе
/// `grad` будет ошибочно ловиться как `rad`.
fn parse_hue_component(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("turn") {
        return num.trim().parse::<f32>().ok().map(|v| v * 360.0);
    }
    if let Some(num) = s.strip_suffix("grad") {
        return num.trim().parse::<f32>().ok().map(|v| v * 0.9);
    }
    if let Some(num) = s.strip_suffix("rad") {
        return num.trim().parse::<f32>().ok().map(|v| v * (180.0 / std::f32::consts::PI));
    }
    let s = s.strip_suffix("deg").unwrap_or(s);
    s.trim().parse::<f32>().ok()
}

fn parse_percent_component(s: &str) -> Option<f32> {
    let s = s.trim();
    let pct = s.strip_suffix('%')?;
    let p = pct.trim().parse::<f32>().ok()?;
    Some((p / 100.0).clamp(0.0, 1.0))
}

fn clamp_byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// Преобразование HSL → RGB по CSS Color Module Level 3 (как у whatwg).
/// `h` — в градусах (любое значение, нормализуется по mod 360),
/// `s` и `l` — нормированные 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 360.0;
    if s == 0.0 {
        let v = clamp_byte(l * 255.0);
        return (v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    (clamp_byte(r * 255.0), clamp_byte(g * 255.0), clamp_byte(b * 255.0))
}

fn hue_to_rgb(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    #[test]
    fn rgb_legacy_commas() {
        assert_eq!(parse_color("rgb(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("rgb(0, 128, 0)"), Some(rgba(0, 128, 0, 255)));
    }

    #[test]
    fn rgb_modern_whitespace() {
        assert_eq!(parse_color("rgb(255 0 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_percent_components() {
        // 100% = 255, 50% = 128 (округление).
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some(rgba(255, 0, 0, 255)));
        let half = parse_color("rgb(50%, 50%, 50%)").unwrap();
        assert!((half.r as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgba_with_alpha_float() {
        // alpha 0.5 → 128 (округление 127.5).
        let c = parse_color("rgba(255, 0, 0, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1, "a={}", c.a);
    }

    #[test]
    fn rgba_with_alpha_percent() {
        let c = parse_color("rgba(255, 0, 0, 50%)").unwrap();
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_modern_slash_alpha() {
        // Modern syntax: rgb(r g b / a) — без `rgba` префикса.
        let c = parse_color("rgb(255 0 0 / 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn rgb_out_of_range_clamps() {
        // 300 должно зажаться до 255, -10 до 0.
        assert_eq!(parse_color("rgb(300, -10, 0)"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn rgb_invalid_components() {
        assert_eq!(parse_color("rgb(abc, def, ghi)"), None);
        assert_eq!(parse_color("rgb(255, 0)"), None);
        assert_eq!(parse_color("rgb()"), None);
    }

    #[test]
    fn hsl_primary_colors() {
        assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(
            parse_color("hsl(120, 100%, 50%)"),
            Some(rgba(0, 255, 0, 255))
        );
        assert_eq!(
            parse_color("hsl(240, 100%, 50%)"),
            Some(rgba(0, 0, 255, 255))
        );
    }

    #[test]
    fn hsl_with_deg_unit() {
        assert_eq!(
            parse_color("hsl(0deg, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_turn() {
        // 0.5turn = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(0.5turn, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 1turn = 360deg = 0deg → red.
        assert_eq!(
            parse_color("hsl(1turn, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_in_rad() {
        // π rad = 180deg → cyan. f32 округление допустимо.
        let c = parse_color("hsl(3.14159265rad, 100%, 50%)").unwrap();
        assert_eq!(c.r, 0);
        assert!(c.g >= 254);
        assert!(c.b >= 254);
    }

    #[test]
    fn hsl_hue_in_grad() {
        // 200grad = 180deg → cyan.
        assert_eq!(
            parse_color("hsl(200grad, 100%, 50%)"),
            Some(rgba(0, 255, 255, 255))
        );
        // 400grad = 360deg = 0 → red.
        assert_eq!(
            parse_color("hsl(400grad, 100%, 50%)"),
            Some(rgba(255, 0, 0, 255))
        );
    }

    #[test]
    fn hsl_hue_units_dont_collide() {
        // `grad` не должен ловиться как `rad`. 100grad = 90deg → жёлто-зелёный.
        // А 100rad = 5729.58deg, mod 360 ≈ 329.58 — пурпурно-розовый. Цвета
        // должны отличаться, иначе суффикс ловится не тот.
        let g = parse_color("hsl(100grad, 100%, 50%)").unwrap();
        let r = parse_color("hsl(100rad, 100%, 50%)").unwrap();
        assert_ne!(g, r, "grad и rad дают разные цвета");
    }

    #[test]
    fn hsl_grayscale_when_saturation_zero() {
        // s=0 → lightness как оттенок серого.
        let c = parse_color("hsl(0, 0%, 50%)").unwrap();
        assert!((c.r as i32 - 128).abs() <= 1);
        assert_eq!(c.r, c.g);
        assert_eq!(c.g, c.b);
    }

    #[test]
    fn hsla_with_alpha() {
        let c = parse_color("hsla(0, 100%, 50%, 0.5)").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn hsl_hue_wraps() {
        // 360° = 0°, должен дать тот же красный.
        assert_eq!(
            parse_color("hsl(360, 100%, 50%)"),
            parse_color("hsl(0, 100%, 50%)")
        );
    }

    #[test]
    fn hex_with_alpha_8_digits() {
        // #ff000080 → red, alpha 128.
        let c = parse_color("#ff000080").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 128);
    }

    #[test]
    fn hex_short_with_alpha() {
        // #f008 → ff 00 00 88.
        let c = parse_color("#f008").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 0x88);
    }

    #[test]
    fn named_and_hex_still_work() {
        assert_eq!(parse_color("red"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#ff0000"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("#f00"), Some(rgba(255, 0, 0, 255)));
    }

    #[test]
    fn case_insensitive_function_names() {
        assert_eq!(parse_color("RGB(255, 0, 0)"), Some(rgba(255, 0, 0, 255)));
        assert_eq!(parse_color("Rgba(0, 0, 0, 1)"), Some(rgba(0, 0, 0, 255)));
    }

    // ── Полный набор CSS3 named colors ────────────────────────────────────

    #[test]
    fn named_colors_table_is_sorted() {
        // Бинарный поиск требует сортировки. Защита от опечатки при добавлении
        // нового цвета не на своё место.
        for w in NAMED_COLORS.windows(2) {
            assert!(w[0].0 < w[1].0, "table not sorted at {} >= {}", w[0].0, w[1].0);
        }
    }

    #[test]
    fn named_color_count() {
        // Sanity-check: CSS3 = 147 named colors + `rebeccapurple` (CSS4 §6.1)
        // = 148. `transparent` обрабатывается отдельно, в таблице его нет.
        // Если число изменилось — обновить и тест, и CLAUDE.md.
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn named_color_typical_websafe() {
        assert_eq!(parse_color("cornflowerblue"), Some(rgba(100, 149, 237, 255)));
        assert_eq!(parse_color("dodgerblue"), Some(rgba(30, 144, 255, 255)));
        assert_eq!(parse_color("hotpink"), Some(rgba(255, 105, 180, 255)));
        assert_eq!(parse_color("indigo"), Some(rgba(75, 0, 130, 255)));
        assert_eq!(parse_color("teal"), Some(rgba(0, 128, 128, 255)));
    }

    #[test]
    fn named_color_grey_variants_match_gray() {
        // CSS принимает оба написания; цвета должны совпадать.
        assert_eq!(parse_color("gray"), parse_color("grey"));
        assert_eq!(parse_color("darkgray"), parse_color("darkgrey"));
        assert_eq!(parse_color("lightgray"), parse_color("lightgrey"));
        assert_eq!(parse_color("slategray"), parse_color("slategrey"));
        assert_eq!(parse_color("dimgray"), parse_color("dimgrey"));
    }

    #[test]
    fn named_color_rebeccapurple_css4() {
        // Добавлен в CSS Color L4 §6.1 в честь Ребекки Майер.
        assert_eq!(parse_color("rebeccapurple"), Some(rgba(102, 51, 153, 255)));
    }

    #[test]
    fn named_color_case_insensitive() {
        assert_eq!(parse_color("CornflowerBlue"), parse_color("cornflowerblue"));
        assert_eq!(parse_color("RED"), parse_color("red"));
    }

    #[test]
    fn named_color_transparent() {
        // Особый случай — alpha = 0.
        let c = parse_color("transparent").unwrap();
        assert_eq!(c, Color::TRANSPARENT);
        assert_eq!(c.a, 0);
    }

    #[test]
    fn named_color_unknown_returns_none() {
        assert_eq!(parse_color("notacolor"), None);
        assert_eq!(parse_color("currentcolor"), None); // не реализовано как named
    }

    #[test]
    fn named_color_aqua_and_cyan_same() {
        // CSS3: оба имени дают (0, 255, 255).
        assert_eq!(parse_color("aqua"), parse_color("cyan"));
    }

    #[test]
    fn named_color_fuchsia_and_magenta_same() {
        // CSS3: оба имени дают (255, 0, 255).
        assert_eq!(parse_color("fuchsia"), parse_color("magenta"));
    }

    // ── Relative units: parse_length + resolve ────────────────────────────

    #[test]
    fn parse_length_recognizes_units() {
        assert_eq!(parse_length("10px"), Some(Length::Px(10.0)));
        assert_eq!(parse_length("1.5em"), Some(Length::Em(1.5)));
        assert_eq!(parse_length("2rem"), Some(Length::Rem(2.0)));
        assert_eq!(parse_length("50%"), Some(Length::Percent(50.0)));
        assert_eq!(parse_length("0"), Some(Length::Px(0.0)));
        // Пробелы вокруг числа допустимы.
        assert_eq!(parse_length(" 10 px "), Some(Length::Px(10.0)));
        // Мусор → None.
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length("px"), None);
    }

    /// Тестовый viewport: квадратный, чтобы vh == vw, vmin == vmax.
    fn vp() -> Size { Size::new(1000.0, 1000.0) }

    #[test]
    fn length_resolve_px_is_identity() {
        assert_eq!(Length::Px(12.0).resolve(16.0, Some(100.0), vp()), Some(12.0));
    }

    #[test]
    fn length_resolve_em_uses_basis() {
        // 1.5em при basis 20 = 30.
        assert_eq!(Length::Em(1.5).resolve(20.0, None, vp()), Some(30.0));
    }

    #[test]
    fn length_resolve_rem_ignores_basis() {
        // rem всегда от ROOT_FONT_SIZE = 16.
        assert_eq!(Length::Rem(2.0).resolve(999.0, None, vp()), Some(32.0));
    }

    #[test]
    fn length_resolve_percent_needs_basis() {
        assert_eq!(Length::Percent(50.0).resolve(16.0, Some(200.0), vp()), Some(100.0));
        assert_eq!(Length::Percent(50.0).resolve(16.0, None, vp()), None);
    }

    // ── viewport units ────────────────────────────────────────────────────

    #[test]
    fn parse_length_recognizes_viewport_units() {
        assert_eq!(parse_length("50vh"), Some(Length::Vh(50.0)));
        assert_eq!(parse_length("50vw"), Some(Length::Vw(50.0)));
        assert_eq!(parse_length("10vmin"), Some(Length::Vmin(10.0)));
        assert_eq!(parse_length("10vmax"), Some(Length::Vmax(10.0)));
        // Дробные значения тоже.
        assert_eq!(parse_length("1.5vh"), Some(Length::Vh(1.5)));
    }

    #[test]
    fn length_resolve_vh_uses_viewport_height() {
        // 50vh от viewport (1024 x 768) = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vh(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vw_uses_viewport_width() {
        // 25vw от viewport (1024 x 768) = 256.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vw(25.0).resolve(16.0, None, v), Some(256.0));
    }

    #[test]
    fn length_resolve_vmin_uses_smaller_dimension() {
        // 50vmin от viewport (1024 x 768) — min = 768; 50% = 384.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmin(50.0).resolve(16.0, None, v), Some(384.0));
    }

    #[test]
    fn length_resolve_vmax_uses_larger_dimension() {
        // 50vmax от viewport (1024 x 768) — max = 1024; 50% = 512.
        let v = Size::new(1024.0, 768.0);
        assert_eq!(Length::Vmax(50.0).resolve(16.0, None, v), Some(512.0));
    }

    // ── text-decoration parsing ────────────────────────────────────────────

    #[test]
    fn text_decoration_underline_sets_only_underline() {
        let d = parse_text_decoration("underline").unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_none_returns_empty() {
        let d = parse_text_decoration("none").unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn text_decoration_multiple_keywords_combine() {
        let d = parse_text_decoration("overline underline").unwrap();
        assert!(d.underline);
        assert!(d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_line_through_with_hyphen() {
        let d = parse_text_decoration("line-through").unwrap();
        assert!(d.line_through);
    }

    #[test]
    fn text_decoration_none_with_other_clears_all() {
        // `none` всегда побеждает: интуитивный сброс.
        let d = parse_text_decoration("underline none").unwrap();
        assert!(d.is_empty());
    }

    #[test]
    fn text_decoration_ignores_unknown_tokens() {
        // `blink` (CSS2 deprecated), цвета и `solid`/`wavy` — игнорируем.
        let d = parse_text_decoration("underline blink red solid").unwrap();
        assert!(d.underline);
        assert!(!d.overline);
        assert!(!d.line_through);
    }

    #[test]
    fn text_decoration_unrecognized_only_returns_none() {
        assert!(parse_text_decoration("blink").is_none());
        assert!(parse_text_decoration("").is_none());
    }

    #[test]
    fn text_decoration_is_case_insensitive() {
        let d = parse_text_decoration("UNDERLINE Line-Through").unwrap();
        assert!(d.underline);
        assert!(d.line_through);
    }

    // ── Border parsing ────────────────────────────────────────────────────────

    fn style_for(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse("<p>x</p>");
        let sheet = lumen_css_parser::parse(&format!("p {{ {css} }}"));
        let root_style = ComputedStyle::root();
        let p = doc.get(doc.root()).children[0];
        compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0))
    }

    #[test]
    fn border_shorthand_sets_all_sides() {
        let s = style_for("border: 2px solid red");
        assert!((s.border_top_width - 2.0).abs() < 0.01);
        assert!((s.border_right_width - 2.0).abs() < 0.01);
        assert!((s.border_bottom_width - 2.0).abs() < 0.01);
        assert!((s.border_left_width - 2.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Solid);
        assert_eq!(s.border_right_style, BorderStyle::Solid);
        assert_eq!(s.border_bottom_style, BorderStyle::Solid);
        assert_eq!(s.border_left_style, BorderStyle::Solid);
        let red = Color { r: 255, g: 0, b: 0, a: 255 };
        assert_eq!(s.border_top_color, Some(red));
        assert_eq!(s.border_right_color, Some(red));
    }

    #[test]
    fn border_width_shorthand_1_value() {
        let s = style_for("border-width: 5px");
        assert!((s.border_top_width - 5.0).abs() < 0.01);
        assert!((s.border_right_width - 5.0).abs() < 0.01);
        assert!((s.border_bottom_width - 5.0).abs() < 0.01);
        assert!((s.border_left_width - 5.0).abs() < 0.01);
    }

    #[test]
    fn border_style_sets_all_sides() {
        let s = style_for("border-style: dashed");
        assert_eq!(s.border_top_style, BorderStyle::Dashed);
        assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
    }

    #[test]
    fn border_color_shorthand() {
        let blue = Color { r: 0, g: 0, b: 255, a: 255 };
        let s = style_for("border-color: blue");
        assert_eq!(s.border_top_color, Some(blue));
        assert_eq!(s.border_left_color, Some(blue));
    }

    #[test]
    fn border_top_side_shorthand() {
        let s = style_for("border-top: 3px dotted green");
        assert!((s.border_top_width - 3.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Dotted);
        let green = Color { r: 0, g: 128, b: 0, a: 255 };
        assert_eq!(s.border_top_color, Some(green));
        // Остальные стороны — не изменены.
        assert!((s.border_right_width - 0.0).abs() < 0.01);
        assert_eq!(s.border_right_style, BorderStyle::None);
    }

    #[test]
    fn border_per_side_width_properties() {
        let s = style_for("border-left-width: 4px; border-right-width: 6px");
        assert!((s.border_left_width - 4.0).abs() < 0.01);
        assert!((s.border_right_width - 6.0).abs() < 0.01);
        assert!((s.border_top_width - 0.0).abs() < 0.01);
    }

    #[test]
    fn border_no_color_means_none() {
        let s = style_for("border: 2px solid");
        assert!(s.border_top_color.is_none());
    }

    #[test]
    fn border_style_kw_none_is_invisible() {
        assert!(!BorderStyle::None.is_visible());
        assert!(BorderStyle::Solid.is_visible());
        assert!(BorderStyle::Dashed.is_visible());
        assert!(BorderStyle::Dotted.is_visible());
    }

    // ── box-sizing parsing ─────────────────────────────────────────────────

    #[test]
    fn box_sizing_default_is_content_box() {
        let s = style_for("color: red");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_border_box_parses() {
        let s = style_for("box-sizing: border-box");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_content_box_parses_back_to_default() {
        // Явное content-box после border-box возвращает к default.
        let s = style_for("box-sizing: border-box; box-sizing: content-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_case_insensitive() {
        let s = style_for("box-sizing: BORDER-BOX");
        assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn box_sizing_unknown_value_keeps_default() {
        // CSS-парсер не должен падать на мусоре — оставляет предыдущее значение.
        let s = style_for("box-sizing: padding-box");
        assert_eq!(s.box_sizing, BoxSizing::ContentBox);
    }

    #[test]
    fn box_sizing_not_inherited() {
        // box-sizing — non-inherited (CSS Basic UI 3 §4.1).
        // Дочерний <p> не получает border-box от родительского <div>.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { box-sizing: border-box; }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.root()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0));
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0));
        assert_eq!(div_style.box_sizing, BoxSizing::BorderBox);
        assert_eq!(p_style.box_sizing, BoxSizing::ContentBox);
    }
}
