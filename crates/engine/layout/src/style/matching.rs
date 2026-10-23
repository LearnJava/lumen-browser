//! Матчинг селекторов: обход комбинаторов справа налево с back-tracking
//! (`matches_complex`/`matches_chain`), compound/simple/attribute-матчинг
//! (`matches_compound`/`matches_simple`/`matches_attribute`) и CSS Scoping L1
//! `::slotted()`/`:host` (`matches_slotted_complex`/`complex_has_host`).
//! Динамические псевдо-классы (`:hover`/`:nth-child`/form-состояние/позиция
//! в дереве) — в потомке [`forms`].
//!
//! Перенесено батчем SPLIT-ST12 из `crates/engine/layout/src/style.rs`
//! (анкер `fn matches_complex`) без правок тел: изменена только видимость
//! `complex_has_host` (нужна донору, `style.rs`) и путь импортов.

use lumen_css_parser::{
    AttrOp, AttrSelector, Combinator, ComplexSelector, CompoundSelector, PseudoClass,
    PseudoElementKind, SimpleSelector, Specificity,
};
use std::cell::Cell;

use lumen_dom::{Attribute, Document, NodeData, NodeId};

use crate::style::SHADOW_HOST_SCOPE;
use forms::{is_element, matches_pseudo_class, previous_element_sibling};

pub(in crate::style) mod forms;

// ──────────────── selector matching ────────────────

pub(crate) fn matches_complex(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    // Справа налево с back-tracking. Алгоритм:
    //   1. Матчим последний compound на текущем `node`; если ОК и осталось > 0
    //      compound-ов левее, для combinator-а перед ним перебираем ВСЕ возможные
    //      кандидаты (предки для descendant / earlier-siblings для later-sibling)
    //      и рекурсивно матчим префикс в каждом. child / next-sibling имеют
    //      ровно одного кандидата.
    // BUG-935 срез 66: префикс — это `n` первых compound-ов самого селектора, без
    // промежуточных `Vec` (два выделения на каждый вызов; на странице с 1500
    // правилами их ~260 на элемент, 98% — отказы по первому compound-у).
    matches_chain(complex, 1 + complex.tail.len(), doc, node, &|_| true)
}

/// `matches_complex` с якорем цепочки (BUG-1444, `:has()` §3.5.1): `leftmost_ok`
/// проверяет узел, на котором сел самый левый compound, — он обязан стоять в
/// нужном отношении к субъекту `:has()` (как `:scope > .a > .b`). Мономорфизация
/// по замыканию: горячий `matches_complex` (`|_| true`) не платит за якорь.
pub(super) fn matches_complex_anchored(
    complex: &ComplexSelector,
    doc: &Document,
    node: NodeId,
    leftmost_ok: &impl Fn(NodeId) -> bool,
) -> bool {
    matches_chain(complex, 1 + complex.tail.len(), doc, node, leftmost_ok)
}

/// Compound номер `i` селектора: 0 — голова, дальше — хвост.
fn compound_at(complex: &ComplexSelector, i: usize) -> &CompoundSelector {
    if i == 0 { &complex.head } else { &complex.tail[i - 1].1 }
}

/// Теневой хост, видимый из `ShadowRoot` `sr` при матчинге листа его собственного
/// теневого дерева (`SHADOW_HOST_SCOPE` == индекс хоста). `ShadowRoot` не имеет
/// `parent`, поэтому комбинатор `:host <потомок>` / `:host > <потомок>` (Scoping 1
/// §3.2, BUG-1453) переходит к хосту отсюда; вне такого скоупа — `None`.
fn scoped_shadow_host(doc: &Document, sr: NodeId) -> Option<NodeId> {
    if !matches!(doc.get(sr).data, NodeData::ShadowRoot { .. }) {
        return None;
    }
    let scope = SHADOW_HOST_SCOPE.with(Cell::get);
    if scope == u32::MAX {
        return None;
    }
    doc.shadow_host_of(sr).filter(|h| h.index() as u32 == scope)
}

