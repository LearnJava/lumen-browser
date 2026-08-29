//! Nested browsing contexts: the `<iframe>`/`<frame>` sandbox gates, where a
//! child document's HTML comes from, the same-origin check its parent is
//! allowed to reach it through, its own subresource pass, and the `load` event
//! fired back at the host element.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;
use crate::relayout::page_measurer;
use lumen_paint::DisplayCommand;

/// Apply sandbox restrictions for all `<iframe sandbox>` elements in the document.
///
/// Two paths depending on whether the iframe has a `srcdoc` attribute:
/// - **`srcdoc` iframes** вЂ” inline HTML is parsed and sandbox gates are applied to
///   the inner document: scripts blocked (if `SCRIPTS`), forms blocked (if `FORMS`),
///   navigation blocked (if `NAVIGATION`), popups blocked (if `AUXILIARY_NAVIGATION`).
/// - **URL-based iframes** вЂ” Phase 0: sub-document is not loaded; logs each active
///   restriction to stderr without applying gates to the host document.
///
/// Returns the total number of blocked capabilities across all sandboxed iframes
/// (script count + form count + navigation link count + popup gate hits).
pub(crate) fn apply_iframe_sandbox_gates(doc: &Document) -> usize {
    let iframes = collect_iframes(doc);
    let mut blocked = 0usize;
    for info in &iframes {
        if !info.is_sandboxed {
            continue;
        }
        let sb = info.sandbox;

        if let Some(html) = &info.srcdoc {
            // srcdoc iframe: parse inline HTML and apply gates to the inner document.
            let inner = lumen_html_parser::parse(html);

            if sb.contains(lumen_core::SandboxFlags::SCRIPTS) {
                let mut scripts = Vec::new();
                let mut modules = Vec::new();
                collect_inline_scripts(&inner, inner.root(), &mut scripts, &mut modules);
                let n = scripts.len() + modules.len();
                if n > 0 {
                    eprintln!(
                        "sandbox: srcdoc iframe вЂ” Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅРѕ {n} СЃРєСЂРёРїС‚(РѕРІ) (sandbox=scripts)"
                    );
                    blocked += n;
                }
            }
            if sb.contains(lumen_core::SandboxFlags::FORMS) {
                blocked += check_form_gate(&inner, sb);
            }
            if sb.contains(lumen_core::SandboxFlags::NAVIGATION) {
                blocked += check_navigation_gate(&inner, sb);
            }
            if check_popup_gate(sb) {
                blocked += 1;
            }
        } else {
            // URL-based iframe: Phase 0 вЂ” sub-document not loaded, log restrictions only.
            let src = info.src.as_deref().unwrap_or("<no src>");
            if sb.contains(lumen_core::SandboxFlags::SCRIPTS) {
                eprintln!("sandbox: iframe '{src}' вЂ” СЃРєСЂРёРїС‚С‹ Р·Р°РїСЂРµС‰РµРЅС‹ (sandbox=scripts)");
            }
            if sb.contains(lumen_core::SandboxFlags::FORMS) {
                eprintln!("sandbox: iframe '{src}' вЂ” С„РѕСЂРјС‹ Р·Р°РїСЂРµС‰РµРЅС‹ (sandbox=forms)");
            }
            if sb.contains(lumen_core::SandboxFlags::NAVIGATION) {
                eprintln!(
                    "sandbox: iframe '{src}' вЂ” РЅР°РІРёРіР°С†РёСЏ Р·Р°РїСЂРµС‰РµРЅР° (sandbox=top-navigation)"
                );
            }
            check_popup_gate(sb);
        }
    }
    blocked
}

// в”Ђв”Ђ iframe sub-РґРѕРєСѓРјРµРЅС‚С‹ (BUG-480) в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// РћС‚РєСѓРґР° Р±СЂР°С‚СЊ HTML sub-РґРѕРєСѓРјРµРЅС‚Р° С„СЂРµР№РјР°.
enum FrameSource {
    /// Р“РѕС‚РѕРІС‹Р№ HTML (Р°С‚СЂРёР±СѓС‚ `srcdoc` / РїСѓСЃС‚РѕР№ `about:blank`).
    Inline(String),
    /// РџСЂРѕС‡РёС‚Р°РЅРЅС‹Р№ С„Р°Р№Р».
    File { html: String, path: std::path::PathBuf },
    /// РўРµР»Рѕ РѕС‚РІРµС‚Р° РїРѕ СЃРµС‚Рё.
    Url { html: String, url: String },
}

/// РџРѕР»СѓС‡РёС‚СЊ РёСЃС…РѕРґРЅРёРє РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р° РґР»СЏ `src`-С„СЂРµР№РјР°: СЂР°Р·СЂРµС€РёС‚СЊ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ
/// `base`, С„Р°Р№Р» РїСЂРѕС‡РёС‚Р°С‚СЊ СЃ РґРёСЃРєР°, URL СЃРєР°С‡Р°С‚СЊ С‡РµСЂРµР· subresource-РєР»РёРµРЅС‚ СЃ
/// `RequestDestination::Document` (С‚РѕС‚ Р¶Рµ mixed-content/SW-РёРЅС‚РµСЂСЃРµРїС‚РѕСЂ, С‡С‚Рѕ Сѓ
/// РѕСЃС‚Р°Р»СЊРЅС‹С… РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ). `None` вЂ” РёСЃС‚РѕС‡РЅРёРє РїРѕР»СѓС‡РёС‚СЊ РЅРµР»СЊР·СЏ (Р»РѕРі РІ stderr).
fn fetch_iframe_source(
    src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<FrameSource> {
    if src.trim().is_empty() {
        return Some(FrameSource::Inline(String::new()));
    }
    let lowered = src.trim_start().to_ascii_lowercase();
    if lowered.starts_with("javascript:") {
        eprintln!("iframe: javascript:-URL РЅРµ РїРѕРґРґРµСЂР¶РёРІР°СЋС‚СЃСЏ (BUG-480 СЃСЂРµР· 1), РїСЂРѕРїСѓСЃРє '{src}'");
        return None;
    }
    if lowered.starts_with("data:") {
        eprintln!("iframe: data:-URL РЅРµ РїРѕРґРґРµСЂР¶РёРІР°СЋС‚СЃСЏ (BUG-480 СЃСЂРµР· 1), РїСЂРѕРїСѓСЃРє '{src}'");
        return None;
    }
    match base.resolve(src) {
        ResolvedResource::File(path) => {
            let html = std::fs::read_to_string(&path)
                .map_err(|e| eprintln!("iframe: С„Р°Р№Р» {} РЅРµ С‡РёС‚Р°РµС‚СЃСЏ: {e}", path.display()))
                .ok()?;
            Some(FrameSource::File { html, path })
        }
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url as _Url;
            use lumen_network::RequestDestination;
            let sub_url = _Url::parse(&url)
                .map_err(|e| eprintln!("iframe: Р±РёС‚С‹Р№ URL '{url}': {e}"))
                .ok()?;
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar);
            let bytes = client
                .fetch_subresource(&sub_url, RequestDestination::Document)
                .map_err(|e| eprintln!("iframe: Р·Р°РіСЂСѓР·РєР° '{url}' РЅРµ СѓРґР°Р»Р°СЃСЊ: {e}"))
                .ok()?;
            Some(FrameSource::Url {
                html: String::from_utf8_lossy(&bytes).into_owned(),
                url,
            })
        }
    }
}

/// Origin-СЃС‚СЂРѕРєР° Р°Р±СЃРѕР»СЋС‚РЅРѕРіРѕ URL (`scheme://host:port`, host РІ РЅРёР¶РЅРµРј СЂРµРіРёСЃС‚СЂРµ).
///
/// РџРѕСЂС‚С‹ РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ (httpв†’80, httpsв†’443) РѕРїСѓСЃРєР°СЋС‚СЃСЏ вЂ” РєР°Рє РІ origin-Р°Р»РіРѕСЂРёС‚РјРµ
/// HTML LS В§7.5.3. `None` вЂ” URL РЅРµ СЂР°СЃРїР°СЂСЃРёР»СЃСЏ РёР»Рё Р±РµР· С…РѕСЃС‚Р° (opaque origin,
/// РєР°Рє Сѓ `file://`).
fn url_origin_str(url: &str) -> Option<String> {
    let u = lumen_core::url::Url::parse(url).ok()?;
    if u.host().is_empty() {
        return None;
    }
    let scheme = u.scheme().to_ascii_lowercase();
    let port = u
        .port()
        .filter(|p| !((scheme == "http" && *p == 80) || (scheme == "https" && *p == 443)))
        .map(|p| format!(":{p}"))
        .unwrap_or_default();
    Some(format!("{scheme}://{}{}", u.host().to_ascii_lowercase(), port))
}

/// РџСЂР°РІРёР»Рѕ РґРѕСЃС‚СѓРїР° СЂРѕРґРёС‚РµР»СЏ Рє РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Сѓ С„СЂРµР№РјР° (BUG-480 СЃСЂРµР· 2).
///
/// HTML LS В§7.3.1.2: `contentDocument` РґРѕСЃС‚СѓРїРµРЅ С‚РѕР»СЊРєРѕ same-origin; opaque
/// origin (`sandbox` Р±РµР· `allow-same-origin`) РЅРµ СЃРѕРІРїР°РґР°РµС‚ РЅРё СЃ С‡РµРј.
/// `about:blank`/`about:srcdoc` РЅР°СЃР»РµРґСѓСЋС‚ origin СЂРѕРґРёС‚РµР»СЏ. Р›РѕРєР°Р»СЊРЅС‹Рµ С„Р°Р№Р»С‹
/// СЃС‡РёС‚Р°РµРј РІР·Р°РёРјРЅРѕ РґРѕСЃС‚СѓРїРЅС‹РјРё (СѓРїСЂРѕС‰С‘РЅРЅР°СЏ РјРѕРґРµР»СЊ Firefox same-directory):
/// Сѓ `file://` РЅРµС‚ С…РѕСЃС‚Р°, Рё СЃС‚СЂРѕРіР°СЏ РїСЂРѕРІРµСЂРєР° СЃРґРµР»Р°Р»Р° Р±С‹ РЅРµРґРѕСЃС‚СѓРїРЅС‹Рј СЃР°РјС‹Р№
/// С‡Р°СЃС‚С‹Р№ Р»РѕРєР°Р»СЊРЅС‹Р№ СЃС†РµРЅР°СЂРёР№; РѕС‚РєР»РѕРЅРµРЅРёРµ РѕС‚ СЃРїРµРєРё Р·Р°РґРѕРєСѓРјРµРЅС‚РёСЂРѕРІР°РЅРѕ РІ
/// bugs/BUG-480-OPEN.md.
/// URL Р±Р°Р·С‹ РІ СЃС‚СЂРѕРєРѕРІРѕР№ С„РѕСЂРјРµ РґР»СЏ С„Р°СЃР°РґРѕРІ `location`/`URL` (BUG-480 СЃСЂРµР· 3).
///
/// Р•РґРёРЅСЃС‚РІРµРЅРЅРѕРµ РєР°РЅРѕРЅРёС‡РµСЃРєРѕРµ РїСЂР°РІРёР»Рѕ РІС‹РІРѕРґР° Р°РґСЂРµСЃР° РёР· [`ResourceBase`] вЂ” С‚Рѕ
/// Р¶Рµ, С‡С‚Рѕ Сѓ `page_url` РІ `parse_and_layout`: СЃРµС‚РµРІР°СЏ Р±Р°Р·Р° Р±РµСЂС‘С‚СЃСЏ РєР°Рє РµСЃС‚СЊ,
/// С„Р°Р№Р»РѕРІР°СЏ РїРѕР»СѓС‡Р°РµС‚ СЃС…РµРјСѓ `file://`.
pub(crate) fn base_url_string(base: &ResourceBase) -> String {
    match base {
        ResourceBase::Url(u) => u.clone(),
        ResourceBase::File(p) => format!("file://{}", p.display()),
    }
}

