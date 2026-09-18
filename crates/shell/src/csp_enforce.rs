//! Content Security Policy enforcement — срез 1 (GAP-CSPENF): `script-src`
//! против инлайновых `<script>`/module-скриптов, взятых из `<meta
//! http-equiv="Content-Security-Policy">`. Срез 4 добавил `img-src`/
//! `default-src` против `<img src>` (host/scheme/`'self'`-источники, не
//! только keyword). Срез 5 добавил заголовок `Content-Security-Policy`
//! ответа: он доезжает до документа (`Document::csp_header`) и сливается с
//! `<meta>`-политиками в [`document_csp_policy`], поэтому все точки
//! enforcement видят его без изменений в них самих. Срез 6 добавил
//! `script-src`/`default-src` против внешнего `<script src>` — та же
//! host/scheme/`'self'` проверка, что срез 4 сделал для `img-src`, теперь
//! останавливает fetch внешнего скрипта до сети.
//!
//! Срез 7 добавил `style-src`/`default-src` против внешнего `<link
//! rel=stylesheet>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4 и 6
//! дали `img-src`/`script-src`, применённый к `load_linked_stylesheets`
//! (`crates/shell/src/stylesheets.rs`); заблокированный лист не фетчится и
//! становится тем же `error`-исходом, что уже даёт сетевая неудача (BUG-804).
//!
//! Срез 9 закрыл последний непроверенный производитель картинок:
//! `loading="lazy"` (`Lumen::fetch_and_register_lazy_images`, `page_load.rs`)
//! теперь гейтится тем же `img_src_blocked`, что срез 4 уже дал eager- и
//! streaming-путям.
//!
//! Срез 10 добавил `connect-src` против `fetch()`/`XMLHttpRequest` — не в
//! этом файле: у JS-инициированного запроса нет точки кода с `&Document` под
//! рукой (в отличие от парсер-/страница-производителей выше), поэтому гейт
//! живёт в `lumen-network::HttpClient::fetch_request_impl`
//! (`with_connect_src_policy`, `crates/network/src/lib.rs`), а
//! `document_csp_policy` из этого модуля используется лишь один раз — в
//! `page_pipeline.rs::parse_and_layout`, чтобы собрать политику для этого
//! `HttpClient` перед тем, как он станет `fetch_provider`. Детали —
//! `bugs/BUG-811-OPEN.md` срез 10.
//!
//! Срезы 11/12 (тоже вне этого файла, по той же причине, что срез 10) добавили
//! `connect-src` против WebSocket/EventSource (`crates/network/src/lib.rs`'s
//! `JsWebSocketProvider`/`JsSseProvider`) и `sendBeacon` (`check_connect_src`,
//! `crates/core/src/ext.rs`) — оба делят один `HttpClient` и один
//! `connect_src_policy` с `fetch()`.
//!
//! Срез 13 добавил `worker-src` (falling back to `default-src` — `worker-src`
//! не получил своего child-src/script-src промежуточного шага CSP3 §6.4, тот
//! же однократный фолбэк на `default-src`, что и у всех директив здесь) против
//! `new Worker(url)`/`new SharedWorker(url)`: тоже вне этого файла — у
//! `_lumen_worker_fetch_script`/`_lumen_sw_fetch_script` (`crates/js/src/
//! worker.rs`/`shared_worker.rs`) нет `&Document`, гейт живёт в
//! `lumen-network::HttpClient::check_worker_src` (`with_worker_src_policy`),
//! тот же `document_csp_policy` из `page_pipeline.rs::parse_and_layout`, что
//! срез 10 уже собирает для `connect_src_policy`. Детали — `bugs/
//! BUG-811-OPEN.md` срез 13.
//!
//! Срез 14 (`crates/js/src/csp.rs`, вне этого файла — JS-only) добавил
//! доставку отчётов `report-uri`: `_lumen_dispatch_csp_violation`
//! переизвлекает директиву из уже доехавшей `originalPolicy` и шлёт
//! `fetch(..., {method:'POST'})` на каждый URI. `report-to` не тронут.
//!
//! Срез 15 добавил `frame-src`/`default-src` против навигации `<iframe>`/
//! `<frame>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4/6/7 дали
//! `img-src`/`script-src`/`style-src`, применённый в `frames.rs::spawn_frame`
//! перед вызовом `fetch_iframe_source` (не в этом файле — у гейта нет
//! готового `&Document`/`ResourceBase` без явной проводки, тот же повод, что
//! у срезов 10-13). Проверяются оба пути (первичная вставка и навигация,
//! включая переприсваивание `.src`); `about:blank`/пустой `src` исключены
//! заранее — CSP3 §6.5 их не ограничивает, они не долетают до сети/диска.
//!
//! Срез 18 добавил `img-src`/`default-src` против `background-image:
//! url(...)` страницы (`subresources.rs::fetch_and_decode_background_images`)
//! — переиспользует уже существующий [`img_src_blocked`] (срез 4), гейт
//! только на top-level документе; фон под-документа `<iframe>`
//! (`frames.rs::fetch_frame_background_images`) не тронут этим срезом.
//!
//! Что НЕ покрыто (следующие срезы): остальные директивы (`object-src`/
//! `media-src`/`manifest-src`/…), `report-to` (Reporting API,
//! нужны группы эндпоинтов из `Report-To`, этот движок его не разбирает),
//! hash-источники (только `'unsafe-inline'` и `'nonce-…'`),
//! `@font-face url()` (использует `fetch_font_bytes`/`fetch_image_bytes`
//! напрямую, не гейтится вовсе), `background-image` внутри `<iframe>`
//! (см. выше), инлайновые `<style>`/атрибут `style` (не блокируются, только
//! внешний `<link>`), `@import` внутри уже загруженного листа (наследует
//! политику владельца, отдельно не проверяется), честная независимая
//! проверка заголовка и `<meta>` вместо их слияния, `importScripts()`
//! внутри уже запущенного воркера (`worker-src` гейтит только начальный
//! скрипт конструктора, не последующие `importScripts`). См.
//! `bugs/BUG-811-OPEN.md`.

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