/// Рекурсивный matcher с back-tracking. Compound `n - 1` матчится на `node`;
/// для левее идущих compound-ов перебираем кандидатов согласно combinator-у.
fn matches_chain(
    complex: &ComplexSelector,
    n: usize,
    doc: &Document,
    node: NodeId,
    leftmost_ok: &impl Fn(NodeId) -> bool,
) -> bool {
    if !matches_compound(compound_at(complex, n - 1), doc, node) {
        return false;
    }
    if n == 1 {
        return leftmost_ok(node);
    }

    // Combinator между compound-ами `n - 2` и `n - 1`.
    let comb = complex.tail[n - 2].0;

    match comb {
        Combinator::Descendant => {
            // Перебираем всех предков как кандидатов.
            let mut cur = doc.get(node).parent;
            let mut top = None;
            while let Some(p) = cur {
                if is_element(doc, p) && matches_chain(complex, n - 1, doc, p, leftmost_ok) {
                    return true;
                }
                top = Some(p);
                cur = doc.get(p).parent;
            }
            // Цепочка упёрлась в корень теневого дерева: хост — последний кандидат.
            match top.and_then(|t| scoped_shadow_host(doc, t)) {
                Some(host) => matches_chain(complex, n - 1, doc, host, leftmost_ok),
                None => false,
            }
        }
        Combinator::Child => {
            // Один кандидат: parent.
            let Some(parent) = doc.get(node).parent else { return false; };
            if !is_element(doc, parent) {
                return match scoped_shadow_host(doc, parent) {
                    Some(host) => matches_chain(complex, n - 1, doc, host, leftmost_ok),
                    None => false,
                };
            }
            matches_chain(complex, n - 1, doc, parent, leftmost_ok)
        }
        Combinator::NextSibling => {
            // Один кандидат: предыдущий element-sibling.
            let Some(prev) = previous_element_sibling(doc, node) else { return false; };
            matches_chain(complex, n - 1, doc, prev, leftmost_ok)
        }
        Combinator::LaterSibling => {
            // Перебираем все earlier-siblings как кандидатов.
            let mut sib = previous_element_sibling(doc, node);
            while let Some(s) = sib {
                if matches_chain(complex, n - 1, doc, s, leftmost_ok) {
                    return true;
                }
                sib = previous_element_sibling(doc, s);
            }
            false
        }
    }
}

/// BUG-935 срез 66: прежняя реализация `matches_complex` (префикс — срезы двух `Vec`,
/// собранных на каждый вызов) — эталон для дифференциального теста и стенда
/// `bug935_cascade_bench`; в сборку не входит.
#[cfg(test)]
pub(crate) fn matches_complex_reference(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    fn chain(compounds: &[&CompoundSelector], combinators: &[Combinator], doc: &Document, node: NodeId) -> bool {
        let n = compounds.len();
        if !matches_compound(compounds[n - 1], doc, node) {
            return false;
        }
        if n == 1 {
            return true;
        }
        let (prev_compounds, prev_combinators) = (&compounds[..n - 1], &combinators[..n - 2]);
        match combinators[n - 2] {
            Combinator::Descendant => {
                let mut cur = doc.get(node).parent;
                while let Some(p) = cur {
                    if is_element(doc, p) && chain(prev_compounds, prev_combinators, doc, p) {
                        return true;
                    }
                    cur = doc.get(p).parent;
                }
                false
            }
            Combinator::Child => {
                let Some(parent) = doc.get(node).parent else { return false };
                is_element(doc, parent) && chain(prev_compounds, prev_combinators, doc, parent)
            }
            Combinator::NextSibling => {
                let Some(prev) = previous_element_sibling(doc, node) else { return false };
                chain(prev_compounds, prev_combinators, doc, prev)
            }
            Combinator::LaterSibling => {
                let mut sib = previous_element_sibling(doc, node);
                while let Some(s) = sib {
                    if chain(prev_compounds, prev_combinators, doc, s) {
                        return true;
                    }
                    sib = previous_element_sibling(doc, s);
                }
                false
            }
        }
    }
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    chain(&compounds, &combinators, doc, node)
}

/// CSS Scoping L1 §6.2: true if `node` is a direct light-tree child of a shadow host,
/// meaning it is eligible to be slotted via a `<slot>` in the shadow tree.
fn is_slotted_element(doc: &Document, node: NodeId) -> bool {
    doc.get(node).parent
        .map(|p| doc.is_shadow_host(p))
        .unwrap_or(false)
}