pub(crate) fn frame_access_allowed(parent_base: &ResourceBase, child_url: &str, opaque_sandbox: bool) -> bool {    if opaque_sandbox {
        return false;
    }
    if child_url.starts_with("about:") {
        return true;
    }
    match parent_base {
        ResourceBase::Url(parent) => match (url_origin_str(parent), url_origin_str(child_url)) {
            (Some(p), Some(c)) => p == c,
            // РҐРѕС‚СЏ Р±С‹ РѕРґРЅР° СЃС‚РѕСЂРѕРЅР° opaque: РІР·Р°РёРјРЅРѕ РґРѕСЃС‚СѓРїРЅС‹ С‚РѕР»СЊРєРѕ РґРІР° С„Р°Р№Р»Р°.
            _ => parent.starts_with("file:") && child_url.starts_with("file:"),
        },
        // РЈ СЂРѕРґРёС‚РµР»СЏ-С„Р°Р№Р»Р° origin opaque: РґРѕСЃС‚СѓРїРµРЅ С‚РѕР»СЊРєРѕ СЂРµР±С‘РЅРѕРє-С„Р°Р№Р»
        // (Сѓ СЃРµС‚РµРІРѕРіРѕ СЂРµР±С‘РЅРєР° РµСЃС‚СЊ С…РѕСЃС‚ вЂ” РѕРЅ РЅРёРєРѕРіРґР° РЅРµ СЂР°РІРµРЅ opaque).
        ResourceBase::File(_) => child_url.starts_with("file:"),
    }
}

/// Р”РёСЃРїРµС‚С‡РµСЂРёР·РѕРІР°С‚СЊ `load` РЅР° `<iframe>`-СЌР»РµРјРµРЅС‚Рµ С‡РµСЂРµР· СЂРѕРґРёС‚РµР»СЊСЃРєРёР№ JS-РєРѕРЅС‚РµРєСЃС‚.
///
/// РЎРѕР±С‹С‚РёРµ РЅРµ РІСЃРїР»С‹РІР°РµС‚ Рё РЅРµ РѕС‚РјРµРЅСЏРµС‚СЃСЏ (HTML LS В§4.8.5); `target` вЂ” СЃР°Рј
/// СЌР»РµРјРµРЅС‚. Р’С‹Р·РѕРІ СЃРёРЅС…СЂРѕРЅРЅС‹Р№: Рє СЌС‚РѕРјСѓ РјРѕРјРµРЅС‚Сѓ СЃРєСЂРёРїС‚С‹ СЂРµР±С‘РЅРєР° СѓР¶Рµ РІС‹РїРѕР»РЅРµРЅС‹ Рё
/// РµРіРѕ DOMContentLoaded РѕС‚РїСЂР°РІР»РµРЅ.
#[allow(unused_variables)] // parent_js С‡РёС‚Р°РµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїРѕРґ feature = "v8"
fn fire_iframe_load_event(parent_js: Option<&Arc<dyn PersistentJs>>, host: NodeId) {
    #[cfg(feature = "v8")]
    if let Some(js) = parent_js {
        js.eval_js(&format!(
            "(function() {{ var e = new Event('load', {{bubbles:false, cancelable:false, isTrusted:true}}); \
             e.target = _lumen_make_element({}); _lumen_dispatch({}, e); }})()",
            host.index(),
            host.index(),
        ));
    }
}

/// Р—Р°РіСЂСѓР·РёС‚СЊ sub-РґРѕРєСѓРјРµРЅС‚С‹ РІСЃРµС… `<iframe>`/`<frame>` РґРѕРєСѓРјРµРЅС‚Р° Рё РІРµСЂРЅСѓС‚СЊ РёС…
/// С…СЌРЅРґР»С‹.
///
/// BUG-854: `<frame>` РїСЂРѕС…РѕРґРёС‚ Р·РґРµСЃСЊ С‚РµРј Р¶Рµ РїСѓС‚С‘Рј, С‡С‚Рѕ `<iframe>` вЂ” СЃРїРёСЃРєРѕРј РёС…
/// РѕР±РѕРёС… РѕС‚РґР°С‘С‚ [`collect_iframes`]; РѕС‚Р»РёС‡РёСЏ С‚РѕР»СЊРєРѕ РІ Р°С‚СЂРёР±СѓС‚Р°С…, РєРѕС‚РѕСЂС‹С… Сѓ
/// `<frame>` РЅРµС‚ (`srcdoc`, `sandbox`, `loading`).
///
/// РЎСЂРµР· 1 BUG-480: РґР»СЏ РєР°Р¶РґРѕРіРѕ С„СЂРµР№РјР° вЂ” СЃРѕР±СЂР°С‚СЊ РёСЃС‚РѕС‡РЅРёРє (`srcdoc` в†’ inline,
/// `src` в†’ С„Р°Р№Р»/СЃРµС‚СЊ; РѕС‚СЃСѓС‚СЃС‚РІРёРµ РѕР±РѕРёС… = `about:blank`), СЂР°СЃРїР°СЂСЃРёС‚СЊ РІ
/// РѕС‚РґРµР»СЊРЅС‹Р№ `Document`, РІС‹РїРѕР»РЅРёС‚СЊ РµРіРѕ СЃРєСЂРёРїС‚С‹ РІ СЃРѕР±СЃС‚РІРµРЅРЅРѕРј JS-РєРѕРЅС‚РµРєСЃС‚Рµ
/// (`run_scripts_with_dom`: С‚РѕС‚ Р¶Рµ РЅР°Р±РѕСЂ РїСЂРѕРІР°Р№РґРµСЂРѕРІ СЃРµС‚Рё Рё С…СЂР°РЅРёР»РёС‰, С‡С‚Рѕ Сѓ
/// СЃС‚СЂР°РЅРёС†С‹), РѕС‚РїСЂР°РІРёС‚СЊ СЂРµР±С‘РЅРєСѓ DOMContentLoaded+load Рё РґРёСЃРїРµРєС‚С‡РЅСѓС‚СЊ `load`
/// РЅР° СЌР»РµРјРµРЅС‚Рµ-С…РѕСЃС‚Рµ. `loading="lazy"` РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ РґРѕ РїРѕСЏРІР»РµРЅРёСЏ
/// viewport-РїСЂРѕРєСЃРё (РѕС‚РґРµР»СЊРЅС‹Р№ СЃСЂРµР·).
///
/// РЎСЂРµР· 3 BUG-480: РєРѕРЅС‚РµРєСЃС‚Сѓ СЂРµР±С‘РЅРєР° РїРµСЂРµРґР°СЋС‚СЃСЏ РґРѕРєСѓРјРµРЅС‚С‹ РїСЂРµРґРєРѕРІ
/// (`window.parent`/`window.top`), Р° СЂРѕРґРёС‚РµР»СЋ вЂ” Р±РёРЅРґРёРЅРі РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р° СЃ РёРјРµРЅРµРј
/// С…РѕСЃС‚Р° (`window[name]`). `top_doc`/`top_base` вЂ” РґРѕРєСѓРјРµРЅС‚ Рё Р±Р°Р·Р° Р’Р•Р РҐРќР•Р“Рћ
/// РѕРєРЅР° СЃС‚СЂР°РЅРёС†С‹; РїСЂРё РїРµСЂРІРѕРј РІС‹Р·РѕРІРµ СЃРѕРІРїР°РґР°СЋС‚ СЃ `parent`/`base`, РІ СЂРµРєСѓСЂСЃРёРё
/// РїРµСЂРµРґР°СЋС‚СЃСЏ Р±РµР· РёР·РјРµРЅРµРЅРёР№.
///
/// РЎСЂРµР· 11 BUG-480: РїРѕРґСЂРµСЃСѓСЂСЃС‹ РїР°СЂСЃРµСЂРЅС‹С… СЌР»РµРјРµРЅС‚РѕРІ СЂРµР±С‘РЅРєР° (`<img src>`,
/// `<link rel=stylesheet>`) Р·Р°РїСЂР°С€РёРІР°СЋС‚СЃСЏ СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ СЂР°Р·Р±РѕСЂР° ([`fetch_frame_subresources`],
/// РґРѕ СЃРєСЂРёРїС‚РѕРІ), Р° РёС… `load`/`error` РґРѕСЃС‚Р°РІР»СЏСЋС‚СЃСЏ РєРѕРЅС‚РµРєСЃС‚Сѓ СЂРµР±С‘РЅРєР° РїРѕСЃР»Рµ DCL
/// Рё РґРѕ window load ([`deliver_frame_subresource_events`]). `media_ctx`/`viewport` вЂ”
/// СЌРєСЂР°РЅРЅС‹Р№ РіРµР№С‚ media `<link>` Рё РІСЊСЋРїРѕСЂС‚ picker-Р° РєР°СЂС‚РёРЅРѕРє: С‚Рµ Р¶Рµ Р·РЅР°С‡РµРЅРёСЏ,
/// С‡С‚Рѕ СЃС‚СЂР°РЅРёС†Р° РёСЃРїРѕР»СЊР·СѓРµС‚ РґР»СЏ СЃРІРѕРёС… РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ.
///
/// Р‘Р»РѕРєРёСЂРѕРІРєРё:
/// - РіР»СѓР±РёРЅР° СЂРµРєСѓСЂСЃРёРё РѕРіСЂР°РЅРёС‡РµРЅР° [`MAX_FRAME_DEPTH`];
/// - `sandbox` Р±РµР· `allow-scripts` РіРµР№С‚РёС‚СЃСЏ РІРЅСѓС‚СЂРё `run_scripts_with_dom`;
/// - `sandbox` Р±РµР· `allow-same-origin` вЂ” opaque origin: СЂРµР±С‘РЅРєСѓ РЅРµ РІС‹РґР°СЋС‚СЃСЏ
///   РїРµСЂСЃРёСЃС‚РµРЅС‚РЅС‹Рµ С…СЂР°РЅРёР»РёС‰Р° (localStorage/IDB/SW/Cache);
/// - РЅР°РІРёРіР°С†РёРѕРЅРЅС‹Рµ Р·Р°РїСЂРѕСЃС‹ РёР· СЃРєСЂРёРїС‚РѕРІ СЂРµР±С‘РЅРєР° (`location.href=`) РїРѕРєР°
///   РѕС‚РєР»РѕРЅСЏСЋС‚СЃСЏ СЃ Р»РѕРіРѕРј вЂ” РЅР°РІРёРіР°С†РёСЏ С„СЂРµР№РјРѕРІ РІРЅРµ СЃСЂРµР·Р° 1.
///
/// Р’С‹Р·С‹РІР°С‚СЊ РјРѕР¶РЅРѕ СЃ Р»СЋР±С‹Рј СЃРѕСЃС‚РѕСЏРЅРёРµРј Р±Р»РѕРєРёСЂРѕРІРѕРє СЃРЅР°СЂСѓР¶Рё: Р»РѕРє СЂРѕРґРёС‚РµР»СЏ
/// Р±РµСЂС‘С‚СЃСЏ РєРѕСЂРѕС‚РєРѕ (С‚РѕР»СЊРєРѕ РѕР±С…РѕРґ РґРµСЂРµРІР°); РІС‹РїРѕР»РЅРµРЅРёРµ СЃРєСЂРёРїС‚РѕРІ СЂРµР±С‘РЅРєР° Рё
/// РґРёСЃРїРµРєС‚С‡ `load` РЅР° С…РѕСЃС‚Рµ РёРґСѓС‚ Р‘Р•Р— СѓРґРµСЂР¶Р°РЅРЅС‹С… Р»Р°РєРѕРІ вЂ” РѕР±СЂР°Р±РѕС‚С‡РёРєРё РІРїСЂР°РІРµ
/// СЃРёРЅС…СЂРѕРЅРЅРѕ С‡РёС‚Р°С‚СЊ DOM РѕР±РµРёС… СЃС‚РѕСЂРѕРЅ.
/// Срез 12 BUG-480: сразу после регистрации `parent`/`top` (выше) —
/// cascade + layout ребёнка на UA-дефолтном вьюпорте [`FRAME_UA_DEFAULT_SIZE`]
/// (реальный host-бокс ещё не известен), результат уходит в
/// `update_layout_rects`/`update_viewport_size` JS-контекста ребёнка — первая
/// content-геометрия внутри фрейма (`getBoundingClientRect` и т.п.) вместо
/// честных нулей. Срез 13: как только layout родителя посчитан,
/// [`sync_frame_viewports`] пересчитывает ребёнка под РЕАЛЬНЫЙ контентный бокс
/// хоста. Paint (компоновка display list ребёнка в бокс `<iframe>` вместо
/// серой заглушки) и relayout при мутациях остаются в очереди среза.
///
/// Исходы подресурсов парсерных элементов под-документа фрейма (BUG-480 срез 11).
pub(crate) struct FrameSubresourceOutcomes {
    /// `(узел <link rel=stylesheet>, лист получен)` в порядке объявления —
    /// форма [`load_linked_stylesheets`].
    pub(crate) links: Vec<(NodeId, bool)>,
    /// `(узел <img>, байты получены)` в порядке DOM.
    pub(crate) images: Vec<(NodeId, bool)>,
    /// BUG-480 срез 15: декодированные картинки ребёнка — `(ключ регистрации,
    /// пиксели)`, форма `LoadedPage::images`. Ключ — РАЗРЕШЁННЫЙ адрес
    /// ([`frame_image_key`]), а не сырой `src`.
    pub(crate) decoded_images: Vec<(String, Arc<lumen_image::Image>)>,
    /// BUG-480 срез 15: `(сырой src, ключ регистрации)` для КАЖДОГО `<img>`
    /// ребёнка — в том числе не загрузившегося.
    ///
    /// По этой карте [`rekey_frame_images`] переписывает ключи в display list
    /// под-документа. Битые картинки в карте тоже: иначе ключ остался бы сырым
    /// и совпал бы с чужим зарегистрированным — во фрейме нарисовалась бы
    /// картинка страницы.
    pub(crate) image_keys: Vec<(String, String)>,
    /// BUG-480 срез 12: текст каскада ребёнка (инлайновые `<style>` с
    /// разрешённым `@import`, затем внешние `<link rel=stylesheet>`, в этом
    /// порядке — форма страницы, `parse_and_layout`). До среза 12 такой текст
    /// не собирался вовсе (фреймы не лежали в layout); теперь его парсит и
    /// использует `load_frame_sub_documents` сразу после этого прохода.
    pub(crate) css: String,
}

