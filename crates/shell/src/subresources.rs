//! Page subresources fetched by the load pipeline after the document is parsed:
//! `<track>` WebVTT text, `background-image: url(...)` bitmaps, `@font-face`
//! sources and the eager `<img src>` pass (fetch + decode, batch SH-3d).
//!
//! Kept apart from [`crate::tracks`], which is deliberately network-free (its
//! fetching is abstracted behind a closure so the cue logic stays unit-testable)
//! — everything here talks to the network through [`ResourceBase`].
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-3c, SH-3d); behaviour
//! and signatures are unchanged.

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
        // LIB-4: SVG больше не особый случай — `decode_to` рисует его через
        // resvg наравне с любым растровым форматом.
        let image = match lumen_image::decode_to(&bytes, target) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("РќРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ bg-РєР°СЂС‚РёРЅРєР° {url}: {e}");
                return None;
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

/// РћР±С…РѕРґРёС‚ DOM С‡РµСЂРµР· `lumen_layout::collect_image_requests` вЂ” picker СѓС‡РёС‚С‹РІР°РµС‚
/// `<picture>`/`srcset`/`sizes`, РїРѕСЌС‚РѕРјСѓ РєР»СЋС‡ СЃРѕРІРїР°РґР°РµС‚ СЃ С‚РµРј, С‡С‚Рѕ layout
/// СЌРјРёС‚РёС‚ РІ `DisplayCommand::DrawImage.src`. Р”Р»СЏ РєР°Р¶РґРѕРіРѕ Р·Р°РїСЂРѕСЃР° СЃРєР°С‡РёРІР°РµС‚
/// Р±Р°Р№С‚С‹ Рё РґРµРєРѕРґРёСЂСѓРµС‚ С‡РµСЂРµР· `lumen_image::decode` (PNG/JPEG dispatch).
///
/// РџРѕР±РѕС‡РЅС‹Р№ СЌС„С„РµРєС‚: РґР»СЏ `<img>` Р±РµР· СЏРІРЅС‹С… `width`/`height` РїСЂРѕСЃС‚Р°РІР»СЏРµС‚
/// intrinsic dimensions РёР· РґРµРєРѕРґРёСЂРѕРІР°РЅРЅРѕРіРѕ РёР·РѕР±СЂР°Р¶РµРЅРёСЏ (HTML5 В§10 mapped
/// attributes). Author CSS Р·Р°С‚РµРј РїРµСЂРµРєСЂРѕРµС‚ РїСЂРё РЅРµРѕР±С…РѕРґРёРјРѕСЃС‚Рё.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(images, animated_gifs, lazy_pairs)`:
/// - `images` вЂ” РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ РєР°СЂС‚РёРЅРєРё РґР»СЏ РЅРµРјРµРґР»РµРЅРЅРѕР№ СЂРµРіРёСЃС‚СЂР°С†РёРё РІ renderer-Рµ
///   (РІРєР»СЋС‡Р°РµС‚ frame 0 РєР°Р¶РґРѕРіРѕ Р°РЅРёРјРёСЂРѕРІР°РЅРЅРѕРіРѕ GIF);
/// - `animated_gifs` вЂ” РјРЅРѕРіРѕРєР°РґСЂРѕРІС‹Рµ GIF-Р°РЅРёРјР°С†РёРё РґР»СЏ С‚РёРєР°РЅСЊСЏ РІ `RedrawRequested`;
/// - `lazy_pairs` вЂ” `(node_id_u32, url)` РґР»СЏ `<img loading="lazy">`, РєРѕС‚РѕСЂС‹Рµ
///   РЅРµ Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ СЃРµР№С‡Р°СЃ Рё Р±СѓРґСѓС‚ Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°РЅС‹ С‡РµСЂРµР· `_lumen_init_lazy_images`.
#[allow(clippy::type_complexity)]
pub(crate) fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: lumen_core::geom::Size,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> (Vec<(String, Arc<lumen_image::Image>)>, Vec<(String, lumen_image::AnimatedGif)>, Vec<(u32, String)>) {
    let requests = lumen_layout::collect_image_requests(doc, viewport);

    /// Р РµР·СѓР»СЊС‚Р°С‚ РїР°СЂР°Р»Р»РµР»СЊРЅРѕР№ С„Р°Р·С‹ fetch+decode РѕРґРЅРѕР№ РєР°СЂС‚РёРЅРєРё. РџСЂРёРјРµРЅРµРЅРёРµ Рє
    /// РґРѕРєСѓРјРµРЅС‚Сѓ (intrinsic size) Рё СЃР±РѕСЂРєР° РІС‹С…РѕРґРЅС‹С… РІРµРєС‚РѕСЂРѕРІ вЂ” РѕС‚РґРµР»СЊРЅРѕР№
    /// РїРѕСЃР»РµРґРѕРІР°С‚РµР»СЊРЅРѕР№ С„Р°Р·РѕР№, С‡С‚РѕР±С‹ РїРѕСЂСЏРґРѕРє Рё `&mut doc` РѕСЃС‚Р°Р»РёСЃСЊ РїРѕРґ РєРѕРЅС‚СЂРѕР»РµРј.
    enum ImgOutcome {
        /// `loading="lazy"` вЂ” РѕС‚Р»РѕР¶РёС‚СЊ РґРѕ РїСЂРёР±Р»РёР¶РµРЅРёСЏ Рє РІСЊСЋРїРѕСЂС‚Сѓ.
        Lazy,
        /// РџСЂРѕРїСѓСЃРє (РѕС€РёР±РєР° СЃРµС‚Рё/РґРµРєРѕРґРёСЂРѕРІР°РЅРёСЏ) вЂ” СѓР¶Рµ Р·Р°Р»РѕРіРёСЂРѕРІР°РЅРѕ.
        Skip,
        /// РЎС‚Р°С‚РёС‡РµСЃРєР°СЏ РєР°СЂС‚РёРЅРєР° (РІРєР»СЋС‡Р°СЏ 1-РєР°РґСЂРѕРІС‹Р№ GIF). `Arc<Image>` (BUG-272
        /// СЃСЂРµР· 17): СЂР°Р·РґРµР»СЏРµС‚ Р°Р»Р»РѕРєР°С†РёСЋ РїРёРєСЃРµР»РµР№ СЃ `IMAGE_CACHE`, Р° РЅРµ РєРѕРїРёСЂСѓРµС‚.
        Static {
            image: Arc<lumen_image::Image>,
            /// Intrinsic-СЂР°Р·РјРµСЂС‹ РґР»СЏ HTML-Р°С‚СЂРёР±СѓС‚РѕРІ, РµСЃР»Рё РёС… РЅРµ Р·Р°РґР°Р» Р°РІС‚РѕСЂ.
            intrinsic: Option<(u32, u32)>,
        },
        /// РњРЅРѕРіРѕРєР°РґСЂРѕРІС‹Р№ GIF: РїРµСЂРІС‹Р№ РєР°РґСЂ + РїРѕР»РЅР°СЏ Р°РЅРёРјР°С†РёСЏ.
        Animated {
            first: Arc<lumen_image::Image>,
            gif: lumen_image::AnimatedGif,
            intrinsic: Option<(u32, u32)>,
        },
    }

    // Р¤Р°Р·Р° 1 (РїР°СЂР°Р»Р»РµР»СЊРЅРѕ): СЃРµС‚СЊ + РґРµРєРѕРґРёСЂРѕРІР°РЅРёРµ. РќРµ С‚СЂРѕРіР°РµРј `doc`.
    // BUG-172: РґРµРєРѕРґ РёРґС‘С‚ С‡РµСЂРµР· `IMAGE_CACHE` вЂ” РєР°СЂС‚РёРЅРєРё, СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ
    // РїСЂРѕРіСЂРµСЃСЃРёРІРЅС‹Рј streaming-РїСЂРѕС…РѕРґРѕРј (`spawn_stream_image_loads`), Р±РµСЂСѓС‚СЃСЏ РёР·
    // РєСЌС€Р° Р±РµР· РїРѕРІС‚РѕСЂРЅРѕРіРѕ fetch+decode; РёС… `wants_intrinsic`/`is_lazy` СЂРµС€Р°СЋС‚СЃСЏ
    // Р·РґРµСЃСЊ, Р° РїРёРєСЃРµР»Рё С‚РѕР»СЊРєРѕ РєР»РѕРЅРёСЂСѓСЋС‚СЃСЏ.
    let outcomes = parallel_map(&requests, |_, req| {
        if req.is_lazy {
            return ImgOutcome::Lazy;
        }
        // BUG-269: apply intrinsic size whenever the author left AT LEAST ONE
        // dimension unset (not only when BOTH are unset). A replaced element
        // with a fixed width and `height: auto` must derive its height from the
        // intrinsic aspect ratio (CSS 2.1 В§10.6.2); `apply_intrinsic_size` fills
        // the missing slot from that ratio.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        let decoded = image_cache::IMAGE_CACHE.get_or_decode_current(&req.url, || {
            decode_image(&req.url, base, sink, cookie_jar.clone(), target)
        });
        match decoded {
            None => ImgOutcome::Skip,
            Some(image_cache::DecodedImage::Static(img)) => {
                // BUG-272 СЃСЂРµР· 17: share the cache's Arc, not a pixel copy.
                let intrinsic = wants_intrinsic.then_some((img.width, img.height));
                ImgOutcome::Static { image: img, intrinsic }
            }
            Some(image_cache::DecodedImage::Animated { first, gif }) => {
                let intrinsic = wants_intrinsic.then_some((first.width, first.height));
                ImgOutcome::Animated { first, gif: (*gif).clone(), intrinsic }
            }
        }
    });

    // Р¤Р°Р·Р° 2 (РїРѕСЃР»РµРґРѕРІР°С‚РµР»СЊРЅРѕ): РјСѓС‚Р°С†РёСЏ `doc` + СЃР±РѕСЂРєР° СЂРµР·СѓР»СЊС‚Р°С‚Р° РІ РїРѕСЂСЏРґРєРµ DOM.
    let mut out: Vec<(String, Arc<lumen_image::Image>)> = Vec::new();
    let mut anim_gifs: Vec<(String, lumen_image::AnimatedGif)> = Vec::new();
    let mut lazy_pairs: Vec<(u32, String)> = Vec::new();
    for (req, outcome) in requests.into_iter().zip(outcomes) {
        match outcome {
            ImgOutcome::Lazy => lazy_pairs.push((req.node_id.index() as u32, req.url)),
            ImgOutcome::Skip => {}
            ImgOutcome::Static { image, intrinsic } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                out.push((req.url, image));
            }
            ImgOutcome::Animated { first, gif, intrinsic } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                out.push((req.url.clone(), first));
                anim_gifs.push((req.url, gif));
            }
        }
    }
    (out, anim_gifs, lazy_pairs)
}

