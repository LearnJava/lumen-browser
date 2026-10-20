//! Разбор исходного текста в [`CssomNode`]-дерево (CSS Syntax 3 §5, упрощённо).
//!
//! Универсальный сканер «элемента»: прелюдия до `;` / `{…}` / `}` нулевой
//! глубины с учётом строк, скобок, экранирования и комментариев. Что элемент
//! значит, решает контекст ([`Ctx`]): лист и группы — только правила, тело
//! стилевого правила — декларации вперемешку с правилами, `@keyframes` — кадры.

use super::{CssomNode, RuleKind};
use crate::parser::{canonical_selector_list, split_selector_commas};
use crate::{parse_apply_call, parse_inline_style, Declaration, MIXIN_APPLY_MARKER};

/// Контекст разбора списка элементов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ctx {
    /// Только правила; `root` — корень листа (разрешены `@import`/`@namespace`).
    Rules { root: bool },
    /// Тело стилевого правила: декларации и правила.
    Body,
    /// Тело `@keyframes`: кадры.
    Keyframes,
}

/// Результат разбора списка: ведущие декларации и узлы.
pub(super) struct Items {
    pub(super) decls: Vec<Declaration>,
    pub(super) nodes: Vec<CssomNode>,
    pending: Vec<Declaration>,
}

impl Items {
    fn new() -> Self {
        Self { decls: Vec::new(), nodes: Vec::new(), pending: Vec::new() }
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending);
        if self.nodes.is_empty() {
            self.decls.extend(pending);
        } else {
            self.nodes.push(nested_declarations(pending));
        }
    }

    fn push(&mut self, node: CssomNode) {
        self.flush();
        self.nodes.push(node);
    }
}

/// Узел `NestedDeclarations` из набора деклараций.
pub(super) fn nested_declarations(decls: Vec<Declaration>) -> CssomNode {
    let mut n = CssomNode::new(RuleKind::NestedDeclarations, String::new());
    n.decls = decls;
    n
}

pub(super) fn parse_sheet(src: &str) -> Vec<CssomNode> {
    parse_items(src, Ctx::Rules { root: true }).nodes
}

#[derive(Debug, Clone, Copy)]
enum Term {
    Semi,
    Block { inner: (usize, usize), closed: bool },
    Close,
    Eof,
}

struct Elem {
    start: usize,
    prelude_end: usize,
    term: Term,
    /// Позиция после элемента (после `;` / `}`).
    end: usize,
}

