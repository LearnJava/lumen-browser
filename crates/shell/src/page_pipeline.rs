//! The page-load pipeline's entry point: [`render_bytes`] turns fetched bytes
//! into a laid-out, painted page, and [`dispatch_preload_hints`] emits the
//! preload-scanner hints the parser found along the way.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn render_bytes(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    hp: &dyn HyphenationProvider,
    cookie_banner_dismiss: bool,
    deterministic: deterministic::DetConfig,
    dark_mode: bool,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    cross_origin_isolated: bool,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    target: lumen_core::ColorSpace,
    cache_control_no_store: bool,
) -> Result<RenderedPage, Box<dyn Error>> {
    let parsed = parse_and_layout(bytes, content_type, base, &sink, viewport, preload_seen, ls_store, ss_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic, dark_mode, cookie_jar, cross_origin_isolated, sw_worker_store, cache_backend, target, false)?;
    let display_list = paint_ordered(&parsed.layout);
    println!(
        "Р Р°СЃРїР°СЂСЃРµРЅРѕ: {} DOM-СѓР·Р»РѕРІ, {} CSS-РїСЂР°РІРёР», {} paint-РєРѕРјР°РЅРґ, {} РєР°СЂС‚РёРЅРѕРє, {} preload-С…РёРЅС‚РѕРІ",
        parsed.document.lock().unwrap().len(),
        parsed.rule_count,
        display_list.len(),
        parsed.images.len(),
        parsed.preload_hints.len(),
    );
    let layout_box = parsed.layout;
    let layout_source = LayoutSource {
        document: Arc::clone(&parsed.document),
        stylesheet: Arc::new(parsed.stylesheet),
        html_source: Some(parsed.html_source),
        cache_control_no_store,
        dynamic_css: Some(parsed.dynamic_css),
    };
    Ok((
        LoadedPage {
            display_list,
            title: parsed.title,
            images: parsed.images,
            animated_gifs: parsed.animated_gifs,
            lazy_pairs: parsed.lazy_pairs,
            layout_box,
            font_registry: parsed.font_registry,
            pending_web_fonts: parsed.pending_web_fonts,
            js_navigate: parsed.js_navigate,
            page_tracks: parsed.page_tracks,
            frames: parsed.frames,
        },
        layout_source,
        parsed.js_ctx,
    ))
}

