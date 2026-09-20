//! Reading facts back out of a parsed [`Document`]: the page title, its inline
//! `<style>` text and the fingerprint of that text, plus the window-title
//! format the shell puts on the OS window.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// Найти первый `<title>` в дереве и склеить его текстовые дети.
///
/// HTML5 разрешает только один `<title>` в `<head>`, но мы lenient-парсер —
/// берём первый встречный. Энтити уже декодированы tokenizer-ом (RCDATA-режим).
pub(crate) fn extract_title(doc: &Document) -> Option<String> {
    let mut buf = String::new();
    if walk_title(doc, doc.root(), &mut buf) {
        let trimmed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn walk_title(doc: &Document, id: NodeId, out: &mut String) -> bool {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "title"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
            }
        }
        return true;
    }
    for &child in &node.children {
        if walk_title(doc, child, out) {
            return true;
        }
    }
    false
}

/// GAP-CSPENF срез 21: `csp_gate` — политика документа, если объявлена.
/// Каждый `<style>`-узел проверяется независимо (собственный `nonce`, тело
/// для `'sha256-…'`/nonce/`'unsafe-inline'` — `crate::csp_enforce::
/// inline_style_blocked`, тот же гейт, что срез 1/20 уже дают инлайновым
/// `<script>`), заблокированный узел не попадает в склеенный текст вовсе —
/// тот же принцип «не применённый CSS», что уже применяется к заблокированным
/// внешним `<link>` (срез 7). Возвращает и текст ИМЕННО той политики, что
/// нарушил каждый заблокированный узел (срез 57 — было числом; CSP3 §7.8
/// хочет `originalPolicy` нарушенной политики, не объединённый текст всех) —
/// вызывающий код диспатчит `securitypolicyviolation` по одной записи на узел
/// после того, как появляется JS-рантайм.
pub(crate) fn extract_style_blocks(
    doc: &Document,
    csp_gate: Option<&[lumen_network::csp::CspPolicy]>,
) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut blocked = Vec::new();
    walk_style_blocks(doc, doc.root(), csp_gate, &mut out, &mut blocked);
    (out, blocked)
}

/// Хэш текста всех инлайновых `<style>` в порядке документа (BUG-743).
///
/// Считается на каждом релейауте, поэтому не собирает строку: обходит те же
/// узлы, что и [`walk_style_blocks`], и хэширует их текст по кускам. Меняется
/// при вставке, удалении и правке любого блока — этого достаточно, чтобы
/// понять, что каскад пора пересобрать.
pub(crate) fn inline_style_fingerprint(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_style_blocks(doc, doc.root(), &mut h);
    // Пустой документ и документ без единого `<style>` должны давать один хэш —
    // отдельная соль не нужна, но длина цепочки в него уже вошла.
    0_u8.hash(&mut h);
    h.finish()
}

/// Рекурсивная половина [`inline_style_fingerprint`].
fn hash_style_blocks(doc: &Document, id: NodeId, h: &mut impl std::hash::Hasher) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                h.write(s.as_bytes());
            }
        }
        h.write_u8(0xff);
        return;
    }
    for &child in &node.children {
        hash_style_blocks(doc, child, h);
    }
}

/// Fingerprint of every `<link>` element's `rel`/`href`/`media` in document
/// order (BUG-443).
///
/// The sibling of [`inline_style_fingerprint`] for the *external* half of the
/// cascade. Since BUG-443 the shell collects the page CSS **before** running
/// the document's scripts, so it needs to know whether those scripts touched
/// the set of linked stylesheets — a script-inserted `<link rel=stylesheet>`
/// must still reach the first cascade, and the only way to notice one is to
/// compare this hash across script execution. Cheap: no fetch, no string
/// building, one tree walk.
pub(crate) fn stylesheet_link_fingerprint(doc: &Document) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash_link_elements(doc, doc.root(), &mut h);
    h.write_u8(0);
    h.finish()
}

/// Р екурсивная половина [`stylesheet_link_fingerprint`].
fn hash_link_elements(doc: &Document, id: NodeId, h: &mut impl std::hash::Hasher) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "link"
    {
        for a in attrs {
            h.write(a.name.local.as_bytes());
            h.write_u8(0x1e);
            h.write(a.value.as_bytes());
            h.write_u8(0x1f);
        }
        h.write_u8(0xff);
    }
    for &child in &node.children {
        hash_link_elements(doc, child, h);
    }
}