/// Запросить подресурсы парсерных элементов под-документа фрейма (BUG-480
/// срез 11): `<link rel=stylesheet>` и `<img src>`.
///
/// До этого среза за URL картинок и листов ребёнка не ходил никто — сервер не
/// видел ни одного запроса (срез 24 зафиксировал это записью запросов), хотя
/// сами элементы в дереве были. Проход повторяет страницу: стили — тот же
/// [`load_linked_stylesheets`] (media-гейт по `media_ctx` страницы), картинки —
/// picker [`lumen_layout::collect_image_requests`] (`<picture>`/`srcset`), чей
/// ключ URL совпадает с тем, что эмитит layout.
///
/// Срез 12: текст каскада (инлайновые `<style>` через `extract_style_blocks`/
/// `inline_css_imports`, затем внешние листы) теперь возвращается вместо
/// отбрасывания — им пользуется layout ребёнка в `load_frame_sub_documents`
/// сразу после этого прохода.
///
/// Срез 15: картинки проходят весь путь страницы, а не только сеть —
/// [`decode_image`] через `IMAGE_CACHE`, intrinsic-размеры в дерево ребёнка
/// (иначе `<img>` без атрибутов лёг бы нулевым боксом) и пиксели наружу для
/// регистрации в рендерере. До среза брались только байты, которые никто не
/// декодировал: рисовать их было некому, пока содержимое фрейма не попадало на
/// экран (срез 14). `loading="lazy"` не запрашивается вовсе: прокси вьюпорта у
/// фреймов нет, так же как срез 1 пропускает сами `loading=lazy`-iframe.
pub(crate) fn fetch_frame_subresources(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    viewport: lumen_core::geom::Size,
    target: lumen_core::ColorSpace,
) -> FrameSubresourceOutcomes {
    let inline = extract_style_blocks(doc);
    let mut css = inline_css_imports(
        &inline,
        base,
        sink,
        cookie_jar.clone(),
        media_ctx,
        &mut std::collections::HashSet::new(),
        0,
    );
    let (linked, links) = load_linked_stylesheets(doc, base, sink, cookie_jar.clone(), media_ctx);
    css.push_str(&linked);

    let requests: Vec<lumen_layout::ImageRequest> =
        lumen_layout::collect_image_requests(doc, viewport)
            .into_iter()
            .filter(|req| !req.is_lazy)
            .collect();
    // Фаза 1 (параллельно): сеть + декодирование, `doc` не трогаем — форма
    // `fetch_and_decode_images` страницы.
    let decoded = parallel_map(&requests, |_, req| {
        let sink: &Arc<dyn EventSink> = &sink.clone();
        let key = frame_image_key(base, &req.url);
        let img = crate::image_cache::IMAGE_CACHE.get_or_decode_current(&key, || {
            decode_image(&req.url, base, sink, cookie_jar.clone(), target)
        });
        (key, img)
    });
    // Фаза 2 (последовательно): intrinsic-размеры в дерево ребёнка и сборка
    // выходных векторов в порядке DOM.
    let mut images = Vec::with_capacity(requests.len());
    let mut decoded_images = Vec::new();
    let mut image_keys = Vec::with_capacity(requests.len());
    for (req, (key, img)) in requests.iter().zip(decoded) {
        image_keys.push((req.url.clone(), key.clone()));
        // BUG-269, как у страницы: intrinsic нужен, если автор не задал ХОТЯ БЫ
        // одно измерение — второе достраивается по соотношению сторон.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        let first = match &img {
            None => None,
            Some(crate::image_cache::DecodedImage::Static(i)) => Some(Arc::clone(i)),
            // Многокадровый GIF: во фрейм идёт первый кадр. Тиканья анимации у
            // под-документов нет (`Lumen::animated_gifs` — карта страницы),
            // поэтому сама анимация наружу не отдаётся.
            Some(crate::image_cache::DecodedImage::Animated { first, .. }) => Some(Arc::clone(first)),
        };
        images.push((req.node_id, first.is_some()));
        if let Some(image) = first {
            if wants_intrinsic {
                lumen_layout::apply_intrinsic_size(doc, req.node_id, image.width, image.height);
            }
            decoded_images.push((key, image));
        }
    }

    FrameSubresourceOutcomes { links, images, css, decoded_images, image_keys }
}

/// Ключ регистрации картинки под-документа фрейма (BUG-480 срез 15):
/// РАЗРЕШЁННЫЙ относительно базы РЕБЁНКА адрес, а не сырой `src`.
///
/// Ключ картинки в `IMAGE_CACHE`, в `Renderer::register_image` и в
/// `DisplayCommand::DrawImage.src` у страницы — сырое значение атрибута, а оно
/// уникально только внутри ОДНОГО документа: страница и фрейм из другого
/// каталога легко держат каждый свой `<img src="pic.png">`. С общим ключом
/// побеждала бы картинка страницы, причём молча. Разрешённый адрес разводит их
/// и, наоборот, СХЛОПЫВАЕТ действительно один и тот же файл — тогда декод
/// разделяется, как и задумано кэшем.
fn frame_image_key(base: &ResourceBase, raw_src: &str) -> String {
    base.resolve_str(raw_src)
}

