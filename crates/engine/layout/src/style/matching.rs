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
use lumen_dom::{Attribute, Document, NodeData, NodeId};

use forms::{is_element, matches_pseudo_class, previous_element_sibling};

pub(in crate::style) mod forms;

// ──────────────── selector matching ────────────────

pub(crate) fn matches_complex(complex: &ComplexSelector, doc: &Document, node: NodeId) -> bool {
    // Справа налево с back-tracking. Алгоритм:
    //   1. Складываем (compounds, combinators) в массивы.
    //   2. Рекурсивно: матчим последний compound на текущем `node`; если ОК
    //      и осталось > 0 compound-ов левее, для combinator-а перед ним
    //      перебираем ВСЕ возможные кандидаты (предки для descendant /
    //      earlier-siblings для later-sibling) и рекурсивно матчим суффикс
    //      в каждом. child / next-sibling имеют ровно одного кандидата.
    let mut compounds: Vec<&CompoundSelector> = Vec::with_capacity(1 + complex.tail.len());
    let mut combinators: Vec<Combinator> = Vec::with_capacity(complex.tail.len());
    compounds.push(&complex.head);
    for (comb, comp) in &complex.tail {
        combinators.push(*comb);
        compounds.push(comp);
    }
    matches_chain(&compounds, &combinators, doc, node)
}

/// Рекурсивный matcher с back-tracking. `compounds[last]` матчится на `node`;
/// для левее идущих compound-ов перебираем кандидатов согласно combinator-у.
fn matches_chain(
    compounds: &[&CompoundSelector],
    combinators: &[Combinator],
    doc: &Document,
    node: NodeId,
) -> bool {
    let n = compounds.len();
    debug_assert_eq!(combinators.len(), n - 1);

    if !matches_compound(compounds[n - 1], doc, node) {
        return false;
    }
    if n == 1 {
        return true;
    }

    let comb = combinators[n - 2];
    let prev_compounds = &compounds[..n - 1];
    let prev_combinators = &combinators[..n - 2];

    match comb {
        Combinator::Descendant => {
            // Перебираем всех предков как кандидатов.
            let mut cur = doc.get(node).parent;
            while let Some(p) = cur {
                if is_element(doc, p)
                    && matches_chain(prev_compounds, prev_combinators, doc, p)
                {
                    return true;
                }
                cur = doc.get(p).parent;
            }
            false
        }
        Combinator::Child => {
            // Один кандидат: parent.
            let Some(parent) = doc.get(node).parent else { return false; };
            if !is_element(doc, parent) {
                return false;
            }
            matches_chain(prev_compounds, prev_combinators, doc, parent)
        }
        Combinator::NextSibling => {
            // Один кандидат: предыдущий element-sibling.
            let Some(prev) = previous_element_sibling(doc, node) else { return false; };
            matches_chain(prev_compounds, prev_combinators, doc, prev)
        }
        Combinator::LaterSibling => {
            // Перебираем все earlier-siblings как кандидатов.
            let mut sib = previous_element_sibling(doc, node);
            while let Some(s) = sib {
                if matches_chain(prev_compounds, prev_combinators, doc, s) {
                    return true;
                }
                sib = previous_element_sibling(doc, s);
            }
            false
        }
    }
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