/// CSS страницы, который динамический `<style>` изменить не может (BUG-743).
///
/// Позволяет пересобрать каскад после поздней вставки `<style>` целиком из
/// памяти: текст, притянутый `@import`-ами инлайновых листов (префикс — по CSS
/// Cascade L4 §6.5 правила импортированного листа идут раньше), и тела внешних
/// `<link rel=stylesheet>` (суффикс — как при первой сборке). Сетевых запросов
/// пересборка не делает, поэтому `@import` внутри *нового* `<style>` останется
/// неразрешённым; это осознанный размен — релейаут не место для сети.
#[derive(Clone)]
pub(crate) struct DynamicCssBase {
    /// Содержимое `@import`-ов инлайновых `<style>`, разрешённое при загрузке.
    pub(crate) imports_prefix: String,
    /// Склеенные тела внешних `<link rel=stylesheet>`.
    pub(crate) linked: String,
    /// Хэш инлайновых `<style>`, из которых собран текущий лист.
    pub(crate) inline_fp: u64,
    /// CSSOM-5 срез 2 (BUG-897): последний увиденный
    /// `PersistentJs::document_adopted_fingerprint()` — тот же принцип, что
    /// у [`Self::inline_fp`], но для `document.adoptedStyleSheets`.
    /// `build_page_cascade` ставит сюда `0` как временный placeholder — на
    /// этом шаге JS ещё не запущен, реального значения взять неоткуда; сразу
    /// после `run_scripts_with_dom` `parse_and_layout` перезаписывает поле
    /// настоящим фингерпринтом, до того как этот `PageCascade` попадёт в
    /// `LayoutSource` и станет виден `refresh_dynamic_css`.
    pub(crate) adopted_fp: u64,
}

fn walk_style_blocks(
    doc: &Document,
    id: NodeId,
    csp_gate: Option<&[lumen_network::csp::CspPolicy]>,
    out: &mut String,
    blocked: &mut Vec<String>,
) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        let mut text = String::new();
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                text.push_str(s);
            }
        }
        if let Some(policy) = csp_gate {
            let nonce = node.get_attr("nonce");
            // Срез 58: one `securitypolicyviolation` per independently
            // violated policy (CSP3 §7.8/§3.4), not just the first.
            let violated = crate::csp_enforce::violating_inline_policy(
                policy,
                &lumen_network::csp::CspDirective::StyleSrc,
                nonce,
                &text,
            );
            if !violated.is_empty() {
                blocked.extend(violated.into_iter().map(str::to_owned));
                return;
            }
        }
        out.push_str(&text);
        out.push('\n');
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, csp_gate, out, blocked);
    }
}

/// GAP-CSPENF срез 23: walk the whole tree once and collect every element
/// whose `style=""` attribute `style-src-attr`/`style-src`/`default-src`
/// forbids (`crate::csp_enforce::violating_style_attr_policy`) — the last
/// inline class срезы 21/22 named as not covered (those gate `<style>`
/// element text; this gates the attribute). Returns the blocked set (handed
/// to [`lumen_dom::Document::set_style_attr_csp_blocked`], the only thing
/// `lumen_layout`'s cascade consults — see that method's doc comment for why
/// the decision travels as bare node ids) plus, in the same document order,
/// the text of the policy that blocked each one (срез 57 — was a bare count;
/// CSP3 §7.8 wants the violated policy's own `originalPolicy`), so the
/// caller can fire one `securitypolicyviolation` per blocked node the same
/// one-shot-push way [`extract_style_blocks`] already does for blocked
/// `<style>` blocks.
pub(crate) fn collect_style_attr_csp_blocked(
    doc: &Document,
    csp_gate: Option<&[lumen_network::csp::CspPolicy]>,
) -> (std::collections::HashSet<NodeId>, Vec<String>) {
    let mut blocked = std::collections::HashSet::new();
    let mut policies = Vec::new();
    if let Some(policy) = csp_gate {
        walk_style_attrs(doc, doc.root(), policy, &mut blocked, &mut policies);
    }
    (blocked, policies)
}