pub(crate) fn fetch_image_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    match base.resolve(raw_src) {
        ResolvedResource::File(path) => std::fs::read(&path).map_err(|e| {
            format!("file://{} {e}", path.display()).into()
        }),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            // Images are loaded in no-cors mode: cross-origin allowed, but
            // mixed-content enforcement still applies for HTTPS pages.
            let lumen_url = Url::parse(&url)?;
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
            // PERF-1: one span per image fetch вЂ” back-to-back spans on a lane
            // reveal sequential UI-thread subresource loading.
            let mut fetch_span = lumen_core::trace::span(format!("img {url}"), "net");
            let bytes = client.fetch_subresource(&lumen_url, RequestDestination::Image)?;
            fetch_span.set_bytes(bytes.len());
            Ok(bytes)
        }
    }
}

/// Fetch + decode one `<img src>` into a [`DecodedImage`], or `None` on a
/// fetch/decode failure (already logged).
///
/// BUG-172: single source of truth for the decode logic shared by the streaming
/// progressive loader ([`Lumen::spawn_stream_image_loads`]) and the final pipeline
/// ([`fetch_and_decode_images`]). Both call this through
/// [`image_cache::IMAGE_CACHE`], so each `src` is fetched and decoded exactly once
/// per navigation; the second path clones the cached pixels instead of repeating
/// the network round-trip and the decoder.
pub(crate) fn decode_image(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> Option<image_cache::DecodedImage> {
    use image_cache::DecodedImage;
    let bytes = match fetch_image_bytes(raw_src, base, sink, cookie_jar) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("РџСЂРѕРїСѓСЃРє РєР°СЂС‚РёРЅРєРё {raw_src}: {e}");
            return None;
        }
    };

    // Animated GIF detection: decode metadata lazily; keep the animation if >1 frame.
    if lumen_image::is_gif(&bytes) {
        return match lumen_image::decode_gif_animated(&bytes) {
            Ok(gif) if gif.frame_count() > 1 => {
                // BUG-272 СЃСЂРµР· 19: only the first frame is materialised eagerly.
                match gif.frame_image(0) {
                    Ok(first) => {
                        eprintln!(
                            "Р—Р°РіСЂСѓР¶РµРЅР° GIF-Р°РЅРёРјР°С†РёСЏ: {} ({}Г—{}, {} РєР°РґСЂРѕРІ)",
                            raw_src, gif.width, gif.height, gif.frame_count()
                        );
                        Some(DecodedImage::Animated {
                            first: Arc::new(first),
                            gif: Arc::new(gif),
                        })
                    }
                    Err(e) => {
                        eprintln!("РќРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ GIF {raw_src}: {e}");
                        None
                    }
                }
            }
            Ok(gif) => {
                // Single-frame GIF: treat as static image.
                gif.frame_image(0).ok().map(|image| {
                    eprintln!(
                        "Р—Р°РіСЂСѓР¶РµРЅР° РєР°СЂС‚РёРЅРєР° (GIF, 1 РєР°РґСЂ): {} ({}Г—{})",
                        raw_src, image.width, image.height
                    );
                    DecodedImage::Static(Arc::new(image))
                })
            }
            Err(e) => {
                eprintln!("РќРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ GIF {raw_src}: {e}");
                None
            }
        };
    }

    // LIB-4: SVG больше не особый случай — `decode_to` рисует его через resvg.
    match lumen_image::decode_to(&bytes, target) {
        Ok(image) => {
            eprintln!(
                "Р—Р°РіСЂСѓР¶РµРЅР° РєР°СЂС‚РёРЅРєР°: {} ({}Г—{}, {:?})",
                raw_src, image.width, image.height, image.format
            );
            Some(DecodedImage::Static(Arc::new(image)))
        }
        Err(e) => {
            eprintln!("РќРµ РґРµРєРѕРґРёСЂСѓРµС‚СЃСЏ {raw_src}: {e}");
            None
        }
    }
}

