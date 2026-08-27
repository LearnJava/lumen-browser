//! The document's external CSS: which `<link rel=stylesheet>` applies at
//! all, fetching the ones that do, and flattening their `@import` chains.
//!
//! The media gate and the two [`lumen_css_parser::MediaContext`] builders live
//! here rather than next to the cascade because they answer a question about
//! the `<link>` element, not about a rule: `collect_link_hrefs` drops a sheet
//! whose `media` does not match before anything is fetched, and the print
//! pipeline swaps in `print_media_context` to make the same gate answer
//! differently (BUG-268 / BUG-270).
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// BUG-268: media-РіРµР№С‚ РґР»СЏ `<link rel=stylesheet media=...>` (HTML LS В§4.2.4).
///
/// РћС‚СЃСѓС‚СЃС‚РІСѓСЋС‰РёР№/РїСѓСЃС‚РѕР№ Р°С‚СЂРёР±СѓС‚ = В«allВ» в†’ Р»РёСЃС‚ РїСЂРёРјРµРЅСЏРµС‚СЃСЏ. РРЅР°С‡Рµ СЃС‚СЂРѕРєР°
/// РїР°СЂСЃРёС‚СЃСЏ С€С‚Р°С‚РЅС‹Рј media-query-РїР°СЂСЃРµСЂРѕРј lumen-css-parser Рё РјР°С‚С‡РёС‚СЃСЏ РїСЂРѕС‚РёРІ
/// РїРµСЂРµРґР°РЅРЅРѕРіРѕ РєРѕРЅС‚РµРєСЃС‚Р° вЂ” РІС‚РѕСЂРѕР№ РјР°С‚С‡РµСЂ РЅРµ РїРёС€РµРј. `ctx` РїРµСЂРµРґР°С‘С‚СЃСЏ
/// РїР°СЂР°РјРµС‚СЂРѕРј (Р° РЅРµ С…Р°СЂРґРєРѕРґРёС‚СЃСЏ В«screenВ»), С‡С‚РѕР±С‹ print-РїР°Р№РїР»Р°Р№РЅ РјРѕРі
/// РёСЃРїРѕР»СЊР·РѕРІР°С‚СЊ С‚РѕС‚ Р¶Рµ РіРµР№С‚ СЃ `media_type: "print"`, РєРѕРіРґР° РєР°СЃРєР°Рґ РЅР°СѓС‡РёС‚СЃСЏ
/// print-РєРѕРЅС‚РµРєСЃС‚Сѓ (СЃРј. BUGS.md BUG-270).
pub(crate) fn link_media_matches(media: &str, ctx: &lumen_css_parser::MediaContext) -> bool {
    let media = media.trim();
    if media.is_empty() {
        return true;
    }
    lumen_css_parser::parse_media_query(media).matches(ctx)
}

/// Р­РєСЂР°РЅРЅС‹Р№ `MediaContext` РґР»СЏ media-РіРµР№С‚Р° `<link>`: С‚Рµ Р¶Рµ media_type /
/// СЂР°Р·РјРµСЂС‹ / prefers-color-scheme, С‡С‚Рѕ РєР°СЃРєР°Рґ СЃС‚СЂРѕРёС‚ РІРЅСѓС‚СЂРё layout
/// (`media_context_from_viewport`, layout/src/style.rs) вЂ” РіРµР№С‚ РЅР° `<link>`
/// Рё С„РёР»СЊС‚СЂ `@media`-Р±Р»РѕРєРѕРІ РґРѕР»Р¶РЅС‹ СЂРµС€Р°С‚СЊ РѕРґРёРЅР°РєРѕРІРѕ.
pub(crate) fn screen_media_context(viewport: Size, dark_mode: bool) -> lumen_css_parser::MediaContext {
    lumen_css_parser::MediaContext {
        media_type: "screen".into(),
        width: viewport.width,
        height: viewport.height,
        prefers_dark: dark_mode,
        ..Default::default()
    }
}