/// Действующая политика документа: заголовок `Content-Security-Policy` ответа
/// (срез 5, `Document::csp_header`) плюс каждая `<meta
/// http-equiv="Content-Security-Policy">` (срез 1), в порядке «заголовок,
/// затем документ».
///
/// Заголовок и каждая `<meta>` по спецификации (CSP3 §3.4) — независимые
/// политики, каждая проверяется отдельно, и нарушение любой из них —
/// нарушение; здесь они упрощённо сливаются в одну строку через `;` — для
/// одиночной политики (подавляющее большинство случаев) результат совпадает,
/// для нескольких политик со связанными ослаблениями (например,
/// `'unsafe-inline'` в одной и `'self'` в другой) это может дать более мягкий
/// эффективный результат, чем спецификация. Честная независимая проверка —
/// отдельная работа (`bugs/BUG-811-OPEN.md`).
///
/// `Content-Security-Policy-Report-Only` не учитывается ни с той, ни с другой
/// стороны: у `<meta>` репортинг-вариант недопустим по HTML LS, а заголовок
/// отфильтрован в `page_source::content_security_policy_header` — здесь
/// enforcement, а report-only по определению ничего не блокирует.
///
/// The returned `String` is the combined raw policy text — carried through to
/// `SecurityPolicyViolationEvent.originalPolicy` (CSP3 §7.8), which the
/// parsed [`CspPolicy`] itself does not retain.
pub(crate) fn document_csp_policy(doc: &Document, root: NodeId) -> Option<(CspPolicy, String)> {
    let mut parts: Vec<String> = doc.csp_header().map(str::to_owned).into_iter().collect();
    collect_meta_csp(doc, root, &mut parts);
    if parts.is_empty() {
        return None;
    }
    let combined = parts.join("; ");
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
/// `securitypolicyviolation`, срез 1 зовёт её впервые для инлайна
/// (`blocked_uri = "inline"`); срез 6 обобщил на внешний `<script src>`
/// (`blocked_uri` = резолвленный адрес файла).
pub(crate) fn fire_script_src_violation(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    blocked_uri: &str,
    original_policy: &str,
) {
    use lumen_core::ext::JsRuntime as _;
    let _ = rt.eval(&format!(
        "_lumen_dispatch_csp_violation({}, {}, {}, 'enforce');",
        js_string_literal("script-src"),
        js_string_literal(blocked_uri),
        js_string_literal(original_policy),
    ));
}

/// `true` if `script-src` (or `default-src`) forbids fetching the external
/// `<script src>` at `url` — срез 6, external counterpart to
/// [`inline_script_blocked`]. Same "don't invent a violation" stance as
/// [`img_src_blocked`]: a `url` that fails to parse is treated as allowed.
pub(crate) fn script_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::ScriptSrc, &parsed, self_origin)
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

/// `true` if `style-src` (or `default-src`) forbids fetching the external
/// `<link rel=stylesheet>` at `url` — срез 7, same fetch-gate shape as
/// [`img_src_blocked`]/[`script_src_blocked`]: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn style_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::StyleSrc, &parsed, self_origin)
}

/// `true` if `frame-src` (or `default-src`) forbids navigating a nested
/// `<iframe>`/`<frame>` to `url` — срез 15, same fetch-gate shape as
/// [`img_src_blocked`]/[`script_src_blocked`]/[`style_src_blocked`]: absence
/// of a policy is not checked here (the caller only calls this when a policy
/// exists), and a `url` that fails to parse is treated as allowed (the
/// caller's own scheme special-cases — `about:blank`, empty `src` — are
/// expected to have already been filtered out before this is called, since
/// those never reach the network/filesystem and CSP3 §6.5 does not restrict
/// them).
pub(crate) fn frame_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::FrameSrc, &parsed, self_origin)
}

