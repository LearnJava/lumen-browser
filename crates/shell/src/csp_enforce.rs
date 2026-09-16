//! Content Security Policy enforcement — срез 1 (GAP-CSPENF): `script-src`
//! против инлайновых `<script>`/module-скриптов, взятых из `<meta
//! http-equiv="Content-Security-Policy">`. Срез 4 добавил `img-src`/
//! `default-src` против `<img src>` (host/scheme/`'self'`-источники, не
//! только keyword).
//!
//! Что НЕ покрыто этим срезом (следующие срезы): заголовок
//! `Content-Security-Policy` ответа (только `<meta>`), внешние `<script
//! src>` против host/scheme источников, директивы кроме `script-src`/
//! `img-src` (`connect-src`/`style-src`/…), `report-uri`/`report-to`,
//! hash-источники (только `'unsafe-inline'` и `'nonce-…'`), CSP на путях
//! загрузки картинок помимо eager-пайплайна (lazy-load, стриминговый
//! progressive loader). См. `bugs/BUG-811-OPEN.md`.

use lumen_network::csp::{CspDirective, CspPolicy, CspSource};
use lumen_network::Origin;

use crate::*;

/// Собрать текст каждой `<meta http-equiv="Content-Security-Policy">`
/// документа, в порядке документа. `Content-Security-Policy-Report-Only`
/// не поддерживается через `<meta>` — это и в спеке недопустимо (HTML LS
/// не даёт `http-equiv` репортинг-варианту).
fn collect_meta_csp(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "meta"
    {
        let http_equiv = attrs
            .iter()
            .find(|a| a.name.local == "http-equiv")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if http_equiv.eq_ignore_ascii_case("content-security-policy")
            && let Some(content) = attrs.iter().find(|a| a.name.local == "content")
        {
            out.push(content.value.clone());
        }
        return;
    }
    for &child in &node.children {
        collect_meta_csp(doc, child, out);
    }
}

/// Политика документа для `script-src` — только из `<meta>` (срез 1).
///
/// Несколько `<meta>` CSP по спецификации (CSP3 §3.4) — независимые
/// политики, каждая проверяется отдельно; здесь они упрощённо сливаются в
/// одну строку через `;` — для одиночной политики (подавляющее большинство
/// случаев) результат совпадает, для нескольких политик со связанными
/// ослаблениями (например, `'unsafe-inline'` в одной и `'self'` в другой)
/// это может дать более мягкий эффективный результат, чем спецификация.
///
/// The returned `String` is the combined raw policy text — carried through to
/// `SecurityPolicyViolationEvent.originalPolicy` (CSP3 §7.8), which the
/// parsed [`CspPolicy`] itself does not retain.
pub(crate) fn document_meta_csp_policy(doc: &Document, root: NodeId) -> Option<(CspPolicy, String)> {
    let mut metas = Vec::new();
    collect_meta_csp(doc, root, &mut metas);
    if metas.is_empty() {
        return None;
    }
    let combined = metas.join("; ");
    let policy = lumen_network::csp::parse_csp_header(&combined);
    Some((policy, combined))
}

/// `true`, если `script-src` (или `default-src`) документа запрещает
/// инлайновое исполнение с данным `nonce` (атрибут `nonce` элемента
/// `<script>`, `None` — атрибута нет).
///
/// Отсутствие директивы, применимой к скриптам, — не нарушение (страница не
/// объявляла ограничения). `'strict-dynamic'` без совпавшего nonce НЕ
/// разрешает голый инлайн (CSP3 §8.2) — здесь не учитывается умышленно, тем
/// самым инлайн без nonce остаётся заблокированным.
pub(crate) fn inline_script_blocked(policy: &CspPolicy, nonce: Option<&str>) -> bool {
    let Some(sources) = policy.effective_sources(&CspDirective::ScriptSrc) else {
        return false;
    };
    let allowed = sources.iter().any(|s| match s {
        CspSource::UnsafeInline => true,
        CspSource::Nonce(n) => nonce.is_some_and(|actual| actual == n),
        _ => false,
    });
    !allowed
}

/// Вызвать уже определённый JS-хук `_lumen_dispatch_csp_violation`
/// (`crates/js/src/csp.rs`) — единственная точка диспетчеризации
/// `securitypolicyviolation`, срез 1 зовёт её впервые.
pub(crate) fn fire_script_src_violation(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    original_policy: &str,
) {
    use lumen_core::ext::JsRuntime as _;
    let _ = rt.eval(&format!(
        "_lumen_dispatch_csp_violation({}, {}, {}, 'enforce');",
        js_string_literal("script-src"),
        js_string_literal("inline"),
        js_string_literal(original_policy),
    ));
}

/// `true` if `img-src` (or `default-src`) forbids fetching `url` — срез 4.
/// Absence of a policy is not checked here (the caller only calls this when
/// a policy exists); a `url` that fails to parse is treated as allowed — the
/// fetch proceeds and hits the normal network-failure path instead of a CSP
/// one, same "don't invent a violation" stance as the rest of this module.
pub(crate) fn img_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::ImgSrc, &parsed, self_origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_policy_allows_inline() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!inline_script_blocked(&p, None));
    }

    #[test]
    fn script_src_none_blocks_inline() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(inline_script_blocked(&p, None));
    }

    #[test]
    fn script_src_unsafe_inline_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self' 'unsafe-inline'");
        assert!(!inline_script_blocked(&p, None));
    }

    #[test]
    fn default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'self'");
        assert!(inline_script_blocked(&p, None));
    }

    #[test]
    fn matching_nonce_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(!inline_script_blocked(&p, Some("abc123")));
    }

    #[test]
    fn mismatched_nonce_blocks() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(inline_script_blocked(&p, Some("other")));
    }

    #[test]
    fn no_img_src_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self'");
        assert!(!img_src_blocked(&p, "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_none_blocks() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(img_src_blocked(&p, "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("img-src cdn.example.com");
        assert!(!img_src_blocked(&p, "https://cdn.example.com/x.png", None));
        assert!(img_src_blocked(&p, "https://other.example.com/x.png", None));
    }

    #[test]
    fn img_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!img_src_blocked(&p, "not a url", None));
    }
}