/// РћС‚РїСЂР°РІРёС‚СЊ preload-С…РёРЅС‚С‹ РІ EventSink.
///
/// РљР°Р¶РґС‹Р№ `PreloadHint` СЂРµР·РѕР»РІРёС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ `base` (4B.3) Рё
/// РїСЂРµРѕР±СЂР°Р·СѓРµС‚СЃСЏ РІ `Event::SubresourceHintFound { url, kind, priority }`.
/// РҐРёРЅС‚С‹ СЃРѕСЂС‚РёСЂСѓСЋС‚СЃСЏ РїРѕ СѓР±С‹РІР°РЅРёСЋ РїСЂРёРѕСЂРёС‚РµС‚Р° (High в†’ Medium в†’ Low), С‡С‚РѕР±С‹
/// СЃР°РјС‹Рµ РєСЂРёС‚РёС‡РЅС‹Рµ СЂРµСЃСѓСЂСЃС‹ СЃС‚Р°СЂС‚РѕРІР°Р»Рё РїРµСЂРІС‹РјРё (РїРѕР»РµР·РЅРѕ РїСЂРё HTTP/2).
/// `srcset`-СЃС‚СЂРѕРєРё СЌРјРёС‚СЏС‚СЃСЏ РєР°Рє-РµСЃС‚СЊ (multi-URL С„РѕСЂРјР°С‚ вЂ” Р·Р°РґР°С‡Р° picker-Р°).
/// `seen` вЂ” РЅР°Р±РѕСЂ СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹С… URL (cross-call РґРµРґСѓРїР»РёРєР°С†РёСЏ); caller
/// РїРµСЂРµРґР°С‘С‚ `&mut HashSet::new()` РґР»СЏ РѕРґРЅРѕСЂР°Р·РѕРІРѕРіРѕ РІС‹Р·РѕРІР° РёР»Рё persistent-СЃРµС‚
/// РґР»СЏ РґРµРґСѓРїР° РјРµР¶РґСѓ streaming-СЃРєР°РЅРѕРј Рё С„РёРЅР°Р»СЊРЅС‹Рј pipeline.
/// Sink Р»РѕРіРёСЂСѓРµС‚ С…РёРЅС‚ РІ stderr. РЎР°Рј fetch РїРѕ С…РёРЅС‚Сѓ РґРµР»Р°РµС‚ JS-С€РёРј РЅР° СЌР»РµРјРµРЅС‚Рµ
/// `<link>` (BUG-826) вЂ” С‚Р°Рј Р¶Рµ, РіРґРµ Р¶РёРІСѓС‚ РµРіРѕ СЃРѕР±С‹С‚РёСЏ `load`/`error`; Р·РґРµСЃСЊ
/// СЃРµС‚РµРІРѕРіРѕ Р·Р°РїСЂРѕСЃР° РїРѕ-РїСЂРµР¶РЅРµРјСѓ РЅРµС‚, РїРѕСЌС‚РѕРјСѓ СЃС‚СЂРѕРєР° Р»РѕРіР° РіРѕРІРѕСЂРёС‚ В«С…РёРЅС‚ РЅР°Р№РґРµРЅВ»,
/// Р° РЅРµ В«СЂРµСЃСѓСЂСЃ Р·Р°РїСЂРѕС€РµРЅВ».
pub(crate) fn dispatch_preload_hints(
    hints: &[lumen_html_parser::PreloadHint],
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    seen: &mut std::collections::HashSet<String>,
) {
    use lumen_html_parser::PreloadHint;

    // РџРµСЂРІС‹Р№ РїСЂРѕС…РѕРґ: СЂРµР·РѕР»РІ URL + РІС‹С‡РёСЃР»РµРЅРёРµ kind.
    let mut resolved: Vec<(String, SubresourceKind)> = Vec::with_capacity(hints.len());
    for hint in hints {
        let pair = match hint {
            PreloadHint::Stylesheet { url, .. } =>
                (base.resolve_str(url), SubresourceKind::Stylesheet),
            PreloadHint::Script { url } =>
                (base.resolve_str(url), SubresourceKind::Script),
            PreloadHint::Image { url: Some(url), .. } =>
                (base.resolve_str(url), SubresourceKind::Image),
            // srcset СЃРѕРґРµСЂР¶РёС‚ СЃРїРёСЃРѕРє URL вЂ” СЂРµР·РѕР»РІРёРЅРі РєР°Р¶РґРѕРіРѕ РєР°РЅРґРёРґР°С‚Р°
            // РѕС‚РєР»Р°РґС‹РІР°РµС‚СЃСЏ РґРѕ picker-Р°; СЌРјРёС‚РёРј srcset-СЃС‚СЂРѕРєСѓ РєР°Рє-РµСЃС‚СЊ.
            PreloadHint::Image { url: None, srcset: Some(s), .. } =>
                (s.clone(), SubresourceKind::Image),
            PreloadHint::SourceSet { srcset, .. } =>
                (srcset.clone(), SubresourceKind::Image),
            PreloadHint::Preload { url, as_kind } => {
                let kind = match as_kind.as_deref() {
                    Some("font") => SubresourceKind::Font,
                    Some("image") => SubresourceKind::Image,
                    Some("script") => SubresourceKind::Script,
                    Some("style") => SubresourceKind::Stylesheet,
                    _ => SubresourceKind::Other { as_kind: as_kind.clone() },
                };
                (base.resolve_str(url), kind)
            }
            // BUG-826: РѕСЃС‚Р°Р»СЊРЅС‹Рµ РґРІР° РІРёРґР° author-С…РёРЅС‚Р°. Р РµР°Р»СЊРЅС‹Р№ fetch Рё
            // СЃРѕР±С‹С‚РёСЏ `load`/`error` РґР»СЏ РЅРёС… РґРµР»Р°РµС‚ JS-С€РёРј РЅР° СЃР°РјРѕРј СЌР»РµРјРµРЅС‚Рµ
            // (`_lumen_link_hint_prepare`), Р·РґРµСЃСЊ вЂ” С‚РѕР»СЊРєРѕ СЃС‚СЂРѕРєР° СЃРµС‚РµРІРѕРіРѕ Р»РѕРіР°.
            PreloadHint::ModulePreload { url } =>
                (base.resolve_str(url), SubresourceKind::Script),
            PreloadHint::Prefetch { url } =>
                (base.resolve_str(url), SubresourceKind::Other { as_kind: Some("prefetch".into()) }),
            // Preconnect URL вЂ” origin, РЅРµ СЃРѕРґРµСЂР¶РёС‚ path вЂ” СЂРµР·РѕР»РІРёРЅРі С‚СЂРёРІРёР°Р»РµРЅ.
            PreloadHint::Preconnect { url, dns_only } =>
                (base.resolve_str(url), SubresourceKind::Preconnect { dns_only: *dns_only }),
            PreloadHint::Image { url: None, srcset: None, .. } => continue,
        };
        resolved.push(pair);
    }

    // Stable-sort РїРѕ РїСЂРёРѕСЂРёС‚РµС‚Сѓ: High РїРµСЂРІС‹РјРё. Stable СЃРѕС…СЂР°РЅСЏРµС‚ source-order
    // РІРЅСѓС‚СЂРё РѕРґРЅРѕРіРѕ СѓСЂРѕРІРЅСЏ РїСЂРёРѕСЂРёС‚РµС‚Р° (РІР°Р¶РЅРѕ РґР»СЏ HTTP/2 multiplexing).
    resolved.sort_by_key(|(_, k)| FetchPriority::for_kind(k));

    // Р”РµРґСѓРїР»РёРєР°С†РёСЏ + emit: РїСЂРѕРїСѓСЃРєР°РµРј URL, СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅС‹Рµ РІ РїСЂРµРґС‹РґСѓС‰РёС… РІС‹Р·РѕРІР°С…
    // (cross-call dedup РґР»СЏ streaming + С„РёРЅР°Р»СЊРЅРѕРіРѕ pipeline).
    for (url, kind) in resolved {
        if seen.insert(url.clone()) {
            let priority = FetchPriority::for_kind(&kind);
            sink.emit(&Event::SubresourceHintFound { url, kind, priority });
        }
    }
}