fn walk_style_attrs(
    doc: &Document,
    id: NodeId,
    policy: &[lumen_network::csp::CspPolicy],
    blocked: &mut std::collections::HashSet<NodeId>,
    policies: &mut Vec<String>,
) {
    let node = doc.get(id);
    if let Some(style) = node.get_attr("style")
        && !style.is_empty()
    {
        // Срез 58: one `securitypolicyviolation` per independently violated
        // policy (CSP3 §7.8/§3.4), not just the first.
        let violated = crate::csp_enforce::violating_style_attr_policy(policy, style);
        if !violated.is_empty() {
            blocked.insert(id);
            policies.extend(violated.into_iter().map(str::to_owned));
        }
    }
    for &child in &node.children {
        walk_style_attrs(doc, child, policy, blocked, policies);
    }
}

/// Формат заголовка окна. С title из страницы — `"<title> — Lumen"`,
/// без — fallback на версию билда.
pub(crate) fn window_title(page_title: Option<&str>) -> String {
    match page_title {
        Some(t) => format!("{t} — Lumen"),
        None => format!("Lumen {}", env!("CARGO_PKG_VERSION")),
    }
}

// ── HTML5 Drag and Drop state (PH3-9) ────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// GAP-CSPENF срез 21: two `<style>` blocks, one with a matching `nonce`,
    /// one without — proves the DOM-walking plumbing (`get_attr("nonce")` +
    /// per-node gate) rather than the match logic itself, which
    /// `csp_enforce::tests` already covers exhaustively.
    #[test]
    fn extract_style_blocks_skips_only_the_blocked_node() {
        let doc = lumen_html_parser::parse(
            "<style nonce=\"abc\">a{color:red}</style><style>b{color:blue}</style>",
        );
        let policy = lumen_network::csp::parse_csp_header("style-src 'nonce-abc'");
        let (css, blocked) = extract_style_blocks(&doc, Some(std::slice::from_ref(&policy)));
        assert!(css.contains("a{color:red}"));
        assert!(!css.contains("b{color:blue}"));
        assert_eq!(blocked, vec![policy.raw.clone()]);
    }

    #[test]
    fn extract_style_blocks_no_policy_keeps_everything() {
        let doc = lumen_html_parser::parse("<style>a{color:red}</style>");
        let (css, blocked) = extract_style_blocks(&doc, None);
        assert!(css.contains("a{color:red}"));
        assert!(blocked.is_empty());
    }

    /// GAP-CSPENF срез 23: `style-src-attr 'none'` blocks exactly the one
    /// element carrying a non-empty `style=""` attribute — proves the
    /// DOM-walking plumbing, not `style_attribute_blocked` itself (already
    /// covered exhaustively by `csp_enforce::tests`).
    #[test]
    fn collect_style_attr_csp_blocked_finds_only_the_styled_node() {
        let doc = lumen_html_parser::parse(
            "<div style=\"color:red\">a</div><div>b</div>",
        );
        let policy = lumen_network::csp::parse_csp_header("style-src-attr 'none'");
        let (blocked, policies) = collect_style_attr_csp_blocked(&doc, Some(std::slice::from_ref(&policy)));
        assert_eq!(policies, vec![policy.raw.clone()]);
        assert_eq!(blocked.len(), 1);
    }

    #[test]
    fn collect_style_attr_csp_blocked_no_policy_blocks_nothing() {
        let doc = lumen_html_parser::parse("<div style=\"color:red\">a</div>");
        let (blocked, policies) = collect_style_attr_csp_blocked(&doc, None);
        assert!(policies.is_empty());
        assert!(blocked.is_empty());
    }

    #[test]
    fn collect_style_attr_csp_blocked_unsafe_inline_allows() {
        let doc = lumen_html_parser::parse("<div style=\"color:red\">a</div>");
        let policy = lumen_network::csp::parse_csp_header("style-src-attr 'unsafe-inline'");
        let (blocked, policies) = collect_style_attr_csp_blocked(&doc, Some(std::slice::from_ref(&policy)));
        assert!(policies.is_empty());
        assert!(blocked.is_empty());
    }
}