/// Доставить исходы подресурсов фрейма ([`fetch_frame_subresources`]) его
/// JS-контексту (BUG-480 срез 11).
///
/// Стили идут через `_lumen_deliver_parser_link_events` — тот же проход, что у
/// top-level после каскада (пер-узловой флаг «уже отчитался» внутри шима гасит
/// двойной отчёт для ссылок, вставленных скриптом ребёнка); картинки — через
/// `_lumen_resource_fire`, как парсерные `<script src>` (BUG-804). Зеркало
/// среза 10 внутри `_lumen_resource_fire` автоматически доставит те же события
/// обработчикам фасадов родителя.
fn deliver_frame_subresource_events(js: &Arc<dyn PersistentJs>, sub: &FrameSubresourceOutcomes) {
    use std::fmt::Write as _;
    if !sub.links.is_empty() {
        let mut arg = String::with_capacity(sub.links.len() * 8 + 40);
        arg.push_str("_lumen_deliver_parser_link_events([");
        for (i, (node, ok)) in sub.links.iter().enumerate() {
            if i > 0 {
                arg.push(',');
            }
            let _ = write!(arg, "{},{}", node.index(), u8::from(*ok));
        }
        arg.push_str("]);");
        js.eval_js(&arg);
    }
    for (node, ok) in &sub.images {
        let kind = if *ok { "load" } else { "error" };
        js.eval_js(&format!("_lumen_resource_fire({}, '{kind}');", node.index()));
    }
}

/// Измеритель для layout под-документа фрейма: bundled Inter + системные
/// face-ы, как у страницы ([`page_measurer`]), но без `@font-face`-шрифтов
/// ребёнка — собственного прохода `url()`-загрузки у фрейма пока нет.
///
/// `None` — шрифт не разобрался; вызывающая сторона тогда просто не считает
/// geometry (лог в stderr), а не валит загрузку страницы.
fn frame_measurer() -> Option<lumen_paint::MultiFontMeasurer> {
    match lumen_font::Font::parse(INTER_FONT) {
        Ok(font) => Some(page_measurer(&font, &[])),
        Err(e) => {
            eprintln!("iframe: сбой измерителя шрифта, geometry ребёнка не посчитана: {e}");
            None
        }
    }
}

/// Посчитать cascade + layout под-документа фрейма на заданном вьюпорте и
/// отдать снимок прямоугольников JS-контексту ребёнка (BUG-480 срезы 12/13/14).
///
/// Результат ВОЗВРАЩАЕТСЯ, а не выбрасывается (срез 14): по нему рисуется
/// display list ребёнка и в нём же ищется host-бокс вложенного фрейма
/// (`NodeId` уникален только внутри своего документа, поэтому вложенному фрейму
/// нужен именно layout его собственного родителя, а не страницы).
///
/// `js` необязателен: у фрейма без скриптов JS-контекста нет, но layout ему
/// нужен ровно так же — его содержимое всё равно попадает на экран.
///
/// Лок дерева держится ровно на время прохода: `update_layout_rects` уходит уже
/// без него, потому что это вызов на JS-поток ребёнка.
#[allow(clippy::unwrap_used)] // РєРѕСЂРѕС‚РєРёР№ Р»РѕРє РґРµСЂРµРІР°, docs/lint-policy.md В§10
fn layout_frame_document(
    doc: &Arc<Mutex<Document>>,
    sheet: &lumen_css_parser::Stylesheet,
    viewport: lumen_core::geom::Size,
    js: Option<&Arc<dyn PersistentJs>>,
    measurer: &lumen_paint::MultiFontMeasurer,
) -> lumen_layout::LayoutBox {
    let (frame_layout, rects) = {
        let d = doc.lock().unwrap();
        let frame_layout = lumen_layout::layout_measured(&d, sheet, viewport, measurer);
        let rects = lumen_layout::collect_layout_rects(&frame_layout);
        (frame_layout, rects)
    };
    if let Some(js) = js {
        js.update_layout_rects(rects);
        js.update_viewport_size(viewport.width, viewport.height);
    }
    frame_layout
}

/// КОНТЕНТНЫЙ бокс host-элемента `<iframe>`/`<frame>` в layout родителя —
/// вьюпорт под-документа по HTML LS §4.8.5 и одновременно место, куда
/// вклеивается его display list (срез 14).
///
/// `LayoutBox::rect` — border-бокс, поэтому вычитаются рамки и padding. Порядок
/// операций повторяет приватную `content_box_rect` из `display_list.rs`
/// побитово: срез 14 ищет по этому прямоугольнику команду-заглушку в готовом
/// display list родителя, а сравнение чисел с плавающей точкой переживает
/// перестановку слагаемых не всегда.
pub(crate) fn host_content_rect(b: &lumen_layout::LayoutBox) -> Rect {
    let s = &b.style;
    Rect::new(
        b.rect.x + s.border_left_width + s.padding_left.px(),
        b.rect.y + s.border_top_width + s.padding_top.px(),
        (b.rect.width
            - s.border_left_width
            - s.border_right_width
            - s.padding_left.px()
            - s.padding_right.px())
        .max(0.0),
        (b.rect.height
            - s.border_top_width
            - s.border_bottom_width
            - s.padding_top.px()
            - s.padding_bottom.px())
        .max(0.0),
    )
}

/// Пересчитать layout под-документов фреймов под РЕАЛЬНЫЙ размер их host-бокса
/// (BUG-480 срез 13).
///
/// Срез 12 считал geometry ребёнка на UA-дефолтном [`FRAME_UA_DEFAULT_SIZE`],
/// потому что [`load_frame_sub_documents`] идёт ДО layout страницы-родителя и
/// настоящего размера бокса ещё не знает. Здесь он уже известен: проход
/// вызывается сразу после layout родителя — и на первой загрузке
/// (`parse_and_layout`), и на каждом последующем relayout
/// ([`Lumen::apply_relayout_result`]), поэтому `width:100%`-фрейм переживает
/// ресайз окна, смену зума и любое движение вёрстки над ним.
///
/// Пересчёт идёт ТОЛЬКО когда контентный бокс хоста реально изменился
/// (`FrameHandle::viewport` — размер последнего посчитанного прохода): relayout
/// случается на каждый кадр анимации, а layout под-документа стоит примерно
/// столько же, сколько layout страницы его размера.
///
/// Обход идёт ПО ВОЗРАСТАНИЮ глубины (срез 14): host-элемент фрейма глубины
/// `d` живёт в документе фрейма глубины `d-1`, а `NodeId` уникален только
/// внутри своего документа — искать его в layout страницы значило бы найти либо
/// ничего, либо чужой бокс с совпавшим индексом. Поэтому вложенному фрейму
/// нужен уже пересчитанный layout его собственного родителя, а он готов ровно
/// после прохода предыдущей глубины.
///
/// Display list ребёнка собирается ПОСЛЕ всех layout-ов и в обратном порядке
/// глубин: в него вклеивается содержимое его собственных вложенных фреймов,
/// значит те должны быть нарисованы раньше.
pub(crate) fn sync_frame_viewports(frames: &mut [FrameHandle], page_layout: &lumen_layout::LayoutBox) {
    if frames.is_empty() {
        return;
    }
    let mut measurer: Option<lumen_paint::MultiFontMeasurer> = None;
    // «Layout пересчитан на этом проходе» — гейт для пересборки display list:
    // перерисовывать нужно и сам фрейм, и каждого его предка (его содержимое
    // вклеено в их списки).
    let mut relaid = vec![false; frames.len()];
    for depth in 0..=MAX_FRAME_DEPTH {
        // Фаза 1 — только чтение: где стоит host-бокс каждого фрейма этой
        // глубины. Отдельно от записи, потому что для глубины ≥ 1 читается
        // ЧУЖОЙ элемент того же среза (`layout` фрейма-родителя).
        let mut plan: Vec<(usize, Rect)> = Vec::new();
        for (i, h) in frames.iter().enumerate() {
            if h.depth != depth {
                continue;
            }
            let host = match &h.parent_doc {
                None => crate::forms::find_layout_box(page_layout, h.host),
                Some(pd) => frames
                    .iter()
                    .find(|o| Arc::ptr_eq(&o.doc, pd))
                    .and_then(|p| p.layout.as_ref())
                    .and_then(|pl| crate::forms::find_layout_box(pl, h.host)),
            };
            if let Some(b) = host {
                plan.push((i, host_content_rect(b)));
            }
        }
        // Фаза 2 — запись.
        for (i, rect) in plan {
            // Положение хоста пишется ВСЕГДА, а не только при смене размера:
            // фрейм может уехать вниз, не изменив габаритов (что-то над ним
            // выросло), и тогда вклеивать его содержимое надо по новому адресу.
            frames[i].host_rect = Some(rect);
            // Схлопнутый бокс (`display:none`, нулевые атрибуты) вьюпортом быть
            // не может — ребёнок остаётся на прежнем размере, а не считается в 0.
            if rect.width <= 0.0 || rect.height <= 0.0 {
                continue;
            }
            let size = lumen_core::geom::Size::new(rect.width, rect.height);
            if (size.width - frames[i].viewport.width).abs() < 0.01
                && (size.height - frames[i].viewport.height).abs() < 0.01
                && frames[i].layout.is_some()
            {
                continue;
            }
            if measurer.is_none() {
                measurer = frame_measurer();
            }
            let Some(m) = measurer.as_ref() else { return };
            let layout = layout_frame_document(
                &frames[i].doc,
                &frames[i].sheet,
                size,
                frames[i].js.as_ref(),
                m,
            );
            frames[i].layout = Some(layout);
            frames[i].viewport = size;
            relaid[i] = true;
        }
    }
    rebuild_frame_display_lists(frames, &relaid);
    clamp_frame_scroll(frames);
}

/// Зажать прокрутку под-документов, оказавшуюся за новым пределом (срез 17):
/// содержимое стало ниже или вьюпорт выше.
///
/// Вызывается сразу после пересборки display list'ов и только после неё:
/// предел ([`frame_max_scroll`]) считается по ГОТОВОМУ списку ребёнка.
fn clamp_frame_scroll(frames: &mut [FrameHandle]) {
    for h in frames.iter_mut() {
        let max = frame_max_scroll(h);
        if h.scroll_y <= max {
            continue;
        }
        h.scroll_y = max;
        if let Some(js) = h.js.as_ref()
            && js.set_page_scroll_y(max)
        {
            js.fire_window_scroll();
        }
    }
}