/// Print `MediaContext` РґР»СЏ media-РіРµР№С‚Р° `<link>` РїСЂРё РіРµРЅРµСЂР°С†РёРё PDF (BUG-270):
/// `media_type: "print"`, С‡С‚РѕР±С‹ `<link rel=stylesheet media=print>` РїРѕРїР°РґР°Р»Рё РІ
/// РєР°СЃРєР°Рґ, Р° `media=screen` вЂ” РЅРµС‚. РљР°СЃРєР°РґРЅС‹Р№ С„РёР»СЊС‚СЂ `@media` РІРЅСѓС‚СЂРё layout
/// СЂРµС€Р°РµС‚ С‚Р°Рє Р¶Рµ С‡РµСЂРµР· `set_print_media` в†’ `media_context_from_viewport`.
pub(crate) fn print_media_context(viewport: Size, dark_mode: bool) -> lumen_css_parser::MediaContext {
    lumen_css_parser::MediaContext {
        media_type: "print".into(),
        width: viewport.width,
        height: viewport.height,
        prefers_dark: dark_mode,
        ..Default::default()
    }
}

/// Р—Р°РіСЂСѓР·РёС‚СЊ РІСЃРµ `<link rel=stylesheet>` РґРѕРєСѓРјРµРЅС‚Р° Рё СЃРєР»РµРёС‚СЊ РёС… С‚РµРєСЃС‚.
///
/// Р’С‚РѕСЂРѕР№ СЌР»РµРјРµРЅС‚ СЂРµР·СѓР»СЊС‚Р°С‚Р° вЂ” РёСЃС…РѕРґ РїРѕ РєР°Р¶РґРѕРјСѓ СЌР»РµРјРµРЅС‚Сѓ (`СѓР·РµР»`, `РїРѕР»СѓС‡РµРЅ
/// Р»Рё Р»РёСЃС‚`) РІ РїРѕСЂСЏРґРєРµ РѕР±СЉСЏРІР»РµРЅРёСЏ, РґР»СЏ BUG-804: `load`/`error` РїСЂРёРЅР°РґР»РµР¶Р°С‚
/// СЌР»РµРјРµРЅС‚Сѓ `<link>`, Р° Р·РЅР°РµС‚ РёСЃС…РѕРґ С‚РѕР»СЊРєРѕ СЌС‚РѕС‚ РїСЂРѕС…РѕРґ. Р Р°РЅСЊС€Рµ РїСЂРѕРІР°Р» РїСЂРѕСЃС‚Рѕ
/// Р»РѕРіРёСЂРѕРІР°Р»СЃСЏ, Рё СЃС‚СЂР°РЅРёС†Р° РЅРµ РјРѕРіР»Р° РѕС‚Р»РёС‡РёС‚СЊ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ Р»РёСЃС‚ РѕС‚ 404.
pub(crate) fn load_linked_stylesheets(doc: &Document, base: &ResourceBase, sink: &Arc<dyn EventSink>, cookie_jar: Option<Arc<lumen_storage::CookieJar>>, media_ctx: &lumen_css_parser::MediaContext) -> (String, Vec<(NodeId, bool)>) {
    let mut hrefs = Vec::new();
    collect_link_hrefs(doc, doc.root(), &mut hrefs, media_ctx);

    // Р—Р°РіСЂСѓР¶Р°РµРј РІСЃРµ С‚Р°Р±Р»РёС†С‹ РїР°СЂР°Р»Р»РµР»СЊРЅРѕ (СЃРµС‚СЊ вЂ” РіР»Р°РІРЅС‹Р№ С‚РѕСЂРјРѕР·), Р·Р°С‚РµРј
    // РєРѕРЅРєР°С‚РµРЅРёСЂСѓРµРј СЃС‚СЂРѕРіРѕ РІ РїРѕСЂСЏРґРєРµ РѕР±СЉСЏРІР»РµРЅРёСЏ, С‡С‚РѕР±С‹ РєР°СЃРєР°Рґ РЅРµ РЅР°СЂСѓС€РёР»СЃСЏ.
    // РљР°Р¶РґС‹Р№ Р»РёСЃС‚ СЂРµР·РѕР»РІРёС‚ СЃРѕР±СЃС‚РІРµРЅРЅС‹Рµ `@import` РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ РЎР’РћР•Р“Рћ URL
    // (`sheet_base`), С‡С‚РѕР±С‹ РІР»РѕР¶РµРЅРЅС‹Рµ РёРјРїРѕСЂС‚С‹ (`<link href="/css/a.css">` в†’
    // `@import "b.css"` = `/css/b.css`) СЂР°Р·СЂРµС€Р°Р»РёСЃСЊ РєРѕСЂСЂРµРєС‚РЅРѕ.
    let parts = parallel_map(&hrefs, |_, (_, href)| {
        let (text, sheet_base) = fetch_stylesheet_text(href, base, sink, cookie_jar.clone())?;
        Some(inline_css_imports(
            &text,
            &sheet_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            &mut std::collections::HashSet::new(),
            0,
        ))
    });

    let mut css = String::new();
    let mut outcomes = Vec::with_capacity(parts.len());
    for ((node, _), part) in hrefs.iter().zip(parts) {
        outcomes.push((*node, part.is_some()));
        if let Some(part) = part {
            css.push_str(&part);
            css.push('\n');
        }
    }
    (css, outcomes)
}

