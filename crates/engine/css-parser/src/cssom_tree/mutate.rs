//! Мутации дерева CSSOM: `insertRule`, `deleteRule`, сеттеры правил.
//!
//! Правило, затронутое правкой, и все его предки теряют `raw` — для каскада
//! они регенерируются из структуры ([`super::serialize::node_source`]).

use super::parse::{collapse_ws, keyframe_key, nested_declarations, parse_items, space_after_colons, Ctx};
use super::{find, find_mut, CssomNode, CssomTree, RuleKind};
use crate::parser::canonical_selector_list;
use crate::Declaration;

/// Ошибка CSSOM-мутации (имена — `DOMException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssomTreeError {
    /// `IndexSizeError`.
    IndexSize,
    /// `SyntaxError`.
    Syntax,
    /// `HierarchyRequestError`.
    HierarchyRequest,
    /// Узла с таким id нет (устаревшая обёртка).
    NotFound,
}

/// Сбрасывает `raw` у узла `id` и всех его предков.
fn invalidate(nodes: &mut [CssomNode], id: u32) -> bool {
    for n in nodes {
        if n.id == id {
            if n.kind != RuleKind::Mixin {
                n.raw = None;
            }
            return true;
        }
        if invalidate(&mut n.children, id) {
            n.raw = None;
            return true;
        }
    }
    false
}

/// Контекст тела узла `id`; `in_style` — есть ли стилевой предок.
fn body_ctx(nodes: &[CssomNode], id: u32, in_style: bool) -> Option<Ctx> {
    for n in nodes {
        if n.id == id {
            return match n.kind {
                RuleKind::Style | RuleKind::Scope => Some(Ctx::Body),
                RuleKind::Keyframes => Some(Ctx::Keyframes),
                k if k.is_grouping() => Some(if in_style { Ctx::Body } else { Ctx::Rules { root: false } }),
                _ => None,
            };
        }
        if let Some(c) = body_ctx(&n.children, id, in_style || n.kind == RuleKind::Style) {
            return Some(c);
        }
    }
    None
}

/// Есть ли у узла `id` стилевой предок (его селектор — вложенный).
fn has_style_ancestor(nodes: &[CssomNode], id: u32, in_style: bool) -> Option<bool> {
    for n in nodes {
        if n.id == id {
            return Some(in_style);
        }
        if let Some(r) = has_style_ancestor(&n.children, id, in_style || n.kind == RuleKind::Style) {
            return Some(r);
        }
    }
    None
}

fn children_mut(tree: &mut CssomTree, parent: Option<u32>) -> Result<&mut Vec<CssomNode>, CssomTreeError> {
    match parent {
        None => Ok(&mut tree.rules),
        Some(id) => find_mut(&mut tree.rules, id).map(|n| &mut n.children).ok_or(CssomTreeError::NotFound),
    }
}

/// Позиция в `children` для видимого индекса `index` (`index == len` — конец).
fn real_position(children: &[CssomNode], index: usize) -> usize {
    children.iter().enumerate().filter(|(_, c)| c.kind.visible()).nth(index).map_or(children.len(), |(i, _)| i)
}

pub(super) fn insert_rule(
    tree: &mut CssomTree,
    parent: Option<u32>,
    text: &str,
    index: usize,
) -> Result<usize, CssomTreeError> {
    let ctx = match parent {
        None => Ctx::Rules { root: true },
        Some(id) => body_ctx(&tree.rules, id, false).ok_or_else(|| {
            if find(&tree.rules, id).is_some() { CssomTreeError::HierarchyRequest } else { CssomTreeError::NotFound }
        })?,
    };
    let visible_len = match parent {
        None => tree.visible_len(),
        Some(id) => find(&tree.rules, id).map_or(0, CssomNode::visible_len),
    };
    if index > visible_len {
        return Err(CssomTreeError::IndexSize);
    }
    let node = parse_single(text, ctx)?;
    if parent.is_none() {
        check_root_hierarchy(&tree.rules, &node, index)?;
    }
    let children = children_mut(tree, parent)?;
    let pos = real_position(children, index);
    children.insert(pos, node);
    if let Some(id) = parent {
        invalidate(&mut tree.rules, id);
    }
    Ok(index)
}