/// CSS Scoping L1 §6.1 — true if the subject (last) compound of `complex`
/// contains a `:host` / `:host(sel)` pseudo-class. Used to select, from a shadow
/// tree's stylesheet, the rules that target the host element (as opposed to rules
/// scoped to shadow descendants).
pub(in crate::style) fn complex_has_host(complex: &ComplexSelector) -> bool {
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    last.parts.iter().any(|p| matches!(p, SimpleSelector::PseudoClass(PseudoClass::Host(_))))
}

/// CSS Scoping L1 §6.2 — attempts to match a complex selector containing
/// `::slotted(inner_sel)` against `node`.
///
/// Returns `Some(specificity)` when all conditions hold:
/// 1. The last compound of `complex` contains `::slotted(inner_sel)`.
/// 2. `node` is a slotted element (DOM parent is a shadow host).
/// 3. `node` matches every selector in `inner_sel`.
/// 4. The outer context (compound minus `::slotted`) matches the shadow host (node's parent).
///    If the outer context is empty, no ancestor check is needed.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn matches_slotted_complex(
    complex: &ComplexSelector,
    doc: &Document,
    node: NodeId,
) -> Option<Specificity> {
    // Locate the last compound, which must contain ::slotted.
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    let slotted_inner: Option<&Vec<ComplexSelector>> = last.parts.iter().find_map(|p| {
        if let SimpleSelector::PseudoElement(PseudoElementKind::Slotted(inner)) = p {
            Some(inner.as_ref()?)
        } else {
            None
        }
    });
    // Rule must contain ::slotted.
    let inner_selectors = slotted_inner?;

    // Node must be a slotted element.
    if !is_slotted_element(doc, node) {
        return None;
    }

    // Node must match the inner selector list.
    if !inner_selectors.iter().any(|s| matches_complex(s, doc, node)) {
        return None;
    }

    // Build the outer complex selector (strip ::slotted from the last compound).
    let stripped_last = CompoundSelector {
        parts: last.parts.iter()
            .filter(|p| !matches!(p, SimpleSelector::PseudoElement(PseudoElementKind::Slotted(_))))
            .cloned()
            .collect(),
    };

    // If there is no outer context at all, the rule matches.
    if complex.tail.is_empty() && stripped_last.parts.is_empty() {
        return Some(complex.specificity());
    }

    // Outer context: match against the shadow host (node's DOM parent).
    let host = doc.get(node).parent.expect("is_slotted_element ensures parent");
    let outer = if complex.tail.is_empty() {
        ComplexSelector { head: stripped_last, tail: vec![] }
    } else {
        let mut tail = complex.tail.clone();
        tail.last_mut().expect("non-empty tail").1 = stripped_last;
        ComplexSelector { head: complex.head.clone(), tail }
    };

    if matches_complex(&outer, doc, host) {
        Some(complex.specificity())
    } else {
        None
    }
}