/// Р—Р°РіСЂСѓР¶Р°РµС‚ С‚РµРєСЃС‚ РѕРґРЅРѕР№ С‚Р°Р±Р»РёС†С‹ СЃС‚РёР»РµР№, СЂР°Р·СЂРµС€С‘РЅРЅРѕР№ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ `base`.
///
/// РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚ Р»РѕРєР°Р»СЊРЅС‹Рµ РїСѓС‚Рё (`file://`/РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹Рµ вЂ” С‡РёС‚Р°СЋС‚СЃСЏ СЃ РґРёСЃРєР°)
/// Рё `http(s)` (С‡РµСЂРµР· prefetch-РєСЌС€, РєР°Рє `<link rel=stylesheet>`). Р’РѕР·РІСЂР°С‰Р°РµС‚
/// С‚РµРєСЃС‚ Р»РёСЃС‚Р° **Рё** РµРіРѕ СЂР°Р·СЂРµС€С‘РЅРЅС‹Р№ [`ResourceBase`], С‡С‚РѕР±С‹ РІР»РѕР¶РµРЅРЅС‹Рµ
/// `@import` СЂРµР·РѕР»РІРёР»РёСЃСЊ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ СЃРѕР±СЃС‚РІРµРЅРЅРѕРіРѕ URL Р»РёСЃС‚Р°, Р° РЅРµ РґРѕРєСѓРјРµРЅС‚Р°.
/// РџСЂРё Р»СЋР±РѕР№ РѕС€РёР±РєРµ resolve/С‡С‚РµРЅРёСЏ/СЃРµС‚Рё вЂ” `None` (Р·Р°Р»РѕРіРёСЂРѕРІР°РЅРѕ), РїРѕСЌС‚РѕРјСѓ РѕРґРёРЅ
/// Р±РёС‚С‹Р№ `@import`/`<link>` РЅРµ РІР°Р»РёС‚ РІРµСЃСЊ СЂРµРЅРґРµСЂ.
fn fetch_stylesheet_text(
    href: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<(String, ResourceBase)> {
    match base.resolve(href) {
        ResolvedResource::File(path) => match std::fs::read_to_string(&path) {
            Ok(content) => {
                eprintln!("Р—Р°РіСЂСѓР¶РµРЅ CSS: {}", path.display());
                Some((content, ResourceBase::File(path)))
            }
            Err(e) => {
                eprintln!("РџСЂРѕРїСѓСЃРє CSS {}: {e}", path.display());
                None
            }
        },
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            let sub_url = match Url::parse(&url) {
                Ok(u) => u,
                Err(e) => { eprintln!("РџСЂРѕРїСѓСЃРє CSS {url}: {e}"); return None; }
            };

            // Cross-origin stylesheets are allowed by the web platform:
            // `<link rel=stylesheet>` is fetched in no-cors mode and the
            // resulting styles apply normally (Fetch В§request, HTML В§link).
            // CORS only gates script-level CSSOM reads (cssRules), not the
            // visual application вЂ” so we fetch cross-origin CSS like any
            // browser. Real sites host CSS on CDN subdomains (icdn.*,
            // static.*); blocking them left pages unstyled.

            // BUG-171: read through the prefetch cache вЂ” the streaming thread
            // warms linked stylesheets with this same client, so the cascade
            // concatenation here reuses identical bytes without a second fetch.
            // PERF-1: one span per stylesheet fetch.
            let mut fetch_span = lumen_core::trace::span(format!("css {url}"), "net");
            let bytes = crate::prefetch::PREFETCH_CACHE.fetch_current(&url, || {
                let client = base.http_client_for_subresource(sink.clone(), cookie_jar.clone());
                client
                    .fetch_subresource(&sub_url, RequestDestination::Style)
                    .map_err(|e| e.to_string())
            });
            match bytes {
                Ok(bytes) => {
                    fetch_span.set_bytes(bytes.len());
                    Some((
                        String::from_utf8_lossy(&bytes[..]).into_owned(),
                        ResourceBase::Url(url),
                    ))
                }
                Err(e) => { eprintln!("РџСЂРѕРїСѓСЃРє CSS {url}: {e}"); None }
            }
        }
    }
}

