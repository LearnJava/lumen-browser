//! Page subresources fetched by the load pipeline after the document is parsed:
//! `<track>` WebVTT text, `background-image: url(...)` bitmaps and `@font-face`
//! sources.
//!
//! Kept apart from [`crate::tracks`], which is deliberately network-free (its
//! fetching is abstracted behind a closure so the cue logic stays unit-testable)
//! — everything here talks to the network through [`ResourceBase`].
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// P3-webvtt СЃСЂРµР· 3: С„РµС‚С‡РёС‚ С‚РµРєСЃС‚ `.vtt` РїРѕ `src` РёР· `<track>` (С„Р°Р№Р» РёР»Рё URL).
/// `None` вЂ” СЂРµСЃСѓСЂСЃ РЅРµ СЃРєР°С‡Р°Р»СЃСЏ; СЃС‚СЂР°РЅРёС†Р° РїСЂРѕРґРѕР»Р¶Р°РµС‚ Р¶РёС‚СЊ Р±РµР· СЃСѓР±С‚РёС‚СЂРѕРІ.
pub(crate) fn fetch_vtt_text(
    src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<String> {
    match base.resolve(src) {
        ResolvedResource::File(path) => std::fs::read_to_string(&path).ok(),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;
            let sub_url = Url::parse(&url).ok()?;
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
            let bytes = client
                .fetch_subresource(&sub_url, RequestDestination::Media)
                .ok()?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// РЎРєР°С‡РёРІР°РµС‚ Рё РґРµРєРѕРґРёСЂСѓРµС‚ РІСЃРµ `background-image: url(...)` РёР· РіРѕС‚РѕРІРѕРіРѕ
/// layout-РґРµСЂРµРІР°. Р”СѓР±Р»РёРєР°С‚С‹ URL С„РёР»СЊС‚СЂСѓСЋС‚СЃСЏ РЅР° СЃС‚РѕСЂРѕРЅРµ layout
/// (`collect_background_image_requests`). РћС€РёР±РєРё СЃРєР°С‡РёРІР°РЅРёСЏ / РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ
/// Р»РѕРіРёСЂСѓСЋС‚СЃСЏ РІ stderr вЂ” battle-tested fail-soft: Р±РёС‚Р°СЏ bg-РєР°СЂС‚РёРЅРєР° РЅРµ РІР°Р»РёС‚
/// СЃС‚СЂР°РЅРёС†Сѓ, renderer РІСЃС‘ СЂР°РІРЅРѕ РѕС‚РѕР±СЂР°Р·РёС‚ background-color РїРѕРІРµСЂС….
pub(crate) fn fetch_and_decode_background_images(
    layout: &LayoutBox,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> Vec<(String, Arc<lumen_image::Image>)> {
    // DPR 1.0 вЂ” С‚РѕС‚ Р¶Рµ, С‡С‚Рѕ Сѓ `build_display_list_ordered` (РѕР±С‘СЂС‚РєР° Р±РµР· dpr),
    // РёРЅР°С‡Рµ РІС‹Р±СЂР°РЅРЅС‹Р№ Р·РґРµСЃСЊ РєР°РЅРґРёРґР°С‚ `image-set()` РЅРµ СЃРѕРІРїР°Р» Р±С‹ СЃ РєР»СЋС‡РѕРј,
    // РєРѕС‚РѕСЂС‹Р№ СЌРјРёС‚С‚РµСЂ РєР»Р°РґС‘С‚ РІ `DrawBackgroundImage.src`.
    let urls = lumen_layout::collect_background_image_requests(layout, 1.0);
    // РџР°СЂР°Р»Р»РµР»СЊРЅР°СЏ Р·Р°РіСЂСѓР·РєР°+РґРµРєРѕРґРёСЂРѕРІР°РЅРёРµ, РїРѕСЂСЏРґРѕРє СЃРѕС…СЂР°РЅСЏРµРј (РєР»СЋС‡Рё СѓРЅРёРєР°Р»СЊРЅС‹).
    let decoded = parallel_map(&urls, |_, url| {
        let bytes = match fetch_image_bytes(url, base, sink, cookie_jar.clone()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("РџСЂРѕРїСѓСЃРє bg-РєР°СЂС‚РёРЅРєРё {url}: {e}");
                return None;
            }
        };
        // RP-5: РІРЅРµС€РЅРёР№ SVG СЂРµРЅРґРµСЂРёС‚СЃСЏ С‡РµСЂРµР· layout/paint-pipeline, РєР°Рє РІ
        // decode_image; РѕСЃС‚Р°Р»СЊРЅС‹Рµ С„РѕСЂРјР°С‚С‹ вЂ” РѕР±С‹С‡РЅС‹Рј СЂР°СЃС‚СЂРѕРІС‹Рј РґРµРєРѕРґРµСЂРѕРј.
        let image = if lumen_image::is_svg(&bytes) {
            match svg_image::rasterize_svg(&bytes, base, sink) {
                Some(i) => i,
                None => return None,
            }
        } else {
            match lumen_image::decode_to(&bytes, target) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("РќРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ bg-РєР°СЂС‚РёРЅРєР° {url}: {e}");
                    return None;
                }
            }
        };
        eprintln!(
            "Р—Р°РіСЂСѓР¶РµРЅР° bg-РєР°СЂС‚РёРЅРєР°: {url} ({}Г—{}, {:?})",
            image.width, image.height, image.format
        );
        // BUG-272 СЃСЂРµР· 17: wrap once in Arc so `register_image` shares the buffer.
        Some((url.clone(), Arc::new(image)))
    });
    decoded.into_iter().flatten().collect()
}

/// Р—Р°РіСЂСѓР¶Р°РµС‚ С€СЂРёС„С‚С‹ РёР· @font-face РїСЂР°РІРёР» С‚Р°Р±Р»РёС†С‹ СЃС‚РёР»РµР№ РІ `FontRegistry`.
///
/// Р”Р»СЏ РєР°Р¶РґРѕРіРѕ `FontFaceRule` РїРµСЂРµР±РёСЂР°РµС‚ `src:` РёСЃС‚РѕС‡РЅРёРєРё РІ РїРѕСЂСЏРґРєРµ (CSS В§4.1:
/// РїРµСЂРІС‹Р№ СѓСЃРїРµС€РЅС‹Р№ wins). `local()` РїСЂРѕРїСѓСЃРєР°РµС‚СЃСЏ вЂ” `SystemFontIndex` СѓР¶Рµ
/// РїРѕРєСЂС‹РІР°РµС‚ СЃРёСЃС‚РµРјРЅС‹Рµ С€СЂРёС„С‚С‹. `url()` Р·Р°РіСЂСѓР¶Р°РµС‚СЃСЏ С‚Р°Рє Р¶Рµ, РєР°Рє РёР·РѕР±СЂР°Р¶РµРЅРёСЏ.
/// WOFF/WOFF2 РїСЂРѕР·СЂР°С‡РЅРѕ РґРµРєРѕРґРёСЂСѓСЋС‚СЃСЏ РІ sfnt РїРµСЂРµРґ СЂРµРіРёСЃС‚СЂР°С†РёРµР№.
///
/// РћС€РёР±РєРё Р·Р°РіСЂСѓР·РєРё/РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ РѕС‚РґРµР»СЊРЅС‹С… РёСЃС‚РѕС‡РЅРёРєРѕРІ РЅРµ С„Р°С‚Р°Р»СЊРЅС‹: РїРёС€СѓС‚СЃСЏ РІ
/// stderr Рё РїРµСЂРµС…РѕРґРёРј Рє СЃР»РµРґСѓСЋС‰РµРјСѓ РёСЃС‚РѕС‡РЅРёРєСѓ.
/// Convert a FontFaceRule from the CSS parser to a DOM FontFace object.
pub(crate) fn rule_to_font_face(rule: &lumen_css_parser::FontFaceRule) -> lumen_dom::FontFace {
    use lumen_css_parser::FontFaceSourceKind;

    let src_parts: Vec<String> = rule
        .sources
        .iter()
        .map(|src| {
            let kind_str = match src.kind {
                FontFaceSourceKind::Url => "url",
                FontFaceSourceKind::Local => "local",
            };
            format!("{}(\"{}\")", kind_str, src.value)
        })
        .collect();
    let src_str = src_parts.join(", ");

    lumen_dom::FontFace::new(
        rule.family.clone(),
        rule.style.as_deref().unwrap_or("normal").to_string(),
        rule.weight.as_deref().unwrap_or("400").to_string(),
        rule.stretch.clone(),
        rule.unicode_range.clone(),
        src_str,
    )
}

/// PH3-19: Р·Р°РіСЂСѓР¶Р°РµС‚ @font-face РїСЂР°РІРёР»Р°, СЂР°Р·РґРµР»СЏСЏ РёСЃС‚РѕС‡РЅРёРєРё РЅР° РґРІР° РїСЂРѕС…РѕРґР°:
/// 1. `local()` вЂ” СЃРёРЅС…СЂРѕРЅРЅРѕ (СЃРёСЃС‚РµРјРЅС‹Рµ С€СЂРёС„С‚С‹ СѓР¶Рµ РІ РїР°РјСЏС‚Рё), СЂРµР·СѓР»СЊС‚Р°С‚ РІ `registry`.
/// 2. `url()` вЂ” РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ РєР°Рє `Vec<PendingWebFont>` РґР»СЏ С„РѕРЅРѕРІРѕР№ Р·Р°РіСЂСѓР·РєРё;
///    РїРµСЂРІС‹Р№ layout СЃС‚СЂРѕРёС‚СЃСЏ РЅР° fallback (bundled Inter), web-С€СЂРёС„С‚С‹ РїСЂРёС…РѕРґСЏС‚ РїРѕР·Р¶Рµ
///    С‡РµСЂРµР· `LoadEvent::FontLoaded` Рё РІС‹Р·С‹РІР°СЋС‚ relayout (FOUT-swap).
///
/// CSS Fonts L4 В§4.1: РёСЃС‚РѕС‡РЅРёРєРё РІ РєР°Р¶РґРѕРј `@font-face` РїСЂРѕР±СѓСЋС‚СЃСЏ РїРѕ РїРѕСЂСЏРґРєСѓ;
/// РїРµСЂРІС‹Р№ СѓСЃРїРµС€РЅС‹Р№ `local()` РІС‹РёРіСЂС‹РІР°РµС‚ Рё url()-РёСЃС‚РѕС‡РЅРёРєРё СЌС‚РѕРіРѕ РїСЂР°РІРёР»Р° РїСЂРѕРїСѓСЃРєР°СЋС‚СЃСЏ.
pub(crate) fn load_font_faces(
    font_faces: &[lumen_css_parser::FontFaceRule],
    _base: &ResourceBase,
    _sink: &Arc<dyn EventSink>,
    _cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> (lumen_font::FontRegistry, Vec<PendingWebFont>) {
    use lumen_css_parser::FontFaceSourceKind;
    use lumen_core::FontStyle;

    let registry = lumen_font::FontRegistry::new();
    let mut pending: Vec<PendingWebFont> = Vec::new();

    for rule in font_faces {
        if rule.family.is_empty() || rule.sources.is_empty() {
            continue;
        }

        let weight = parse_font_weight(rule.weight.as_deref());
        let style = rule
            .style
            .as_deref()
            .and_then(FontStyle::parse_keyword)
            .unwrap_or(FontStyle::Normal);
        // CSS Fonts L4 В§4.5: РґРµСЃРєСЂРёРїС‚РѕСЂ `font-stretch` РїСЂР°РІРёР»Р° СѓС‡Р°СЃС‚РІСѓРµС‚ РІ
        // РїРѕРґР±РѕСЂРµ `local()`-РёСЃС‚РѕС‡РЅРёРєР° вЂ” `@font-face { src: local("Arial");
        // font-stretch: condensed }` РѕР±СЏР·Р°РЅ РІР·СЏС‚СЊ СѓР·РєРёР№ face СЃРµРјРµР№СЃС‚РІР°, Р° РЅРµ
        // РѕР±С‹С‡РЅС‹Р№. Р”РёР°РїР°Р·РѕРЅ РёР· РґРІСѓС… Р·РЅР°С‡РµРЅРёР№ СЃРІРѕРґРёС‚СЃСЏ Рє РїРµСЂРІРѕРјСѓ (`parse`).
        let stretch = rule
            .stretch
            .as_deref()
            .and_then(lumen_layout::FontStretch::parse)
            .unwrap_or(lumen_layout::FontStretch::NORMAL)
            .as_percent();

        let mut local_resolved = false;
        for src in &rule.sources {
            if src.kind == FontFaceSourceKind::Local {
                // CSS Fonts L4 В§4.1 + В§4.3: try local() first; case-insensitive
                // match against system fonts. First hit wins the whole rule.
                if let Some(bytes) = registry.resolve_local_bytes(&src.value, weight, style, stretch) {
                    eprintln!(
                        "@font-face Р·Р°РіСЂСѓР¶РµРЅ: В«{}В» weight={} src={} (local)",
                        rule.family, weight, src.value,
                    );
                    let ranges = rule
                        .unicode_range
                        .as_deref()
                        .map(lumen_font::parse_unicode_ranges)
                        .unwrap_or_default();
                    registry.register_from_bytes(&rule.family, weight, style, &ranges, bytes);
                    local_resolved = true;
                    break;
                }
            }
        }
        if local_resolved {
            continue;
        }

        // No local() succeeded вЂ” queue the first url() source for async fetch.
        if let Some(url_src) = rule.sources.iter().find(|s| s.kind == FontFaceSourceKind::Url) {
            pending.push(PendingWebFont {
                family: rule.family.clone(),
                weight,
                style,
                unicode_range_str: rule.unicode_range.clone(),
                url: url_src.value.clone(),
            });
        }
    }

    (registry, pending)
}

/// РџР°СЂСЃРёС‚ `font-weight` РґРµСЃРєСЂРёРїС‚РѕСЂ @font-face: РєР»СЋС‡РµРІС‹Рµ СЃР»РѕРІР° + С‡РёСЃР»Р°.
/// Р”РёР°РїР°Р·РѕРЅС‹ (`400 700`) вЂ” Р±РµСЂС‘Рј РїРµСЂРІРѕРµ Р·РЅР°С‡РµРЅРёРµ. Default: 400.
fn parse_font_weight(s: Option<&str>) -> u16 {
    let Some(s) = s else { return 400 };
    match s.trim() {
        "normal" => 400,
        "bold" => 700,
        other => other
            .split_ascii_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(400),
    }
}