/// CSS Shadow Parts L1 §2 — матчит сложный селектор с `::part(<ident>+)` в
/// последнем compound-е против элемента `part_el` теневого дерева.
///
/// `host` — элемент, на который сел compound до `::part()` (теневой хост, чьи
/// части видит эта область видимости), `names` — имена, под которыми `part_el`
/// виден из этой области (токены `part`, пропущенные через `exportparts`
/// промежуточных хостов). Условия:
/// 1. в последнем compound-е есть `::part(sel_names)` и все `sel_names` ∈ `names`;
/// 2. простые селекторы до `::part()` (с комбинаторами левее) матчат `host`;
/// 3. простые селекторы после `::part()` — только нематериальные для структуры
///    псевдоклассы (`:hover`, `:focus`, …) — матчат `part_el`; структурные
///    псевдоклассы (`:first-child`, `:nth-child()`, …) части не матчат (§2),
///    псевдоэлементы после `::part()` допустимы только в режиме `pseudo` (стиль
///    псевдоэлемента части: `::part(a)::before`) — ровно один, принятый предикатом.
pub(crate) fn matches_part_complex(
    complex: &ComplexSelector,
    doc: &Document,
    part_el: NodeId,
    host: NodeId,
    names: &[&str],
    pseudo: Option<&dyn Fn(&PseudoElementKind) -> bool>,
) -> Option<Specificity> {
    let last = complex.tail.last().map(|(_, c)| c).unwrap_or(&complex.head);
    let at = last.parts.iter().position(|p| {
        matches!(p, SimpleSelector::PseudoElement(PseudoElementKind::Part(_)))
    })?;
    let SimpleSelector::PseudoElement(PseudoElementKind::Part(wanted)) = &last.parts[at] else {
        return None;
    };
    if !wanted.iter().all(|w| names.contains(&w.as_str())) {
        return None;
    }
    let mut pseudo_seen = false;
    for p in &last.parts[at + 1..] {
        match p {
            SimpleSelector::PseudoClass(pc) if !pseudo_class_is_structural(pc) => {
                if !matches_pseudo_class(pc, doc, part_el) {
                    return None;
                }
            }
            SimpleSelector::PseudoElement(k) if !pseudo_seen && pseudo.is_some_and(|ok| ok(k)) => {
                pseudo_seen = true;
            }
            _ => return None,
        }
    }
    if pseudo.is_some() && !pseudo_seen {
        return None;
    }
    let head_last = CompoundSelector { parts: last.parts[..at].to_vec() };
    let outer = if complex.tail.is_empty() {
        ComplexSelector { head: head_last, tail: vec![] }
    } else {
        let mut tail = complex.tail.clone();
        if let Some(l) = tail.last_mut() {
            l.1 = head_last;
        }
        ComplexSelector { head: complex.head.clone(), tail }
    };
    // Пустой compound перед `::part()` (`::part(a)` без хоста) — как `*`.
    let outer_empty = outer.tail.is_empty() && outer.head.parts.is_empty();
    if outer_empty || matches_complex(&outer, doc, host) {
        Some(complex.specificity())
    } else {
        None
    }
}

/// Структурные псевдоклассы не матчат `::part()` (Shadow Parts L1 §2): у части нет
/// осмысленных соседей в дереве, видимом снаружи.
fn pseudo_class_is_structural(pc: &PseudoClass) -> bool {
    matches!(
        pc,
        PseudoClass::FirstChild
            | PseudoClass::LastChild
            | PseudoClass::OnlyChild
            | PseudoClass::FirstOfType
            | PseudoClass::LastOfType
            | PseudoClass::OnlyOfType
            | PseudoClass::NthChild(..)
            | PseudoClass::NthLastChild(..)
            | PseudoClass::NthOfType(..)
            | PseudoClass::NthLastOfType(..)
            | PseudoClass::Empty
            | PseudoClass::Root
            | PseudoClass::Has(_)
    )
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

pub(in crate::style) fn matches_simple(
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
    attrs.iter().find(|a| a.name.local == sel.name).is_some_and(|attr| attr_value_matches(sel, &attr.value))
}

/// Совпадает ли `value` — значение присутствующего атрибута — с `sel` (оператор и образец).
pub(in crate::style) fn attr_value_matches(sel: &AttrSelector, value: &str) -> bool {
    let ci = sel.case_insensitive;
    match (sel.op, sel.value.as_deref()) {
        (None, _) => true,
        (Some(AttrOp::Equals), Some(v)) => str_eq(value, v, ci),
        (Some(AttrOp::Includes), Some(v)) => {
            !v.is_empty() && value.split_whitespace().any(|w| str_eq(w, v, ci))
        }
        (Some(AttrOp::DashMatch), Some(v)) => {
            // Точное совпадение или префикс с разделителем `-`. `i` применяется
            // к обеим частям сравнения (CSS L4 §6.3.6).
            str_eq(value, v, ci) || str_starts_with(value, &format!("{v}-"), ci)
        }
        (Some(AttrOp::Prefix), Some(v)) => !v.is_empty() && str_starts_with(value, v, ci),
        (Some(AttrOp::Suffix), Some(v)) => !v.is_empty() && str_ends_with(value, v, ci),
        (Some(AttrOp::Substring), Some(v)) => !v.is_empty() && str_contains(value, v, ci),
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