/// РњР°РєСЃРёРјР°Р»СЊРЅР°СЏ РіР»СѓР±РёРЅР° РІР»РѕР¶РµРЅРЅРѕСЃС‚Рё `@import` (Р·Р°С‰РёС‚Р° РѕС‚ СЂРµРєСѓСЂСЃРёРё/С†РёРєР»РѕРІ).
const MAX_CSS_IMPORT_DEPTH: u32 = 16;

/// Р РµРєСѓСЂСЃРёРІРЅРѕ СЂРµР·РѕР»РІРёС‚ `@import`-РїСЂР°РІРёР»Р° РІ `css_text`, РІРѕР·РІСЂР°С‰Р°СЏ С‚РµРєСЃС‚ СЃ
/// **РїСЂРµРґРїРѕСЃР»Р°РЅРЅС‹Рј** СЃРѕРґРµСЂР¶РёРјС‹Рј РєР°Р¶РґРѕР№ РёРјРїРѕСЂС‚РёСЂРѕРІР°РЅРЅРѕР№ С‚Р°Р±Р»РёС†С‹.
///
/// Per CSS Cascade L4 В§6.5: РїСЂР°РІРёР»Р° РёРјРїРѕСЂС‚РёСЂРѕРІР°РЅРЅРѕРіРѕ Р»РёСЃС‚Р° РїСЂРµРґС€РµСЃС‚РІСѓСЋС‚
/// СЃРѕР±СЃС‚РІРµРЅРЅС‹Рј РїСЂР°РІРёР»Р°Рј РёРјРїРѕСЂС‚РёСЂСѓСЋС‰РµРіРѕ Р»РёСЃС‚Р° (РёРјРїРѕСЂС‚ В«СЂР°РЅСЊС€РµВ» РІ РїРѕСЂСЏРґРєРµ
/// РєР°СЃРєР°РґР°). URL СЂРµР·РѕР»РІСЏС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ `base` (СЂР°СЃРїРѕР»РѕР¶РµРЅРёСЏ СЃР°РјРѕРіРѕ Р»РёСЃС‚Р° вЂ”
/// СЃРј. [`fetch_stylesheet_text`]), РїРѕСЌС‚РѕРјСѓ РІР»РѕР¶РµРЅРЅС‹Рµ РёРјРїРѕСЂС‚С‹ РєРѕСЂСЂРµРєС‚РЅС‹.
/// РРјРїРѕСЂС‚С‹, С‡РµР№ media-query РЅРµ РјР°С‚С‡РёС‚ `media_ctx` (Media Queries L4), РЅРµ
/// Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ РІРѕРІСЃРµ вЂ” РёС… РїСЂР°РІРёР»Р° РІСЃС‘ СЂР°РІРЅРѕ РЅРµРїСЂРёРјРµРЅРёРјС‹. `seen` С…СЂР°РЅРёС‚ СѓР¶Рµ
/// СЂР°Р·СЂРµС€С‘РЅРЅС‹Рµ URL Рё Р·Р°С‰РёС‰Р°РµС‚ РѕС‚ С†РёРєР»РѕРІ (`a в†’ b в†’ a`) Рё РїРѕРІС‚РѕСЂРЅРѕР№ Р·Р°РіСЂСѓР·РєРё;
/// `depth` РѕРіСЂР°РЅРёС‡РёРІР°РµС‚ РіР»СѓР±РёРЅСѓ РІР»РѕР¶РµРЅРЅРѕСЃС‚Рё.
///
/// Р”РёСЂРµРєС‚РёРІС‹ `@import вЂ¦;` РѕСЃС‚Р°СЋС‚СЃСЏ РІ РёСЃС…РѕРґРЅРѕРј С‚РµРєСЃС‚Рµ вЂ” РїР°СЂСЃРµСЂ РєР°СЃРєР°РґР°
/// СЃРѕР±РёСЂР°РµС‚ РёС… РІ `Stylesheet::imports` Рё РёРіРЅРѕСЂРёСЂСѓРµС‚ (РїРѕРІС‚РѕСЂРЅРѕР№ Р·Р°РіСЂСѓР·РєРё РЅРµС‚),
/// С‚Р°Рє С‡С‚Рѕ РґРІРѕР№РЅРѕРіРѕ РїСЂРёРјРµРЅРµРЅРёСЏ РЅРµ РїСЂРѕРёСЃС…РѕРґРёС‚.
pub(crate) fn inline_css_imports(
    css_text: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
    seen: &mut std::collections::HashSet<String>,
    depth: u32,
) -> String {
    // Р‘С‹СЃС‚СЂС‹Р№ РїСѓС‚СЊ: РЅРµС‚ С‚РѕРєРµРЅР° `@import` РІРѕРІСЃРµ в†’ Р»РёС€РЅРёР№ РїР°СЂСЃ РЅРµ РЅСѓР¶РµРЅ
    // (РїРѕРґР°РІР»СЏСЋС‰РµРµ Р±РѕР»СЊС€РёРЅСЃС‚РІРѕ Р»РёСЃС‚РѕРІ). Р›РѕР¶РЅС‹Рµ СЃСЂР°Р±Р°С‚С‹РІР°РЅРёСЏ (РЅР°РїСЂРёРјРµСЂ
    // `@import` РІРЅСѓС‚СЂРё РєРѕРјРјРµРЅС‚Р°СЂРёСЏ) Р±РµР·РѕРїР°СЃРЅС‹ вЂ” РїРѕСЃР»РµРґСѓСЋС‰РёР№ РїР°СЂСЃ РїСЂР°РІРёР»СЊРЅРѕ
    // РЅРµ РЅР°Р№РґС‘С‚ РёРјРїРѕСЂС‚Р° Рё РІРµСЂРЅС‘С‚ С‚РµРєСЃС‚ РєР°Рє РµСЃС‚СЊ.
    if !contains_ignore_ascii_case(css_text.as_bytes(), b"@import") {
        return css_text.to_owned();
    }
    let parsed = lumen_css_parser::parse(css_text);
    if parsed.imports.is_empty() {
        return css_text.to_owned();
    }
    if depth >= MAX_CSS_IMPORT_DEPTH {
        eprintln!("РџСЂРѕРїСѓСЃРє @import: РїСЂРµРІС‹С€РµРЅР° РіР»СѓР±РёРЅР° РІР»РѕР¶РµРЅРЅРѕСЃС‚Рё ({MAX_CSS_IMPORT_DEPTH})");
        return css_text.to_owned();
    }

    let mut prefix = String::new();
    for imp in &parsed.imports {
        // Media Queries L4: РЅРµ РјР°С‚С‡Р°С‰РёР№ РєРѕРЅС‚РµРєСЃС‚ РёРјРїРѕСЂС‚ РЅРµ РїСЂРёРјРµРЅСЏРµС‚СЃСЏ.
        if !imp.media.matches(media_ctx) {
            continue;
        }
        // Р¦РёРєР»/РґСѓР±Р»РёРєР°С‚: РєР»СЋС‡ = Р°Р±СЃРѕР»СЋС‚РЅС‹Р№ СЂРµР·РѕР»РІ URL РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ С‚РµРєСѓС‰РµРіРѕ Р»РёСЃС‚Р°.
        let key = base.resolve_str(&imp.url);
        if !seen.insert(key) {
            continue;
        }
        let Some((text, imp_base)) =
            fetch_stylesheet_text(&imp.url, base, sink, cookie_jar.clone())
        else {
            continue;
        };
        let resolved = inline_css_imports(
            &text,
            &imp_base,
            sink,
            cookie_jar.clone(),
            media_ctx,
            seen,
            depth + 1,
        );
        prefix.push_str(&resolved);
        if !prefix.ends_with('\n') {
            prefix.push('\n');
        }
    }

    if prefix.is_empty() {
        return css_text.to_owned();
    }
    prefix.push_str(css_text);
    prefix
}

