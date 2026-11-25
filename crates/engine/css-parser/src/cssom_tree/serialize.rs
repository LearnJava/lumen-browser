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
        RuleKind::ViewTransition => "@view-transition",
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

/// `"…"` внутри `rest` (первый символ — кавычка): `(содержимое, хвост после
/// закрывающей кавычки)`. `None` — строка не закрыта.
fn css_quoted(rest: &str) -> Option<(String, &str)> {
    let q = rest.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let mut inner = String::new();
    let mut it = rest[1..].char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '\\' {
            match it.next() {
                Some((_, n)) if n != '\n' && n != '\r' && n != '\x0C' => inner.push(n),
                Some((j, _)) => inner.push_str(&rest[1 + j..1 + j + 1]),
                None => inner.push('\\'),
            }
            continue;
        }
        if c == q {
            return Some((inner, &rest[1 + i + 1..]));
        }
        inner.push(c);
    }
    None
}

/// Префикс перед `url(`/строкой — только `@namespace`-вид (`svg `):
/// иденс/пробелы, без скобок и кавычек.
fn is_url_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' || c == '\t')
}

/// `"x"` / `'x'` / `url("x")` / `url('x')` → `url("x")`; `url(x)` без
/// кавычек и прочее — как есть. Для `@namespace` перед url может стоять
/// префикс (`svg url(…)`). Экранирование — CSSOM «serialize a string»: `\` и `"`.
fn normalize_url(prelude: &str) -> String {
    let s = prelude.trim_start();
    // `url(...)`-форма: `url(` с валидным (возможно пустым для @import)
    // префиксом — граница слова перед ним.
    let lower = s.to_ascii_lowercase();
    for (off, _) in lower.match_indices("url(") {
        let boundary = off == 0 || s.as_bytes()[off - 1].is_ascii_whitespace();
        if !boundary {
            continue;
        }
        let prefix = &s[..off];
        if !(prefix.is_empty() || is_url_prefix(prefix)) {
            continue;
        }
        let body = s[off + 4..].trim_start();
        match css_quoted(body) {
            Some((inner, tail)) if tail.trim_start().starts_with(')') => {
                let esc = inner.replace('\\', "\\\\").replace('"', "\\\"");
                return format!("{prefix}url(\"{esc}\"){}", &tail.trim_start()[1..]);
            }
            _ => match body.find(')') {
                // `url(x)` без кавычек — verbatim: `cssText` сохраняет
                // unquoted-форму (`@import url(x.css)` round-trip).
                Some(close) => {
                    let raw = body[..close].trim();
                    return format!("{prefix}url({raw}){}", &body[close + 1..]);
                }
                None => return prelude.to_string(),
            },
        }
    }
    // Голая строка (`@import "x.css"`, `@namespace url "x"` — префикс может
    // кончаться пробелом).
    for (i, _) in s.match_indices(['"', '\'']) {
        let prefix = &s[..i];
        if prefix.is_empty() || is_url_prefix(prefix) {
            if let Some((inner, tail)) = css_quoted(&s[i..]) {
                let esc = inner.replace('\\', "\\\\").replace('"', "\\\"");
                return format!("{prefix}url(\"{esc}\"){tail}");
            }
            break;
        }
    }
    prelude.to_string()
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
        | RuleKind::FontFeatureValues
        | RuleKind::ViewTransition => decl_block(&header(n), &n.decls),
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