// BUG-430: `apply_intrinsic_size` РїРµСЂРµРµС…Р°Р»Р° РІ `lumen-layout` (`box_tree.rs`),
// Рє picker-Сѓ `collect_image_requests`, С‡РµР№ `ImageRequest` РѕРЅР° Рё РѕР±СЃР»СѓР¶РёРІР°РµС‚ вЂ”
// headless-РґСЂР°Р№РІРµСЂСѓ РЅСѓР¶РЅР° С‚Р° Р¶Рµ Р»РѕРіРёРєР° Р·Р°РїРѕР»РЅРµРЅРёСЏ СЃР»РѕС‚РѕРІ `width`/`height`, Р°
// РґСѓР±Р»РёСЂРѕРІР°С‚СЊ РµС‘ (СЃРїРµС†-РїСЂР°РІРёР»Рѕ BUG-269 РїСЂРѕ aspect ratio) РѕР·РЅР°С‡Р°Р»Рѕ Р±С‹ РґРІР°
// СЂР°СЃС…РѕРґСЏС‰РёС…СЃСЏ РЅР°Р±РѕСЂР° СЂР°Р·РјРµСЂРѕРІ Сѓ РѕРєРѕРЅРЅРѕРіРѕ Рё РѕС„Р»Р°Р№РЅ-РїСѓС‚РµР№.