/// ASCII-case-insensitive РїРѕРёСЃРє РїРѕРґСЃС‚СЂРѕРєРё `needle` РІ `haystack` Р±РµР· Р°Р»Р»РѕРєР°С†РёР№.
pub(crate) fn contains_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return needle.is_empty();
    }
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// РЎРѕР±СЂР°С‚СЊ `(СѓР·РµР», href)` РєР°Р¶РґРѕРіРѕ `<link rel=stylesheet>`, РєРѕС‚РѕСЂС‹Р№ РїРѕРїР°РґС‘С‚ РІ
/// РєР°СЃРєР°Рґ.
///
/// РЈР·РµР» РЅСѓР¶РµРЅ BUG-804: РїРѕ РЅРµРјСѓ [`load_linked_stylesheets`] РїРѕС‚РѕРј СЃРѕРѕР±С‰Р°РµС‚
/// JS-СЃС‚РѕСЂРѕРЅРµ РёСЃС…РѕРґ Р·Р°РіСЂСѓР·РєРё, С‡С‚РѕР±С‹ СЌР»РµРјРµРЅС‚ РІС‹СЃС‚СЂРµР»РёР» `load`/`error`. Р Р°РЅСЊС€Рµ
/// СЃРѕР±РёСЂР°Р»РёСЃСЊ РѕРґРЅРё Р°РґСЂРµСЃР°, Рё СЃРІСЏР·Рё В«СЌС‚РѕС‚ Р»РёСЃС‚ вЂ” СЌС‚РѕС‚ СЌР»РµРјРµРЅС‚В» РЅРµ СЃСѓС‰РµСЃС‚РІРѕРІР°Р»Рѕ.
pub(crate) fn collect_link_hrefs(doc: &Document, id: NodeId, out: &mut Vec<(NodeId, String)>, media_ctx: &lumen_css_parser::MediaContext) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "link"
    {
        let rel = attrs
            .iter()
            .find(|a| a.name.local == "rel")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        let href = attrs
            .iter()
            .find(|a| a.name.local == "href")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        // BUG-268: print-only (Рё РІРѕРѕР±С‰Рµ РЅРµ РјР°С‚С‡Р°С‰РёРµ РєРѕРЅС‚РµРєСЃС‚) Р»РёСЃС‚С‹ РЅРµ
        // РІР»РёРІР°СЋС‚СЃСЏ РІ РєР°СЃРєР°Рґ вЂ” РёС… РїСЂР°РІРёР»Р° РЅРµ РѕР±С‘СЂРЅСѓС‚С‹ РІ `@media`, РєР°СЃРєР°Рґ
        // СЃР°Рј РёС… РЅРµ РѕС‚С„РёР»СЊС‚СЂСѓРµС‚.
        let media = attrs
            .iter()
            .find(|a| a.name.local == "media")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if rel.split_ascii_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"))
            && !href.is_empty()
            && link_media_matches(media, media_ctx)
        {
            out.push((id, href.to_owned()));
        }
        return;
    }
    for &child in &node.children {
        collect_link_hrefs(doc, child, out, media_ctx);
    }
}