/// Пересчитать под-документ фрейма `idx` после мутации ЕГО DOM — нативное
/// переключение элемента управления формы (BUG-480 срез 18).
///
/// Отличается от [`sync_frame_viewports`] тем, ЧТО изменилось: там менялся
/// размер host-бокса, здесь — само дерево ребёнка при неизменном вьюпорте,
/// то есть гейт «размер не менялся — не пересчитывать» пропустил бы правку
/// молча. Поэтому layout считается здесь напрямую, а `content_dl`
/// ОЧИЩАЕТСЯ: пустой список — единственный признак «перерисовать», который
/// понимает [`rebuild_frame_display_lists`], и через него правка сама доходит
/// до списков всех предков этого фрейма.
///
/// Дальше работу доделывает [`sync_frame_viewports`] — не ради экономии кода,
/// а потому что мутация могла подвинуть host-бокс ВЛОЖЕННОГО фрейма (раскрытый
/// `<details>` над ним), и порядок обхода по глубине живёт только там.
pub(crate) fn relayout_frame_content(
    frames: &mut [FrameHandle],
    idx: usize,
    page_layout: &lumen_layout::LayoutBox,
) {
    let Some(measurer) = frame_measurer() else { return };
    let size = frames[idx].viewport;
    let layout = layout_frame_document(
        &frames[idx].doc,
        &frames[idx].sheet,
        size,
        frames[idx].js.as_ref(),
        &measurer,
    );
    frames[idx].layout = Some(layout);
    frames[idx].content_dl.clear();
    sync_frame_viewports(frames, page_layout);
}

/// Пересобрать display list под-документов, чьё содержимое изменилось
/// (BUG-480 срез 14).
///
/// От глубокого к мелкому: в список фрейма вклеено содержимое его собственных
/// вложенных фреймов, поэтому те должны быть готовы раньше. Перерисовывается
/// фрейм, чей layout пересчитан на этом проходе, чей список ещё пуст (первый
/// проход после загрузки) — и любой, у кого перерисовался потомок.
fn rebuild_frame_display_lists(frames: &mut [FrameHandle], relaid: &[bool]) {
    let mut dirty: Vec<bool> = (0..frames.len())
        .map(|i| relaid[i] || frames[i].content_dl.is_empty())
        .collect();
    for depth in (0..=MAX_FRAME_DEPTH).rev() {
        for i in 0..frames.len() {
            if frames[i].depth != depth {
                continue;
            }
            let child_dirty = frames.iter().enumerate().any(|(j, c)| {
                dirty[j]
                    && c.parent_doc
                        .as_ref()
                        .is_some_and(|pd| Arc::ptr_eq(pd, &frames[i].doc))
            });
            if !dirty[i] && !child_dirty {
                continue;
            }
            let dl = {
                let Some(layout) = frames[i].layout.as_ref() else {
                    continue;
                };
                let mut dl = crate::display_list_metrics::paint_ordered(layout);
                // Срез 15: ключи картинок ребёнка — ДО вклейки содержимого его
                // вложенных фреймов. Их команды уже переписаны своими ключами
                // (список собирается от глубокого к мелкому), а заглушки
                // вложенных фреймов должны остаться со своим `src` — иначе
                // [`splice_one_frame`] их не найдёт.
                rekey_frame_images(&mut dl, frames, i);
                splice_children_of(&mut dl, frames, i);
                dl
            };
            frames[i].content_dl = dl;
            dirty[i] = true;
        }
    }
}

/// Переписать ключи картинок под-документа в его display list (BUG-480 срез 15).
///
/// `paint_ordered` кладёт в `DrawImage.src` сырое значение атрибута — ключ,
/// уникальный лишь внутри своего документа. Регистрируются картинки фрейма под
/// разрешённым адресом ([`frame_image_key`]), поэтому список надо привести к
/// тем же ключам, иначе рендерер не найдёт текстуру и нарисует серую заглушку.
///
/// Заглушки ВЛОЖЕННЫХ фреймов пропускаются по их `src`: [`splice_one_frame`]
/// ищет их именно по нему, и переписанный ключ означал бы серый прямоугольник
/// вместо содержимого внука. Совпасть `src` картинки и `src` фрейма могут
/// только в патологической разметке (`<img>` и `<iframe>` на один адрес), где
/// правильнее сохранить фрейм.
pub(crate) fn rekey_frame_images(dl: &mut DisplayList, frames: &[FrameHandle], idx: usize) {
    if frames[idx].image_keys.is_empty() {
        return;
    }
    for cmd in dl.iter_mut() {
        let DisplayCommand::DrawImage { src, .. } = cmd else { continue };
        if frames.iter().any(|h| {
            h.parent_doc
                .as_ref()
                .is_some_and(|pd| Arc::ptr_eq(pd, &frames[idx].doc))
                && &h.host_src == src
        }) {
            continue;
        }
        if let Some((_, key)) = frames[idx].image_keys.iter().find(|(raw, _)| raw == src) {
            *src = key.clone();
        }
    }
}

/// Что находится под точкой страницы (BUG-480 срез 16).
///
/// Один результат на оба вопроса, потому что задавать их порознь значит дважды
/// пройти hit-тестом по layout страницы, а спрашивают на каждом движении мыши.
pub(crate) struct PointerTarget {
    /// Hit-тест в layout СТРАНИЦЫ. Если точка во фрейме, это его host-элемент
    /// (для вложенного — самый внешний `<iframe>`): именно его фокусирует и
    /// подсвечивает родитель.
    pub(crate) page: Option<lumen_paint::HitTestResult>,
    /// Непусто, если точка попала в содержимое фрейма.
    pub(crate) frame: Option<FramePointerHit>,
}

/// Куда на самом деле указывает точка страницы, если она попала в СОДЕРЖИМОЕ
/// фрейма (BUG-480 срез 16).
pub(crate) struct FramePointerHit {
    /// Индекс хэндла в `Lumen::frames` — самого глубокого фрейма, накрывшего
    /// точку.
    pub(crate) frame: usize,
    /// Та же точка в координатах ВЬЮПОРТА под-документа — `clientX`/`clientY`
    /// события для скриптов ребёнка (CSSOM-View §10: отсчёт от левого верхнего
    /// угла окна просмотра, а не документа).
    ///
    /// Со срезом 17 это уже НЕ та система, в которой ищется [`Self::hit`]:
    /// hit-тест идёт по layout, который о прокрутке не знает (её применяет
    /// вклейка), поэтому там к точке прибавляется `scroll_y`, а наружу отдаётся
    /// вьюпортная. Пока фрейм не прокручен, оба ответа совпадают — потому срез
    /// 16 и обходился одним полем.
    pub(crate) client: Point,
    /// Hit-тест точки в layout под-документа. `None` — под точкой нет ни
    /// одного бокса ребёнка: событие всё равно принадлежит фрейму (родитель
    /// его не увидит), но адресовать его в под-документе некому.
    pub(crate) hit: Option<lumen_paint::HitTestResult>,
}