/// PH3-19: РґРµСЃРєСЂРёРїС‚РѕСЂ @font-face url()-РёСЃС‚РѕС‡РЅРёРєР°, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅРѕРіРѕ РІ РїР°РјСЏС‚СЊ.
/// РҐСЂР°РЅРёС‚СЃСЏ РІ `ParsedPage` / `LoadedPage`; `apply_loaded_page` СЃРїР°РІРЅРёС‚
/// С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє fetch+decode РґР»СЏ РєР°Р¶РґРѕРіРѕ, СЂРµР·СѓР»СЊС‚Р°С‚ вЂ” `LoadEvent::FontLoaded`.
pub(crate) struct PendingWebFont {
    /// CSS `font-family` РґРµСЃРєСЂРёРїС‚РѕСЂ.
    pub(crate) family: String,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-weight (400 = normal, 700 = bold).
    pub(crate) weight: u16,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-style.
    pub(crate) style: lumen_core::FontStyle,
    /// РЎС‹СЂР°СЏ СЃС‚СЂРѕРєР° `unicode-range` РґРµСЃРєСЂРёРїС‚РѕСЂР° (None в†’ РїРѕРєСЂС‹РІР°РµС‚ РІСЃРµ РєРѕРґРїРѕРёРЅС‚С‹).
    pub(crate) unicode_range_str: Option<String>,
    /// URL РґР»СЏ fetch (@font-face `src: url(...)`).
    pub(crate) url: String,
}

