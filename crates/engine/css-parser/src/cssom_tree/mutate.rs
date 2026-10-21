//! Мутации дерева CSSOM: `insertRule`, `deleteRule`, сеттеры правил.
//!
//! Правило, затронутое правкой, и все его предки теряют `raw` — для каскада
//! они регенерируются из структуры ([`super::serialize::node_source`]).

use super::parse::{collapse_ws, keyframe_key, nested_declarations, parse_items, space_after_colons, Ctx};
use super::{find, find_mut, CssomNode, CssomTree, RuleKind};
use crate::parser::{canonical_scope_selector_list, canonical_selector_list};
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

/// Контекст тела узла `id`; `parent` — контекст тела, внутри которого лежит `nodes`
/// (`None` — корень листа).
fn body_ctx(nodes: &[CssomNode], id: u32, parent: Option<Ctx>) -> Option<Ctx> {
    for n in nodes {
        let own = match n.kind {
            RuleKind::Style => Some(Ctx::Body),
            RuleKind::Scope => Some(Ctx::ScopeBody),
            RuleKind::Keyframes => Some(Ctx::Keyframes),
            k if k.is_grouping() => Some(parent.unwrap_or(Ctx::Rules { root: false })),
            _ => None,
        };
        if n.id == id {
            return own;
        }
        if let Some(c) = body_ctx(&n.children, id, own) {
            return Some(c);
        }
    }
    None
}

/// Контекст тела, внутри которого лежит узел `id`: `Some(None)` — корень листа,
/// `None` — узла нет.
fn container_ctx(nodes: &[CssomNode], id: u32, parent: Option<Ctx>) -> Option<Option<Ctx>> {
    for n in nodes {
        if n.id == id {
            return Some(parent);
        }
        let own = match n.kind {
            RuleKind::Style => Some(Ctx::Body),
            RuleKind::Scope => Some(Ctx::ScopeBody),
            k if k.is_grouping() => parent,
            _ => None,
        };
        if let Some(c) = container_ctx(&n.children, id, own) {
            return Some(c);
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
        Some(id) => body_ctx(&tree.rules, id, None).ok_or_else(|| {
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
    if ctx.is_body() && is_top_level_only_at_rule(text) {
        return Err(CssomTreeError::HierarchyRequest);
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

/// `@font-face`, `@keyframes`, … бывают только на верхнем уровне листа или в условной
/// группе верхнего уровня: в теле стилевого правила их вставка — `HierarchyRequestError`.
fn is_top_level_only_at_rule(text: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix('@') else { return false };
    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    matches!(
        name.to_ascii_lowercase().as_str(),
        "font-face" | "keyframes" | "-webkit-keyframes" | "property" | "counter-style" | "font-feature-values" | "page" | "import" | "namespace"
    )
}

/// Ровно одно правило (или, в теле стилевого правила, набор деклараций).
fn parse_single(text: &str, ctx: Ctx) -> Result<CssomNode, CssomTreeError> {
    let mut items = parse_items(text, ctx);
    if ctx.is_body() && items.nodes.is_empty() && !items.decls.is_empty() {
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
    let container = container_ctx(&tree.rules, id, None).ok_or(CssomTreeError::NotFound)?;
    let kind = find(&tree.rules, id).map(|n| n.kind).ok_or(CssomTreeError::NotFound)?;
    let new = match kind {
        RuleKind::Style => match container {
            Some(Ctx::ScopeBody) => canonical_scope_selector_list(text),
            Some(_) => canonical_selector_list(text, true),
            None => canonical_selector_list(text, false),
        },
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