/// Одинаковый ли документ-хозяин у хэндла и у текущего шага спуска
/// (`None` — страница).
fn same_host_doc(handle: &Option<Arc<Mutex<Document>>>, cur: Option<&Arc<Mutex<Document>>>) -> bool {
    match (handle, cur) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// Перевести точку страницы в под-документ фрейма, если она туда попала
/// (BUG-480 срез 16).
///
/// Спуск идёт ПО ПОПАДАНИЮ В HOST-ЭЛЕМЕНТ, а не по перебору прямоугольников:
/// `hit_test` уже умеет z-index, `transform`, `pointer-events` и клипы, а
/// «содержит ли прямоугольник точку» не умеет ничего из этого — фрейм,
/// накрытый чужим позиционированным блоком, забирал бы клик себе.
///
/// Попадание в САМ host-бокс мимо его контентной части (рамка, padding) фреймом
/// не считается: там точка адресует `<iframe>` как элемент родителя.
///
/// `NodeId` уникален лишь внутри своего документа, поэтому кандидат ищется по
/// паре «host-узел + документ-хозяин»: у вложенного фрейма (глубина ≥ 1) хозяин
/// — документ его собственного фрейма-родителя, и совпадение одного лишь
/// индекса узла нашло бы чужой элемент.
pub(crate) fn pointer_target(
    frames: &[FrameHandle],
    page_layout: &lumen_layout::LayoutBox,
    page: Point,
) -> PointerTarget {
    let mut cur_layout = page_layout;
    let mut cur_doc: Option<&Arc<Mutex<Document>>> = None;
    let mut cur_pt = page;
    let mut page_hit: Option<lumen_paint::HitTestResult> = None;
    let mut best: Option<FramePointerHit> = None;
    // Шагов на один больше предельной глубины: последний завершает спуск и
    // проставляет `hit` даже фрейму самой глубокой вложенности.
    for step in 0..=MAX_FRAME_DEPTH + 1 {
        let hit = hit_test(cur_pt, cur_layout);
        if step == 0 {
            page_hit = hit.clone();
        }
        let descend = hit
            .as_ref()
            .and_then(|h| {
                frames
                    .iter()
                    .position(|f| f.host == h.node && same_host_doc(&f.parent_doc, cur_doc))
            })
            .and_then(|i| {
                let rect = frames[i].host_rect?;
                let layout = frames[i].layout.as_ref()?;
                let inside = cur_pt.x >= rect.x
                    && cur_pt.x < rect.right()
                    && cur_pt.y >= rect.y
                    && cur_pt.y < rect.bottom();
                inside.then_some((i, rect, layout))
            });
        let Some((i, rect, layout)) = descend else {
            // Спуск кончился: точка адресует обычный узел текущего документа.
            if let Some(b) = best.as_mut() {
                b.hit = hit;
            }
            return PointerTarget { page: page_hit, frame: best };
        };
        // Срез 17: та же прокрутка, что сдвигает содержимое при вклейке —
        // иначе клик по видимому блоку попадал бы в тот, что был на этом
        // месте до прокрутки.
        let client = Point::new(cur_pt.x - rect.x, cur_pt.y - rect.y);
        cur_pt = Point::new(client.x, client.y + frames[i].scroll_y);
        cur_layout = layout;
        cur_doc = Some(&frames[i].doc);
        best = Some(FramePointerHit { frame: i, client, hit: None });
    }
    PointerTarget { page: page_hit, frame: best }
}

/// Вклеить содержимое всех под-документов ГЛУБИНЫ 0 в display list страницы
/// (BUG-480 срез 14) — вместо серой заглушки, которую `display_list.rs` рисует
/// для `BoxKind::Iframe`.
///
/// Вызывается на каждой записи `Lumen::display_list`, а не один раз на загрузку:
/// список страницы пересобирается из layout при каждом relayout и о фреймах
/// ничего не знает.
///
/// Идемпотентна: заглушка ищется по своей команде, а после вклейки её там
/// больше нет — повторный проход по уже склеенному списку ничего не делает.
pub(crate) fn splice_frame_content(dl: &mut DisplayList, frames: &[FrameHandle]) {
    for h in frames.iter().filter(|h| h.parent_doc.is_none()) {
        splice_one_frame(dl, h);
    }
}

/// То же для вложенных фреймов: вклеить в список фрейма `parent` содержимое
/// тех фреймов, чей host-элемент лежит в ЕГО документе.
fn splice_children_of(dl: &mut DisplayList, frames: &[FrameHandle], parent: usize) {
    for h in frames.iter().filter(|h| {
        h.parent_doc
            .as_ref()
            .is_some_and(|pd| Arc::ptr_eq(pd, &frames[parent].doc))
    }) {
        splice_one_frame(dl, h);
    }
}

/// Заменить команду-заглушку одного `<iframe>`/`<frame>` на содержимое его
/// под-документа.
///
/// Заглушка — `DrawImage` с ключом-`src` элемента по его контентному боксу
/// (`display_list.rs`, ветка `BoxKind::Iframe`): нерегистрированный ключ
/// рисуется серым. Ищется по ПАРЕ «тот же `src` + тот же прямоугольник» —
/// одного `src` мало (два `<iframe src="">` на странице — обычное дело), одного
/// прямоугольника мало для гарантии, что это именно заглушка, а не совпавшая по
/// геометрии картинка.
///
/// Координаты ребёнка начинаются от его собственного (0, 0), поэтому вокруг
/// содержимого встают `PushClipRect` (в системе координат родителя — клип
/// применяется ДО трансформы) и `PushTransform` на смещение к боксу.
///
/// Прокрутка под-документа (срез 17) входит в ЭТО смещение, а не в клип:
/// клип — это окно фрейма на странице, оно на месте, а уезжает содержимое.
fn splice_one_frame(dl: &mut DisplayList, h: &FrameHandle) {
    let Some(rect) = h.host_rect else { return };
    if h.content_dl.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let Some(at) = dl.iter().position(|c| match c {
        DisplayCommand::DrawImage { rect: r, src, .. } => {
            src == &h.host_src
                && (r.x - rect.x).abs() < 0.01
                && (r.y - rect.y).abs() < 0.01
                && (r.width - rect.width).abs() < 0.01
                && (r.height - rect.height).abs() < 0.01
        }
        _ => false,
    }) else {
        return;
    };
    let mut wrapped: DisplayList = Vec::with_capacity(h.content_dl.len() + 4);
    wrapped.push(DisplayCommand::PushClipRect { rect });
    wrapped.push(DisplayCommand::PushTransform {
        matrix: lumen_layout::Mat4::translation_2d(rect.x, rect.y - h.scroll_y),
    });
    wrapped.extend(h.content_dl.iter().cloned());
    wrapped.push(DisplayCommand::PopTransform);
    wrapped.push(DisplayCommand::PopClip);
    dl.splice(at..at + 1, wrapped);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
#[allow(clippy::unwrap_used)] // РєРѕСЂРѕС‚РєРёР№ Р»РѕРє РґРµСЂРµРІР°; poisoned mutex = РїР°РЅРёРєР° РїРѕС‚РѕРєР° Р·Р°РіСЂСѓР·РєРё, docs/lint-policy.md В§10
pub(crate) fn load_frame_sub_documents(
    parent: &Arc<Mutex<Document>>,
    depth: usize,
    base: &ResourceBase,
    top_doc: &Arc<Mutex<Document>>,
    top_base: &ResourceBase,
    media_ctx: &lumen_css_parser::MediaContext,
    viewport: lumen_core::geom::Size,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
    sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    cross_origin_isolated: bool,
    parent_js: Option<&Arc<dyn PersistentJs>>,
    // BUG-480 срез 15: целевое цветовое пространство декодера картинок — то же,
    // с которым страница декодирует свои (`parse_and_layout`).
    target: lumen_core::ColorSpace,
) -> Vec<FrameHandle> {
    // URL СЂРѕРґРёС‚РµР»СЏ Рё РІРµСЂС…Р° РґР»СЏ С„Р°СЃР°РґРѕРІ location/URL Сѓ РїСЂРµРґРєРѕРІ (СЃСЂРµР· 3):
    // РІС‹С‡РёСЃР»СЏСЋС‚СЃСЏ РѕРґРёРЅ СЂР°Р· РЅР° СѓСЂРѕРІРµРЅСЊ СЂРµРєСѓСЂСЃРёРё.
    let parent_url = base_url_string(base);
    let top_url = base_url_string(top_base);
    // РљРѕСЂРѕС‚РєРёР№ Р»РѕРє: СЃРѕР±РёСЂР°РµРј РѕРїРёСЃР°РЅРёСЏ С„СЂРµР№РјРѕРІ Рё РѕС‚РїСѓСЃРєР°РµРј РґРµСЂРµРІРѕ вЂ” РґР°Р»СЊС€Рµ
    // СЃРµС‚СЊ/СЃРєСЂРёРїС‚С‹/СЃРѕР±С‹С‚РёСЏ, РєРѕС‚РѕСЂС‹Рµ РІРїСЂР°РІРµ С‡РёС‚Р°С‚СЊ РґРѕРєСѓРјРµРЅС‚.
    let infos = {
        let d = parent.lock().unwrap();
        collect_iframes(&d)
    };
    if infos.is_empty() {
        return Vec::new();
    }
    let mut handles = Vec::new();
    for info in infos {
        if info.loading_lazy {
            continue;
        }
        // РСЃС‚РѕС‡РЅРёРє HTML + Р±Р°Р·Р° СЂРµР±С‘РЅРєР° РґР»СЏ РµРіРѕ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… URL.
        let (html, child_base, child_url): (String, ResourceBase, String) = match &info.srcdoc {
            Some(srcdoc) => (srcdoc.clone(), base.clone(), "about:srcdoc".to_owned()),
            None => match info.src.as_deref() {
                Some(src) => match fetch_iframe_source(src, base, sink, cookie_jar.clone()) {
                    Some(FrameSource::Inline(html)) => (html, base.clone(), "about:blank".to_owned()),
                    Some(FrameSource::File { html, path }) => {
                        let url = format!("file://{}", path.display());
                        (html, ResourceBase::File(path), url)
                    }
                    Some(FrameSource::Url { html, url }) => {
                        (html, ResourceBase::Url(url.clone()), url)
                    }
                    None => continue,
                },
                // РќРё src, РЅРё srcdoc вЂ” СЃРїРµРєР° РіСЂСѓР·РёС‚ about:blank РЅРµРјРµРґР»РµРЅРЅРѕ.
                None => (String::new(), base.clone(), "about:blank".to_owned()),
            },
        };

        let mut child_doc = {
            let _s = lumen_core::trace::span("parse-html-frame", "parse");
            lumen_html_parser::parse(&html)
        };
        // СРЕЗ 11 BUG-480: подресурсы парсерных элементов ребёнка (`<img src>`,
        // `<link rel=stylesheet>`). Сеть стартует ДО скриптов — парсерный порядок
        // (источник запроса — шаг разбора, а не исполнение); исходы держим до
        // создания рантайма и доставляем ниже, между DCL и window load.
        let subresources = {
            let _s = lumen_core::trace::span("fetch-frame-subresources", "net");
            fetch_frame_subresources(
                &mut child_doc,
                &child_base,
                sink,
                cookie_jar.clone(),
                media_ctx,
                viewport,
                target,
            )
        };
        // РЎРєСЂРёРїС‚С‹ СЂРµР±С‘РЅРєР° СЃРѕР±РёСЂР°СЋС‚СЃСЏ Рё (РІРЅРµС€РЅРёРµ) СЃРєР°С‡РёРІР°СЋС‚СЃСЏ Р”Рћ РїРµСЂРµРґР°С‡Рё
        // РґРѕРєСѓРјРµРЅС‚Р° РІ СЂР°РЅС‚Р°Р№Рј: run_scripts_with_dom РїСЂРёРЅРёРјР°РµС‚ doc РїРѕ Р·РЅР°С‡РµРЅРёСЋ.
        let (classic_scripts, module_scripts) = {
            let mut classic_items = Vec::new();
            let mut module_items = Vec::new();
            collect_scripts_ordered(&child_doc, child_doc.root(), &mut classic_items, &mut module_items);
            (
                resolve_script_sources(&classic_items, &child_base, sink, cookie_jar.clone()),
                resolve_script_sources(&module_items, &child_base, sink, cookie_jar.clone()),
            )
        };
        // Opaque origin (sandbox Р±РµР· allow-same-origin) вЂ” Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅС‹С…
        // С…СЂР°РЅРёР»РёС‰; РїСЂРѕРІР°Р№РґРµСЂС‹ СЃРµС‚Рё РѕСЃС‚Р°СЋС‚СЃСЏ: sandbox СЂРµР¶РµС‚ origin-РґРѕСЃС‚СѓРї,
        // Р° РЅРµ СЃРµС‚СЊ (СЃРєСЂРёРїС‚С‹ С†РµР»РёРєРѕРј РіРµР№С‚СЏС‚СЃСЏ С„Р»Р°РіРѕРј SCRIPTS РѕС‚РґРµР»СЊРЅРѕ).
        let opaque = info.is_sandboxed && info.sandbox.contains(lumen_core::SandboxFlags::ORIGIN);
        let (child_doc_arc, child_nav, child_js) = run_scripts_with_dom(
            child_doc,
            info.sandbox,
            &child_url,
            fetch_provider.clone(),
            ws_provider.clone(),
            sse_provider.clone(),
            ls_store.clone().filter(|_| !opaque),
            ss_store.clone().filter(|_| !opaque),
            idb_backend.clone().filter(|_| !opaque),
            sw_backend.clone().filter(|_| !opaque),
            sw_worker_store.clone().filter(|_| !opaque),
            cache_backend.clone().filter(|_| !opaque),
            cookie_banner_dismiss,
            deterministic,
            cross_origin_isolated,
            &[],
            classic_scripts,
            module_scripts,
            // BUG-480 срез 8: фрейму рантайм нужен даже без единого парсерного
            // скрипта — иначе ему нечем принимать кросс-фреймовые postMessage/
            // события/RunScript (срезы 4–8), а статические iframe — самый
            // частый встраиваемый случай. Странице (второй вызов) хватает
            // старого поведения: без скриптов ей нечем отвечать.
            true,
        );
        // РќР°РІРёРіР°С†РёСЏ РёР· СЃРєСЂРёРїС‚РѕРІ СЂРµР±С‘РЅРєР° (location.href= Рё С‚.Рї.) РІРЅРµ СЃСЂРµР·Р° 1:
        // РѕС‚РєР»РѕРЅСЏРµРј СЃ Р»РѕРіРѕРј, РЅРµ Р·Р°РІР°Р»РёРІР°СЏ СЃС‚СЂР°РЅРёС†Сѓ.
        if let Some(nav) = child_nav {
            let target = match nav {
                JsNavigateRequest::Push(url) | JsNavigateRequest::Replace(url) => url,
                _ => "<reload/submit>".to_owned(),
            };
            eprintln!("iframe: РЅР°РІРёРіР°С†РёСЏ РёР· РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р° ({child_url}) РЅРµ РїРѕРґРґРµСЂР¶РёРІР°РµС‚СЃСЏ (BUG-480 СЃСЂРµР· 1), Р·Р°РїСЂРѕСЃ '{target}' РѕС‚РєР»РѕРЅС‘РЅ");
        }
        // РЎСЂРµР· 3 BUG-480: СЃСЃС‹Р»РєРё РЅР° РїСЂРµРґРєРѕРІ РІ РєРѕРЅС‚РµРєСЃС‚Рµ СЂРµР±С‘РЅРєР° вЂ” РґРѕ РµРіРѕ
        // DOMContentLoaded/load, С‡С‚РѕР±С‹ РѕР±СЂР°Р±РѕС‚С‡РёРєРё (РІ С‚.С‡. РІСЃС‚СЂРѕРµРЅРЅС‹Р№
        // testharness РЅР° window load) С‡РёС‚Р°Р»Рё window.parent/top/frameElement
        // СЃСЂР°Р·Сѓ. РРЅР»Р°Р№РЅ-СЃРєСЂРёРїС‚С‹ СЂРµР±С‘РЅРєР° Рє СЌС‚РѕРјСѓ РјРѕРјРµРЅС‚Сѓ СѓР¶Рµ РёСЃРїРѕР»РЅРµРЅС‹ Рё РїСЂРё
        // С‡С‚РµРЅРёРё РІРёРґРµР»Рё РїСЂРµР¶РЅРёР№ fallback (parent === window) вЂ” РёР·РІРµСЃС‚РЅРѕРµ
        // РѕРіСЂР°РЅРёС‡РµРЅРёРµ СЃСЂРµР·Р°.
        if let Some(js) = &child_js {
            let accessible_parent = frame_access_allowed(base, &child_url, opaque);
            js.register_parent_document(
                info.node.index() as u32,
                Arc::clone(parent),
                &parent_url,
                accessible_parent,
            );
            // Р РµР±С‘РЅРѕРє РіР»СѓР±РёРЅС‹ в‰Ґ 2 РїРѕР»СѓС‡Р°РµС‚ РѕС‚РґРµР»СЊРЅС‹Р№ СЃР»РѕС‚ top: РµРіРѕ РІРµСЂС… вЂ”
            // РєРѕСЂРµРЅСЊ СЃС‚СЂР°РЅРёС†С‹, Р° РЅРµ РЅРµРїРѕСЃСЂРµРґСЃС‚РІРµРЅРЅС‹Р№ СЂРѕРґРёС‚РµР»СЊ.
            if depth >= 1 {
                let accessible_top = frame_access_allowed(top_base, &child_url, opaque);
                js.register_top_document(Arc::clone(top_doc), &top_url, accessible_top);
            }
        }
        // BUG-480 срез 12: cascade + layout ребёнка — контентная геометрия
        // внутри фрейма (getBoundingClientRect/offsetWidth/offsetHeight)
        // вместо честных нулей (см. frame_bridge.rs: «layout содержимого
        // фрейма — отдельный срез»). Вьюпорт — [`FRAME_UA_DEFAULT_SIZE`]
        // (реальный размер host-бокса ещё не известен на этом шаге).
        // Измеритель собран как у страницы ([`page_measurer`]), но без
        // @font-face ребёнка (`web_fonts: &[]` — задел следующего среза).
        // Каскад ребёнка разбирается один раз и переезжает в хэндл: срез 13
        // пересчитывает layout под реальный host-бокс, и повторный разбор
        // того же текста на каждом relayout был бы чистой тратой. Сам layout
        // тоже едет в хэндл (срез 14): по нему рисуется содержимое фрейма и в
        // нём ищется host-бокс вложенного фрейма.
        let frame_sheet = lumen_css_parser::parse(&subresources.css);
        let frame_layout = frame_measurer().map(|measurer| {
            layout_frame_document(
                &child_doc_arc,
                &frame_sheet,
                FRAME_UA_DEFAULT_SIZE,
                child_js.as_ref(),
                &measurer,
            )
        });
        // Lifecycle СЂРµР±С‘РЅРєР°: DOMContentLoaded СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ parse+inline-СЃРєСЂРёРїС‚РѕРІ
        // (С‚РѕС‚ Р¶Рµ РїРѕСЂСЏРґРѕРє, С‡С‚Рѕ Сѓ top-level РІ parse_and_layout); window load вЂ”
        // СЃР»РµРґРѕРј, РќРћ РїРѕСЃР»Рµ РёСЃС…РѕРґРѕРІ РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ (СЃСЂРµР· 11): В«loadВ» РґРѕРєСѓРјРµРЅС‚Р°
        // СЃР»РµРґСѓРµС‚ Р·Р° РµРіРѕ РїРѕРґСЂРµСЃСѓСЂСЃР°РјРё, Рё С‚РµСЃС‚, РіРґРµ РІРЅСѓС‚СЂРё window load С‡РёС‚Р°СЋС‚
        // Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ `<img>`/`link.onload`, СЂР°Р±РѕС‚Р°РµС‚.
        if let Some(js) = &child_js {
            js.notify_dom_content_loaded();
            deliver_frame_subresource_events(js, &subresources);
            js.notify_window_loaded();
        }
        // Р’Р»РѕР¶РµРЅРЅС‹Рµ С„СЂРµР№РјС‹ СЂРµР±С‘РЅРєР° РѕР±СЂР°Р±Р°С‚С‹РІР°РµРј, РїРѕРєР° РёР·РІРµСЃС‚РЅР° РµРіРѕ Р±Р°Р·Р°.
        // РҐСЌРЅРґР»С‹ СѓРїР»РѕС‰Р°СЋС‚СЃСЏ РІ РѕР±С‰РёР№ СЃРїРёСЃРѕРє СЃС‚СЂР°РЅРёС†С‹: РІСЂРµРјСЏ Р¶РёР·РЅРё РІСЃРµС…
        // РїРѕРґ-РґРѕРєСѓРјРµРЅС‚РѕРІ РїСЂРёРІСЏР·Р°РЅРѕ Рє СЃС‚СЂР°РЅРёС†Рµ С†РµР»РёРєРѕРј (Р·Р°РјРµРЅР°/СѓРґР°Р»РµРЅРёРµ
        // РѕС‚РґРµР»СЊРЅРѕРіРѕ С„СЂРµР№РјР° вЂ” Р±СѓРґСѓС‰РёР№ СЃСЂРµР·).
        if depth < MAX_FRAME_DEPTH {
            let nested = load_frame_sub_documents(
                    &child_doc_arc,
                    depth + 1,
                    &child_base,
                    top_doc,
                    top_base,
                    media_ctx,
                    viewport,
                    sink,
                    cookie_jar.clone(),
                    fetch_provider.clone(),
                    ws_provider.clone(),
                    sse_provider.clone(),
                    ls_store.clone().filter(|_| !opaque),
                    ss_store.clone().filter(|_| !opaque),
                    idb_backend.clone().filter(|_| !opaque),
                    sw_backend.clone().filter(|_| !opaque),
                    sw_worker_store.clone().filter(|_| !opaque),
                    cache_backend.clone().filter(|_| !opaque),
                    cookie_banner_dismiss,
                    deterministic,
                    cross_origin_isolated,
                    child_js.as_ref(),
                    target,
                );
            handles.extend(nested);
        }
        // BUG-480 СЃСЂРµР· 2: Р±РёРЅРґРёРЅРі В«С…РѕСЃС‚ в†’ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚В» РґР»СЏ contentWindow/
        // contentDocument СЂРѕРґРёС‚РµР»СЏ вЂ” СЃС‚СЂРѕРіРѕ РґРѕ trusted `load` РЅР° С…РѕСЃС‚Рµ,
        // С‡С‚РѕР±С‹ РѕР±СЂР°Р±РѕС‚С‡РёРєРё С‡РёС‚Р°Р»Рё С„Р°СЃР°РґС‹ СЃСЂР°Р·Сѓ РёР· РѕР±СЂР°Р±РѕС‚С‡РёРєР°. РЎСЂРµР· 3:
        // РёРјСЏ С…РѕСЃС‚Р° РµРґРµС‚ РІРјРµСЃС‚Рµ СЃ Р±РёРЅРґРёРЅРіРѕРј (РєР»СЋС‡ window[name]).
        if let Some(js) = parent_js {
            let accessible = frame_access_allowed(base, &child_url, opaque);
            js.register_iframe_document(
                info.node.index() as u32,
                Arc::clone(&child_doc_arc),
                &child_url,
                info.name.as_deref(),
                accessible,
            );
        }
        fire_iframe_load_event(parent_js, info.node);
        handles.push(FrameHandle {
            host: info.node,
            url: child_url,
            doc: Arc::clone(&child_doc_arc),
            js: child_js,
            depth,
            sheet: frame_sheet,
            viewport: FRAME_UA_DEFAULT_SIZE,
            parent_doc: (depth > 0).then(|| Arc::clone(parent)),
            layout: frame_layout,
            content_dl: DisplayList::new(),
            host_rect: None,
            host_src: info.src.clone().unwrap_or_default(),
            images: subresources.decoded_images,
            image_keys: subresources.image_keys,
            scroll_y: 0.0,
        });
    }
    handles
}

/// Р–РёРІРѕР№ sub-РґРѕРєСѓРјРµРЅС‚ РѕРґРЅРѕРіРѕ `<iframe>` (BUG-480, СЃСЂРµР· 1).
///
/// Р”РµСЂР¶РёС‚ РїРѕСЂРѕР¶РґС‘РЅРЅС‹Р№ `Document` Рё РµРіРѕ JS-РєРѕРЅС‚РµРєСЃС‚ Р¶РёРІС‹РјРё РЅР° РІСЂРµРјСЏ Р¶РёР·РЅРё
/// СЃС‚СЂР°РЅРёС†С‹: РїРѕРєР° С…СЌРЅРґР» Р¶РёРІ, С‚РёРєР°СЋС‚ С‚Р°Р№РјРµСЂС‹ СЂРµР±С‘РЅРєР° Рё СЂР°Р±РѕС‚Р°СЋС‚ РµРіРѕ
/// РѕР±СЂР°Р±РѕС‚С‡РёРєРё. РџР°РґР°РµС‚ РІРјРµСЃС‚Рµ СЃРѕ СЃС‚СЂР°РЅРёС†РµР№ вЂ” Р·Р°РјРµРЅР° СЃС‚СЂР°РЅРёС†С‹ РІ
/// [`Lumen::apply_loaded_page`] СѓРЅРѕСЃРёС‚ РІСЃРµ С„СЂРµР№РјС‹ СЂР°Р·РѕРј, РѕС‚РґРµР»СЊРЅРѕРіРѕ
/// lifecycle-РјРµРЅРµРґР¶РјРµРЅС‚Р° РЅРµ РЅСѓР¶РЅРѕ.
///
/// РЎСЂРµР· 2 РґР°Р» JS СЂРѕРґРёС‚РµР»СЏ С„Р°СЃР°РґС‹ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р° С‡РµСЂРµР· СЂРµРµСЃС‚СЂ Р±РёРЅРґРёРЅРіРѕРІ
/// `frame_bridge.rs` вЂ” СЂРµРіРёСЃС‚СЂР°С†РёСЏ РёРґС‘С‚ РёР· Р»РѕРєР°Р»СЊРЅС‹С… РїРµСЂРµРјРµРЅРЅС‹С… СЌС‚РѕР№ С„СѓРЅРєС†РёРё,
/// РїРѕСЌС‚РѕРјСѓ РїРѕР»СЏ С…СЌРЅРґР»Р° РїРѕ-РїСЂРµР¶РЅРµРјСѓ РЅРµ С‡РёС‚Р°СЋС‚СЃСЏ; С‡РёС‚Р°С‚СЊСЃСЏ РЅР°С‡РЅСѓС‚ СЃРѕ СЃСЂРµР·РѕРј
/// РЅР°РІРёРіР°С†РёРё/Р·Р°РјРµРЅС‹ С„СЂРµР№РјР°.
#[allow(dead_code)] // url вЂ” РґРѕ СЃСЂРµР·Р° РЅР°РІРёРіР°С†РёРё/Р·Р°РјРµРЅС‹ С„СЂРµР№РјР° (СЃРј. bugs/BUG-480-OPEN.md)
pub(crate) struct FrameHandle {
    /// `NodeId` `<iframe>`-СЌР»РµРјРµРЅС‚Р° РІ РґРѕРєСѓРјРµРЅС‚Рµ-СЂРѕРґРёС‚РµР»Рµ.
    pub(crate) host: NodeId,
    /// РђРґСЂРµСЃ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р°: СЂР°Р·СЂРµС€С‘РЅРЅС‹Р№ URL, РїСѓС‚СЊ С„Р°Р№Р»Р° РёР»Рё `about:blank` /
    /// `about:srcdoc`. Р”РёР°РіРЅРѕСЃС‚РёРєР° Рё Р±СѓРґСѓС‰Р°СЏ РЅР°РІРёРіР°С†РёСЏ С„СЂРµР№РјР°.
    pub(crate) url: String,
    /// РџРѕРґ-РґРѕРєСѓРјРµРЅС‚. РћС‚РґРµР»СЊРЅС‹Р№ `Arc` вЂ” JS-Р·Р°РјС‹РєР°РЅРёСЏ СЂРµР±С‘РЅРєР° РґРµСЂР¶Р°С‚ РµРіРѕ Р¶Рµ.
    pub(crate) doc: Arc<Mutex<Document>>,
    /// JS-РєРѕРЅС‚РµРєСЃС‚ СЂРµР±С‘РЅРєР° (`None` вЂ” Сѓ С„СЂРµР№РјР° РЅРµ Р±С‹Р»Рѕ СЃРєСЂРёРїС‚РѕРІ РёР»Рё v8 РІС‹РєР»СЋС‡РµРЅ).
    pub(crate) js: Option<Arc<dyn PersistentJs>>,
    /// Глубина вложенности: 0 — фрейм страницы, 1 — фрейм внутри фрейма.
    ///
    /// Задаёт ПОРЯДОК обоих проходов [`sync_frame_viewports`]: host-бокс фрейма
    /// глубины `d` ищется в layout фрейма глубины `d-1` (`NodeId` уникален лишь
    /// внутри своего документа), поэтому layout считается по возрастанию
    /// глубины, а display list — по убыванию.
    pub(crate) depth: usize,
    /// Разобранный каскад под-документа (BUG-480 срез 12 собирает его текст,
    /// срез 13 пересчитывает по нему layout при каждой смене размера хоста).
    pub(crate) sheet: lumen_css_parser::Stylesheet,
    /// Вьюпорт последнего посчитанного layout ребёнка: сначала
    /// [`FRAME_UA_DEFAULT_SIZE`], затем контентный бокс хоста. Служит гейтом
    /// «размер не менялся — не пересчитывать» в [`sync_frame_viewports`].
    pub(crate) viewport: lumen_core::geom::Size,
    /// Документ, в дереве которого лежит host-элемент: `None` — страница,
    /// `Some` — под-документ фрейма-родителя (BUG-480 срез 14).
    ///
    /// Родитель адресуется именно `Arc`-ом, а не индексом в списке: список
    /// плоский, вложенные хэндлы попадают в него раньше своего родителя, а
    /// `NodeId` хоста уникален лишь внутри своего документа — сравнение
    /// `Arc::ptr_eq` единственное, что здесь ничего не путает.
    pub(crate) parent_doc: Option<Arc<Mutex<Document>>>,
    /// Layout под-документа на текущем [`Self::viewport`] (BUG-480 срез 14).
    ///
    /// Хранится по двум причинам: по нему рисуется [`Self::content_dl`], и в
    /// нём ищется host-бокс ВЛОЖЕННОГО фрейма — в layout страницы его нет.
    pub(crate) layout: Option<lumen_layout::LayoutBox>,
    /// Display list под-документа в его собственных координатах, с уже
    /// вклеенным содержимым его вложенных фреймов (BUG-480 срез 14).
    ///
    /// Пуст, пока layout не посчитан: тогда на экране остаётся серая заглушка.
    pub(crate) content_dl: DisplayList,
    /// Контентный бокс host-элемента в координатах ЕГО документа — куда
    /// вклеивается [`Self::content_dl`] (BUG-480 срез 14).
    pub(crate) host_rect: Option<Rect>,
    /// Значение атрибута `src` host-элемента — половина ключа, по которому
    /// [`splice_one_frame`] узнаёт команду-заглушку в display list родителя.
    pub(crate) host_src: String,
    /// Декодированные картинки под-документа (BUG-480 срез 15).
    ///
    /// Едут в `LoadedPage::images` страницы: регистрация в рендерере (и в
    /// CPU-кэше снимков) идёт единым списком, поэтому ни одной новой точки
    /// регистрации срез не заводит — все существующие подхватывают их сами.
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// `(сырой src, ключ регистрации)` картинок под-документа — карта для
    /// [`rekey_frame_images`] (BUG-480 срез 15).
    pub(crate) image_keys: Vec<(String, String)>,
    /// Прокрутка под-документа по вертикали, CSS px (BUG-480 срез 17).
    ///
    /// Читают три разных места, и все три обязаны читать ОДНО поле, иначе
    /// пиксели, hit-тест и `window.scrollY` ребёнка разойдутся:
    /// [`splice_one_frame`] сдвигает содержимое, [`pointer_target`] — точку
    /// спуска, а шелл — позицию в JS-контексте ребёнка.
    ///
    /// Горизонтали нет: у под-документа нет ни своей полосы прокрутки, ни
    /// `window.scrollX` (у страницы он тоже захардкожен в 0), а колесо вбок
    /// над фреймом уходит странице.
    pub(crate) scroll_y: f32,
}

// ── скролл под-документа (BUG-480 срез 17) ──────────────────────────────────

/// Предел прокрутки под-документа: насколько его содержимое выше вьюпорта.
///
/// Высота берётся из ГОТОВОГО display list ребёнка — тем же правилом, что и у
/// страницы ([`content_height_of`]), а не из layout-дерева: у страницы
/// «прокручивается ровно то, что нарисовано» (пустой распорка без фона не даёт
/// прокрутки — известная ловушка, см. CLAUDE.md), и разойтись этим двум
/// ответам внутри одного движка нельзя.
pub(crate) fn frame_max_scroll(h: &FrameHandle) -> f32 {
    if h.content_dl.is_empty() {
        return 0.0;
    }
    (crate::display_list_metrics::content_height_of(&h.content_dl) - h.viewport.height).max(0.0)
}

/// Прокрутить под-документ фрейма `idx` в АБСОЛЮТНУЮ позицию `y` (с зажимом).
///
/// Возвращает новую позицию, если она действительно изменилась, и `None`
/// иначе — вызывающая сторона по этому ответу решает две разные вещи: слать ли
/// ребёнку `scroll`/`scrollend` (CSSOM-View §14 — событие принадлежит движению,
/// а не колесу) и продолжать ли цепочку прокрутки выше по CSS Overscroll
/// Behavior L1 §3, как это уже делают overflow-контейнеры страницы.
pub(crate) fn scroll_frame_to(frames: &mut [FrameHandle], idx: usize, y: f32) -> Option<f32> {
    let max = frame_max_scroll(&frames[idx]);
    let clamped = y.clamp(0.0, max);
    if (clamped - frames[idx].scroll_y).abs() <= f32::EPSILON {
        return None;
    }
    frames[idx].scroll_y = clamped;
    Some(clamped)
}

/// РњР°РєСЃРёРјР°Р»СЊРЅР°СЏ РіР»СѓР±РёРЅР° РІР»РѕР¶РµРЅРЅРѕСЃС‚Рё С„СЂРµР№РјРѕРІ: СЃС‚СЂР°РЅРёС†Р° (0) в†’ iframe (1) в†’
/// iframe РІ iframe (2) в†’ РіР»СѓР±Р¶Рµ РЅРµ Р·Р°РіСЂСѓР¶Р°РµРј. Р—Р°С‰РёС‚Р° РѕС‚ СЂРµРєСѓСЂСЃРёРІРЅС‹С…
/// СЃР°РјРѕРІР»РѕР¶РµРЅРёР№ РІ РЅРµРґРѕРІРµСЂРµРЅРЅРѕРј HTML; СЃРїРµРєР° РіР»СѓР±РёРЅСѓ РЅРµ РѕРіСЂР°РЅРёС‡РёРІР°РµС‚.
pub(crate) const MAX_FRAME_DEPTH: usize = 2;

/// UA-дефолт intrinsic-размера `<iframe>` (HTML LS §4.8.5): 300×150 CSS px —
/// см. `iframe_ua_default_size_300_by_150` в `lumen-layout`. BUG-480 срез 12
/// использует его как вьюпорт для ПЕРВОГО layout ребёнка: реальный размер
/// host-бокса ещё не известен в момент вызова (`load_frame_sub_documents` идёт
/// ДО layout страницы-родителя), а собственные скрипты ребёнка и его
/// DOMContentLoaded/load исполняются уже здесь и обязаны видеть какую-то
/// geometry. Срез 13 уточняет её до контентного бокса хоста сразу после layout
/// родителя ([`sync_frame_viewports`]).
const FRAME_UA_DEFAULT_SIZE: lumen_core::geom::Size = lumen_core::geom::Size::new(300.0, 150.0);