/// PH3-19: web-С€СЂРёС„С‚, СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ Рё РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Р№ РїРѕСЃР»Рµ `FontLoaded`.
/// РЎРїРёСЃРѕРє С…СЂР°РЅРёС‚СЃСЏ РІ `Lumen::web_fonts` Рё РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РґР»СЏ РїРµСЂРµСЃР±РѕСЂРєРё
/// `MultiFontMeasurer` РїСЂРё РєР°Р¶РґРѕРј relayout вЂ” РёРЅР°С‡Рµ resize/scroll-reflow
/// С‚РµСЂСЏРµС‚ web-РјРµС‚СЂРёРєРё Рё РѕС‚РєР°С‚С‹РІР°РµС‚СЃСЏ Рє Inter.
// weight/style С…СЂР°РЅСЏС‚СЃСЏ РґР»СЏ Р±СѓРґСѓС‰РµРіРѕ CSS font-matching (РїРѕ weight/style РґРµСЃРєСЂРёРїС‚РѕСЂР°Рј @font-face).
// Clone: ADR-016 M2.2 вЂ” off-thread relayout Р·Р°С…РІР°С‚С‹РІР°РµС‚ РІР»Р°РґРµСЋС‰РёР№ СЃРЅРёРјРѕРє web-С€СЂРёС„С‚РѕРІ.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LoadedWebFont {
    /// CSS `font-family` РґРµСЃРєСЂРёРїС‚РѕСЂ.
    pub(crate) family: String,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-weight.
    pub(crate) weight: u16,
    /// Р Р°Р·СЂРµС€С‘РЅРЅС‹Р№ font-style.
    pub(crate) style: lumen_core::FontStyle,
    /// Р”РёР°РїР°Р·РѕРЅС‹ Unicode РёР· @font-face `unicode-range` РґРµСЃРєСЂРёРїС‚РѕСЂР°.
    pub(crate) unicode_range: Vec<lumen_font::UnicodeRange>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ sfnt-Р±Р°Р№С‚С‹ (TrueType / OTF РїРѕСЃР»Рµ WOFF/WOFF2-СЂР°СЃРїР°РєРѕРІРєРё).
    pub(crate) bytes: Vec<u8>,
}