/// Ровно одно правило (или, в теле стилевого правила, набор деклараций).
fn parse_single(text: &str, ctx: Ctx) -> Result<CssomNode, CssomTreeError> {
    let mut items = parse_items(text, ctx);
    if ctx == Ctx::Body && items.nodes.is_empty() && !items.decls.is_empty() {
        // css-nesting-1: бросаем, только если нет ни одной валидной декларации
        // (csswg-drafts#10520); невалидные среди валидных отбрасываются.
        let mut decls = std::mem::take(&mut items.decls);
        decls.retain(is_known_declaration);
        if decls.is_empty() {
            return Err(CssomTreeError::Syntax);
        }
        return Ok(nested_declarations(decls));
    }
    if !items.decls.is_empty() || items.nodes.len() != 1 {
        return Err(CssomTreeError::Syntax);
    }
    let node = items.nodes.remove(0);
    if node.kind == RuleKind::Opaque || (ctx == Ctx::Keyframes) != (node.kind == RuleKind::Keyframe) {
        return Err(CssomTreeError::Syntax);
    }
    Ok(node)
}

/// Декларация с именем, которое движок знает (или пользовательским/вендорным).
fn is_known_declaration(d: &Declaration) -> bool {
    d.property.starts_with('-')
        || d.property == crate::MIXIN_APPLY_MARKER
        || crate::SUPPORTED_PROPERTIES.iter().any(|p| p.eq_ignore_ascii_case(&d.property))
}

/// CSSOM §6.4.3: порядок `@import` / `@namespace` на корне листа.
fn check_root_hierarchy(rules: &[CssomNode], node: &CssomNode, index: usize) -> Result<(), CssomTreeError> {
    let visible: Vec<&CssomNode> = rules.iter().filter(|r| r.kind.visible()).collect();
    let (before, after) = visible.split_at(index.min(visible.len()));
    let only = |list: &[&CssomNode], kinds: &[RuleKind]| list.iter().all(|r| kinds.contains(&r.kind));
    let ok = match node.kind {
        RuleKind::Import => only(before, &[RuleKind::Import, RuleKind::LayerStatement]),
        RuleKind::Namespace => {
            only(before, &[RuleKind::Import, RuleKind::LayerStatement, RuleKind::Namespace])
                && only(after, &[RuleKind::Namespace])
        }
        RuleKind::LayerStatement => true,
        _ => !after.iter().any(|r| matches!(r.kind, RuleKind::Import | RuleKind::Namespace)),
    };
    if ok { Ok(()) } else { Err(CssomTreeError::HierarchyRequest) }
}

pub(super) fn delete_rule(tree: &mut CssomTree, parent: Option<u32>, index: usize) -> Result<(), CssomTreeError> {
    let children = children_mut(tree, parent)?;
    if index >= children.iter().filter(|c| c.kind.visible()).count() {
        return Err(CssomTreeError::IndexSize);
    }
    let pos = real_position(children, index);
    children.remove(pos);
    if let Some(id) = parent {
        invalidate(&mut tree.rules, id);
    }
    Ok(())
}

pub(super) fn set_prelude(tree: &mut CssomTree, id: u32, text: &str) -> Result<bool, CssomTreeError> {
    let nested = has_style_ancestor(&tree.rules, id, false).ok_or(CssomTreeError::NotFound)?;
    let kind = find(&tree.rules, id).map(|n| n.kind).ok_or(CssomTreeError::NotFound)?;
    let new = match kind {
        RuleKind::Style => canonical_selector_list(text, nested),
        RuleKind::Media => Some(space_after_colons(&collapse_ws(text))),
        RuleKind::Supports | RuleKind::Container => {
            let t = space_after_colons(&collapse_ws(text));
            (!t.is_empty()).then_some(t)
        }
        RuleKind::Keyframe => keyframe_key(text.trim()),
        RuleKind::Keyframes => {
            let t = collapse_ws(text);
            (!t.is_empty()).then_some(t)
        }
        _ => None,
    };
    let Some(new) = new else { return Ok(false) };
    if let Some(n) = find_mut(&mut tree.rules, id) {
        n.prelude = new;
    }
    invalidate(&mut tree.rules, id);
    Ok(true)
}

pub(super) fn set_decls(tree: &mut CssomTree, id: u32, decls: Vec<Declaration>) -> Result<(), CssomTreeError> {
    let node = find_mut(&mut tree.rules, id).ok_or(CssomTreeError::NotFound)?;
    if !matches!(
        node.kind,
        RuleKind::Style
            | RuleKind::FontFace
            | RuleKind::Keyframe
            | RuleKind::Page
            | RuleKind::Property
            | RuleKind::CounterStyle
            | RuleKind::FontFeatureValues
            | RuleKind::NestedDeclarations
    ) {
        return Err(CssomTreeError::Syntax);
    }
    node.decls = decls;
    invalidate(&mut tree.rules, id);
    Ok(())
}
