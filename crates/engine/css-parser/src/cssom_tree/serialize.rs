//! Сериализация дерева: `cssText` правил (формат Chrome, подтверждён WPT
//! `css-nesting/serialize-group-rules-with-decls`, `nested-declarations-cssom`)
//! и исходный текст для каскада.

use super::{CssomNode, RuleKind};
use crate::parser::render_container;
use crate::{parse, parse_apply_call, Declaration, MIXIN_APPLY_MARKER};

fn at_keyword(kind: RuleKind) -> &'static str {
    match kind {
        RuleKind::Media => "@media",
        RuleKind::Supports => "@supports",
        RuleKind::Container => "@container",
        RuleKind::Layer | RuleKind::LayerStatement => "@layer",
        RuleKind::Scope => "@scope",
        RuleKind::StartingStyle => "@starting-style",
        RuleKind::Import => "@import",
        RuleKind::Namespace => "@namespace",
        RuleKind::FontFace => "@font-face",
        RuleKind::Keyframes => "@keyframes",
        RuleKind::Page => "@page",
        RuleKind::Property => "@property",
        RuleKind::CounterStyle => "@counter-style",
        RuleKind::FontFeatureValues => "@font-feature-values",
        _ => "",
    }
}

/// `@keyword prelude` (без пробела, если прелюдия пуста).
fn header(n: &CssomNode) -> String {
    let kw = at_keyword(n.kind);
    if n.prelude.is_empty() {
        kw.to_string()
    } else {
        format!("{kw} {}", n.prelude)
    }
}

fn has_apply(decls: &[Declaration]) -> bool {
    decls.iter().any(|d| d.property == MIXIN_APPLY_MARKER)
}

fn decl_text(d: &Declaration) -> String {
    if d.property == MIXIN_APPLY_MARKER {
        parse_apply_call(&d.value).map(|a| a.css_text()).unwrap_or_default()
    } else {
        d.to_css_text()
    }
}

fn decls_line(decls: &[Declaration]) -> String {
    decls.iter().map(Declaration::to_css_text).collect::<Vec<_>>().join(" ")
}

/// `"x"` / `'x'` → `url("x")`; прочее — как есть. Для `@namespace` строка
/// может идти после префикса.
fn normalize_url(prelude: &str) -> String {
    let (prefix, rest) = match prelude.find(['"', '\'']) {
        Some(i) if prelude[..i].chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ') => {
            (&prelude[..i], &prelude[i..])
        }
        _ => return prelude.to_string(),
    };
    let q = rest.chars().next().unwrap_or('"');
    let Some(close) = rest[1..].find(q) else {
        return prelude.to_string();
    };
    let inner = &rest[1..1 + close];
    let tail = &rest[close + 2..];
    format!("{prefix}url(\"{}\"){tail}", inner.replace('"', "\\\""))
}

/// `style.cssText` узла: собственные декларации одной строкой (`@apply` — как вызов).
pub(super) fn decls_text(n: &CssomNode) -> String {
    n.decls.iter().map(decl_text).collect::<Vec<_>>().join(" ")
}

/// `CSSRule.cssText`.
pub(super) fn css_text(n: &CssomNode) -> String {
    match n.kind {
        RuleKind::Style => style_text(n),
        RuleKind::NestedDeclarations => decls_line(&n.decls),
        RuleKind::Media
        | RuleKind::Supports
        | RuleKind::Container
        | RuleKind::Layer
        | RuleKind::Scope
        | RuleKind::StartingStyle => {
            let kids = serialized_children(n);
            render_container(&header(n), &kids, false)
        }
        RuleKind::Keyframes => {
            let kids: Vec<String> = n.visible_children().map(css_text).collect();
            render_container(&header(n), &kids, false)
        }
        RuleKind::LayerStatement => format!("@layer {};", n.prelude),
        RuleKind::Import | RuleKind::Namespace => format!("{} {};", at_keyword(n.kind), normalize_url(&n.prelude)),
        RuleKind::Keyframe => decl_block(&n.prelude, &n.decls),
        RuleKind::FontFace
        | RuleKind::Page
        | RuleKind::Property
        | RuleKind::CounterStyle
        | RuleKind::FontFeatureValues => decl_block(&header(n), &n.decls),
        RuleKind::Mixin => n
            .raw
            .as_deref()
            .and_then(|r| parse(r).mixin_rules.first().map(|m| m.css_text()))
            .unwrap_or_default(),
        RuleKind::Opaque => n.raw.clone().unwrap_or_default(),
    }
}

/// `cssText` вложенных правил; пустые `CSSNestedDeclarations` на сериализацию
/// внешнего правила не влияют (CSSOM «serialize a CSS rule»).
fn serialized_children(n: &CssomNode) -> Vec<String> {
    n.visible_children()
        .filter(|c| !(c.kind == RuleKind::NestedDeclarations && c.decls.is_empty()))
        .map(css_text)
        .collect()
}

fn decl_block(prelude: &str, decls: &[Declaration]) -> String {
    if decls.is_empty() {
        format!("{prelude} {{ }}")
    } else {
        format!("{prelude} {{ {} }}", decls_line(decls))
    }
}

fn style_text(n: &CssomNode) -> String {
    let apply = has_apply(&n.decls);
    if n.children.is_empty() && !apply {
        return decl_block(&n.prelude, &n.decls);
    }
    let mut parts: Vec<String> = Vec::new();
    if apply {
        parts.extend(n.decls.iter().map(decl_text));
    } else if !n.decls.is_empty() {
        parts.push(decls_line(&n.decls));
    }
    parts.extend(serialized_children(n));
    render_container(&n.prelude, &parts, false)
}

/// Текст всего списка правил для каскада.
pub(super) fn tree_source(nodes: &[CssomNode]) -> String {
    nodes.iter().map(node_source).collect::<Vec<_>>().join("\n")
}

fn decl_source(d: &Declaration) -> String {
    if d.property == MIXIN_APPLY_MARKER {
        format!("@apply{};", d.value.trim_end().trim_end_matches(';'))
    } else {
        d.to_css_text()
    }
}

fn body_source(n: &CssomNode) -> String {
    let mut parts: Vec<String> = n.decls.iter().map(decl_source).collect();
    parts.extend(n.children.iter().map(node_source));
    parts.join(" ")
}

/// Исходный текст узла: `raw` нетронутого, регенерация изменённого.
pub(super) fn node_source(n: &CssomNode) -> String {
    if let Some(raw) = &n.raw {
        return raw.clone();
    }
    match n.kind {
        RuleKind::NestedDeclarations => n.decls.iter().map(decl_source).collect::<Vec<_>>().join(" "),
        RuleKind::Style => format!("{} {{ {} }}", n.prelude, body_source(n)),
        RuleKind::Keyframe => format!("{} {{ {} }}", n.prelude, body_source(n)),
        RuleKind::LayerStatement | RuleKind::Import | RuleKind::Namespace => {
            format!("{} {};", at_keyword(n.kind), n.prelude)
        }
        RuleKind::Mixin | RuleKind::Opaque => String::new(),
        _ => format!("{} {{ {} }}", header(n), body_source(n)),
    }
}