/// Позиция закрывающей `}` для блока, открытого перед `from`, или длина текста.
fn skip_block(b: &[u8], from: usize) -> (usize, bool) {
    let mut depth = 1usize;
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'"' | b'\'' => i = skip_string(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_comment(b, i),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (i, true);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (b.len(), false)
}

/// Индекс последнего байта строки, открытой в `i`.
fn skip_string(b: &[u8], i: usize) -> usize {
    let q = b[i];
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 1,
            c if c == q => return j,
            b'\n' => return j - 1,
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// Индекс последнего байта комментария, открытого в `i`.
fn skip_comment(b: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < b.len() {
        if b[j] == b'*' && b[j + 1] == b'/' {
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

fn skip_trivia(b: &[u8], mut i: usize) -> usize {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b'/') && b.get(i + 1) == Some(&b'*') {
            i = (skip_comment(b, i) + 1).min(b.len());
        } else {
            return i;
        }
    }
}

fn scan(b: &[u8], start: usize) -> Elem {
    let mut depth = 0usize;
    let mut i = start;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'"' | b'\'' => i = skip_string(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_comment(b, i),
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Elem { start, prelude_end: i, term: Term::Semi, end: i + 1 },
            b'}' if depth == 0 => return Elem { start, prelude_end: i, term: Term::Close, end: i },
            b'{' if depth == 0 => {
                let (close, closed) = skip_block(b, i + 1);
                let end = if closed { close + 1 } else { b.len() };
                return Elem { start, prelude_end: i, term: Term::Block { inner: (i + 1, close), closed }, end };
            }
            _ => {}
        }
        i += 1;
    }
    Elem { start, prelude_end: b.len(), term: Term::Eof, end: b.len() }
}

/// Убирает комментарии, схлопывает пробелы (вне строк), обрезает края.
pub(super) fn collapse_ws(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut pending_space = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            pending_space = true;
            i += 1;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = skip_comment(b, i) + 1;
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        if c == b'"' || c == b'\'' {
            let end = (skip_string(b, i) + 1).min(b.len());
            out.push_str(&s[i..end]);
            i = end;
        } else {
            let ch_len = s[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&s[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

/// `(min-width:100px)` → `(min-width: 100px)` (вне `selector(…)`).
pub(super) fn space_after_colons(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut stack: Vec<bool> = Vec::new(); // true — скобка `selector(`
    let chars: Vec<char> = s.chars().collect();
    let mut quote: Option<char> = None;
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => {
                let is_sel = out[..out.len() - 1].to_ascii_lowercase().ends_with("selector");
                stack.push(is_sel || stack.last().copied().unwrap_or(false));
            }
            (None, ')') => {
                stack.pop();
            }
            (None, ':')
                if stack.last() == Some(&false) && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) =>
            {
                out.push(' ');
            }
            _ => {}
        }
    }
    out
}

fn at_name(text: &str) -> (String, &str) {
    let rest = &text[1..];
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
    (rest[..end].to_ascii_lowercase(), &rest[end..])
}

fn kind_of(name: &str) -> Option<RuleKind> {
    Some(match name {
        "media" => RuleKind::Media,
        "supports" => RuleKind::Supports,
        "container" => RuleKind::Container,
        "layer" => RuleKind::Layer,
        "scope" => RuleKind::Scope,
        "starting-style" => RuleKind::StartingStyle,
        "import" => RuleKind::Import,
        "namespace" => RuleKind::Namespace,
        "font-face" => RuleKind::FontFace,
        "keyframes" | "-webkit-keyframes" => RuleKind::Keyframes,
        "page" => RuleKind::Page,
        "property" => RuleKind::Property,
        "counter-style" => RuleKind::CounterStyle,
        "font-feature-values" => RuleKind::FontFeatureValues,
        "mixin" => RuleKind::Mixin,
        _ => return None,
    })
}

/// `--name: {…}` — декларация, а не вложенное правило.
fn is_custom_property_prelude(prelude: &str) -> bool {
    let p = prelude.trim_start();
    p.starts_with("--") && p.contains(':') && !p[..p.find(':').unwrap_or(0)].contains(char::is_whitespace)
}

pub(super) fn parse_items(src: &str, ctx: Ctx) -> Items {
    let b = src.as_bytes();
    let mut items = Items::new();
    let mut pos = 0;
    // Состояние порядка `@import`/`@namespace` на корне листа.
    let mut seen_other = false;
    let mut seen_namespace = false;
    loop {
        pos = skip_trivia(b, pos);
        if pos >= b.len() {
            break;
        }
        let e = scan(b, pos);
        if matches!(e.term, Term::Close) {
            pos = e.end + 1;
            continue;
        }
        let mut prelude_text = &src[e.start..e.prelude_end];
        // `--x: {…};` — блок внутри значения пользовательского свойства.
        if ctx == Ctx::Body && matches!(e.term, Term::Block { .. }) && is_custom_property_prelude(prelude_text) {
            let after = scan(b, e.end);
            let (decl_end, next) = match after.term {
                Term::Semi => (after.prelude_end, after.end),
                _ => (e.end, e.end),
            };
            items.pending.extend(parse_inline_style(&src[e.start..decl_end]));
            pos = next;
            continue;
        }
        pos = e.end;
        prelude_text = prelude_text.trim();
        if prelude_text.is_empty() && !matches!(e.term, Term::Block { .. }) {
            continue;
        }
        if prelude_text.starts_with('@') {
            let (name, params) = at_name(prelude_text);
            if ctx == Ctx::Body && name == "apply" {
                if let Some(val) = apply_value(src, &e) {
                    items.pending.push(Declaration { property: MIXIN_APPLY_MARKER.to_string(), value: val, important: false });
                }
                continue;
            }
            if ctx == Ctx::Keyframes {
                continue;
            }
            let kind = kind_of(&name);
            let node = match kind {
                Some(k) => build_at_rule(src, &e, k, params, ctx, seen_other, seen_namespace),
                None if matches!(ctx, Ctx::Rules { .. }) => Some(opaque(src, &e)),
                None => None,
            };
            if let Some(n) = node {
                match n.kind {
                    RuleKind::Import | RuleKind::LayerStatement => {}
                    RuleKind::Namespace => seen_namespace = true,
                    _ => seen_other = true,
                }
                items.push(n);
            }
            continue;
        }
        // Не at-правило.
        match (ctx, e.term) {
            (Ctx::Rules { .. }, Term::Block { inner, closed }) => {
                if let Some(sel) = canonical_selector_list(prelude_text, false) {
                    let n = style_node(src, &e, sel, inner, closed);
                    seen_other = true;
                    items.push(n);
                }
            }
            (Ctx::Keyframes, Term::Block { inner, closed }) => {
                if let Some(key) = keyframe_key(prelude_text) {
                    let mut n = CssomNode::new(RuleKind::Keyframe, key);
                    n.decls = parse_inline_style(&src[inner.0..inner.1]);
                    n.raw = closed.then(|| src[e.start..e.end].to_string());
                    items.push(n);
                }
            }
            (Ctx::Body, Term::Block { inner, closed }) => {
                if let Some(sel) = canonical_selector_list(prelude_text, true) {
                    items.push(style_node(src, &e, sel, inner, closed));
                }
            }
            (Ctx::Body, _) => {
                items.pending.extend(parse_inline_style(&src[e.start..e.prelude_end]));
            }
            _ => {}
        }
    }
    items.flush();
    items
}

/// Сырой хвост `@apply …` (имя, аргументы, блок) для маркерной декларации.
fn apply_value(src: &str, e: &Elem) -> Option<String> {
    let end = match e.term {
        Term::Block { .. } => e.end,
        _ => e.prelude_end,
    };
    let raw = &src[e.start + "@apply".len()..end];
    parse_apply_call(raw)?;
    Some(raw.to_string())
}

fn opaque(src: &str, e: &Elem) -> CssomNode {
    let mut n = CssomNode::new(RuleKind::Opaque, String::new());
    let mut raw = src[e.start..e.end].trim().to_string();
    if matches!(e.term, Term::Block { closed: false, .. }) {
        raw.push('}');
    } else if matches!(e.term, Term::Eof) {
        raw.push(';');
    }
    n.raw = Some(raw);
    n
}

fn style_node(src: &str, e: &Elem, selector: String, inner: (usize, usize), closed: bool) -> CssomNode {
    let mut n = CssomNode::new(RuleKind::Style, selector);
    let body = parse_items(&src[inner.0..inner.1], Ctx::Body);
    n.decls = body.decls;
    n.children = body.nodes;
    n.raw = closed.then(|| src[e.start..e.end].to_string());
    n
}

pub(super) fn keyframe_key(prelude: &str) -> Option<String> {
    let mut keys = Vec::new();
    for part in split_selector_commas(prelude) {
        let p = part.trim().to_ascii_lowercase();
        let ok = p == "from"
            || p == "to"
            || p.strip_suffix('%').is_some_and(|n| n.parse::<f64>().is_ok_and(|v| (0.0..=100.0).contains(&v)));
        if !ok {
            return None;
        }
        keys.push(p);
    }
    Some(keys.join(", "))
}

fn build_at_rule(
    src: &str,
    e: &Elem,
    kind: RuleKind,
    params: &str,
    ctx: Ctx,
    seen_other: bool,
    seen_namespace: bool,
) -> Option<CssomNode> {
    let root = ctx == Ctx::Rules { root: true };
    let in_body = ctx == Ctx::Body;
    let block = match e.term {
        Term::Block { inner, closed } => Some((inner, closed)),
        _ => None,
    };
    let params_c = collapse_ws(params);
    match kind {
        RuleKind::Import | RuleKind::Namespace => {
            if !root || block.is_some() || params_c.is_empty() || seen_other {
                return None;
            }
            if kind == RuleKind::Import && seen_namespace {
                return None;
            }
            let mut n = CssomNode::new(kind, params_c.clone());
            n.raw = Some(format!("{} {};", &src[e.start..e.start + 1 + kind_name_len(kind)], params_c));
            Some(n)
        }
        RuleKind::Layer => {
            if in_body && block.is_none() {
                return None;
            }
            match block {
                None => {
                    let names: Vec<String> =
                        split_selector_commas(&params_c).iter().map(|s| s.trim().to_string()).collect();
                    if names.iter().any(|s| s.is_empty() || s.contains(char::is_whitespace)) {
                        return None;
                    }
                    let prelude = names.join(", ");
                    let mut n = CssomNode::new(RuleKind::LayerStatement, prelude.clone());
                    n.raw = Some(format!("@layer {prelude};"));
                    Some(n)
                }
                Some((inner, closed)) => {
                    if params_c.contains(',') || params_c.contains(char::is_whitespace) {
                        return None;
                    }
                    Some(group_node(src, e, kind, params_c, inner, closed, ctx))
                }
            }
        }
        RuleKind::Media | RuleKind::Supports | RuleKind::Container | RuleKind::Scope | RuleKind::StartingStyle => {
            let (inner, closed) = block?;
            let prelude = match kind {
                RuleKind::Media | RuleKind::Supports | RuleKind::Container => space_after_colons(&params_c),
                _ => params_c,
            };
            Some(group_node(src, e, kind, prelude, inner, closed, ctx))
        }
        RuleKind::Keyframes => {
            let (inner, closed) = block?;
            if !matches!(ctx, Ctx::Rules { .. }) || params_c.is_empty() {
                return None;
            }
            let mut n = CssomNode::new(kind, params_c);
            n.children = parse_items(&src[inner.0..inner.1], Ctx::Keyframes).nodes;
            n.raw = closed.then(|| src[e.start..e.end].to_string());
            Some(n)
        }
        RuleKind::FontFace | RuleKind::Page | RuleKind::Property | RuleKind::CounterStyle | RuleKind::FontFeatureValues => {
            let (inner, closed) = block?;
            if !matches!(ctx, Ctx::Rules { .. }) {
                return None;
            }
            if kind == RuleKind::Property && !params_c.starts_with("--") {
                return None;
            }
            if matches!(kind, RuleKind::CounterStyle | RuleKind::FontFeatureValues) && params_c.is_empty() {
                return None;
            }
            let mut n = CssomNode::new(kind, params_c);
            n.decls = parse_inline_style(&src[inner.0..inner.1]);
            n.raw = closed.then(|| src[e.start..e.end].to_string());
            Some(n)
        }
        RuleKind::Mixin => {
            if !root {
                return None;
            }
            block?;
            Some(opaque_kind(src, e, RuleKind::Mixin))
        }
        _ => None,
    }
}

fn opaque_kind(src: &str, e: &Elem, kind: RuleKind) -> CssomNode {
    let mut n = opaque(src, e);
    n.kind = kind;
    n
}

fn kind_name_len(kind: RuleKind) -> usize {
    match kind {
        RuleKind::Import => "import".len(),
        _ => "namespace".len(),
    }
}

fn group_node(
    src: &str,
    e: &Elem,
    kind: RuleKind,
    prelude: String,
    inner: (usize, usize),
    closed: bool,
    ctx: Ctx,
) -> CssomNode {
    let mut n = CssomNode::new(kind, prelude);
    let child_ctx = if ctx == Ctx::Body || kind == RuleKind::Scope { Ctx::Body } else { Ctx::Rules { root: false } };
    let body = parse_items(&src[inner.0..inner.1], child_ctx);
    n.children = body.nodes;
    if !body.decls.is_empty() {
        n.children.insert(0, nested_declarations(body.decls));
    }
    n.raw = closed.then(|| src[e.start..e.end].to_string());
    n
}