/// `true` if `media-src` (or `default-src`) forbids fetching `url` as a
/// `<track src>` WebVTT body — срез 17, same fetch-gate shape as
/// [`img_src_blocked`]/[`style_src_blocked`]/[`frame_src_blocked`].
///
/// This is the shell's half of the `media-src` gate, and it exists because
/// `<track>` bodies are fetched **twice** by this engine from two unrelated
/// places: the JS shim's own `readTrackBody` (gated by the native
/// `_lumen_check_media_src` binding, `lumen-network`) and — before any JS runs
/// — `tracks::load_video_tracks`, the shell's overlay snapshot, which has a
/// `&Document` and so is gated here instead. Gating only the shim's half left
/// the bytes going out anyway.
pub(crate) fn media_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::MediaSrc, &parsed, self_origin)
}

/// `true` if `font-src` (or `default-src`) forbids fetching `url` as an
/// `@font-face url()` body — срез 19, same fetch-gate shape as
/// [`img_src_blocked`]/[`media_src_blocked`]: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn font_src_blocked(policy: &CspPolicy, url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    !policy.fetch_directive_allows(&CspDirective::FontSrc, &parsed, self_origin)
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

    /// GAP-CSPENF срез 5: a document with no `<meta>` CSP still has a policy
    /// when the response carried the header.
    #[test]
    fn response_header_alone_is_a_policy() {
        let mut doc = Document::new();
        doc.set_csp_header(Some("script-src 'none'".to_owned()));
        let root = doc.root();
        let (policy, original) =
            document_csp_policy(&doc, root).expect("header alone must produce a policy");
        assert!(inline_script_blocked(&policy, None));
        assert_eq!(original, "script-src 'none'");
    }

    /// No header and no `<meta>` — no policy at all, so nothing is blocked.
    #[test]
    fn no_header_and_no_meta_is_no_policy() {
        let doc = Document::new();
        let root = doc.root();
        assert!(document_csp_policy(&doc, root).is_none());
    }

    #[test]
    fn img_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!img_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 6: `script-src` against an external `<script src>`.
    #[test]
    fn no_script_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!script_src_blocked(&p, "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(script_src_blocked(&p, "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("script-src cdn.example.com");
        assert!(!script_src_blocked(&p, "https://cdn.example.com/a.js", None));
        assert!(script_src_blocked(&p, "https://other.example.com/a.js", None));
    }

    #[test]
    fn script_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(!script_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 7: `style-src` against an external `<link
    /// rel=stylesheet>`.
    #[test]
    fn no_style_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("style-src cdn.example.com");
        assert!(!style_src_blocked(&p, "https://cdn.example.com/a.css", None));
        assert!(style_src_blocked(&p, "https://other.example.com/a.css", None));
    }

    #[test]
    fn style_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(style_src_blocked(&p, "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(!style_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 15: `frame-src` against `<iframe>`/`<frame>` navigation.
    #[test]
    fn no_frame_src_allows_navigation() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_none_blocks_navigation() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("frame-src cdn.example.com");
        assert!(!frame_src_blocked(&p, "https://cdn.example.com/frame.html", None));
        assert!(frame_src_blocked(&p, "https://other.example.com/frame.html", None));
    }

    #[test]
    fn frame_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(frame_src_blocked(&p, "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(!frame_src_blocked(&p, "not a url", None));
    }

    /// GAP-CSPENF срез 17: `media-src` against the shell's `<track src>` fetch.
    #[test]
    fn no_media_src_allows_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_none_blocks_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("media-src cdn.example.com");
        assert!(!media_src_blocked(&p, "https://cdn.example.com/cap.vtt", None));
        assert!(media_src_blocked(&p, "https://other.example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(!media_src_blocked(&p, "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `media-src`: a page
    /// that locks down `img-src` only has said nothing about its media.
    #[test]
    fn img_src_none_does_not_block_media() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'; media-src *");
        assert!(!media_src_blocked(&p, "https://example.com/cap.vtt", None));
    }

    /// GAP-CSPENF срез 19: `font-src` against `@font-face url()`.
    #[test]
    fn no_font_src_allows_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_none_blocks_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("font-src cdn.example.com");
        assert!(!font_src_blocked(&p, "https://cdn.example.com/font.woff2", None));
        assert!(font_src_blocked(&p, "https://other.example.com/font.woff2", None));
    }

    #[test]
    fn font_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(font_src_blocked(&p, "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(!font_src_blocked(&p, "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `font-src`.
    #[test]
    fn media_src_none_does_not_block_font() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'; font-src *");
        assert!(!font_src_blocked(&p, "https://example.com/font.woff2", None));
    }
}
