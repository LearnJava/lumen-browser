//! The page-load pipeline's entry point: [`render_bytes`] turns fetched bytes
//! into a laid-out, painted page, and [`dispatch_preload_hints`] emits the
//! preload-scanner hints the parser found along the way.
//!
//! Below them (batch SH-3d) is the phase `render_bytes` is a wrapper around —
//! [`parse_and_layout`], shared with the headless dump modes — and the three
//! shapes a page takes on the way through: [`ParsedPage`] (what `decode →
//! parse → layout` produced), [`LoadedPage`] (what the window is to draw and
//! be titled with) and [`LayoutSource`] (what a later reflow re-runs from
//! without touching the network).
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-3c, SH-3d); behaviour
//! and signatures are unchanged.

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

/// Р РµР·СѓР»СЊС‚Р°С‚ Р·Р°РіСЂСѓР·РєРё СЃС‚СЂР°РЅРёС†С‹: С‡С‚Рѕ СЂРёСЃРѕРІР°С‚СЊ Рё РєР°Рє РЅР°Р·РІР°С‚СЊ РѕРєРЅРѕ.
/// Р Р°СЃС€РёСЂСЏРµС‚СЃСЏ: favicon, current URL, scroll state вЂ” РїРѕР·Р¶Рµ.
pub(crate) struct LoadedPage {
    pub(crate) display_list: DisplayList,
    pub(crate) title: Option<String>,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ `<img src="вЂ¦">` РґР»СЏ GPU upload С‡РµСЂРµР·
    /// `Renderer::register_image`. РљР»СЋС‡ вЂ” raw src attribute value (С‚РѕС‚ Р¶Рµ,
    /// С‡С‚Рѕ РїРѕРїР°РґР°РµС‚ РІ `DisplayCommand::DrawImage.src`), С‡С‚РѕР±С‹ render-side
    /// РјРѕРі СЃРґРµР»Р°С‚СЊ lookup Р±РµР· РѕС‚РґРµР»СЊРЅРѕР№ РЅРѕСЂРјР°Р»РёР·Р°С†РёРё URL. `Arc<Image>` (BUG-272
    /// СЃСЂРµР· 17): СЂР°Р·РґРµР»СЏРµС‚ РїРёРєСЃРµР»Рё СЃ `IMAGE_CACHE`/`register_image`, РЅРµ РєРѕРїРёСЂСѓРµС‚.
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations decoded at load time. Keyed by the same src URL
    /// as `DrawImage.src`. Frame 0 of each entry is already in `images` so the
    /// renderer has a valid texture on first paint; subsequent frames are uploaded
    /// on each `RedrawRequested` tick via `Lumen::animated_gifs`.
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` вЂ” registered with JS
    /// after page load via `_lumen_init_lazy_images` for proximity-based loading.
    #[allow(dead_code)] // read only inside #[cfg(feature = "v8")] blocks
    pub(crate) lazy_pairs: Vec<(u32, String)>,
    /// Layout-РґРµСЂРµРІРѕ СЃС‚СЂР°РЅРёС†С‹ вЂ” РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ animation scheduler-РѕРј.
    pub(crate) layout_box: lumen_layout::LayoutBox,
    /// РџСЂРѕРІР°Р№РґРµСЂ С€СЂРёС„С‚РѕРІ СЃ @font-face local()-РёСЃС‚РѕС‡РЅРёРєР°РјРё СЃС‚СЂР°РЅРёС†С‹.
    /// РџРµСЂРµРґР°С‘С‚СЃСЏ СЂРµРЅРґРµСЂСѓ С‡РµСЂРµР· `set_font_provider` РїСЂРё apply_loaded_page.
    /// PH3-19: РєРѕРЅРєСЂРµС‚РЅС‹Р№ С‚РёРї (РЅРµ С‚СЂРµР№С‚-РѕР±СЉРµРєС‚), С‡С‚РѕР±С‹ `apply_loaded_page`
    /// РјРѕРі РґРёРЅР°РјРёС‡РµСЃРєРё РґРѕСЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ web-С€СЂРёС„С‚С‹ С‡РµСЂРµР· `register_from_bytes`.
    pub(crate) font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-РёСЃС‚РѕС‡РЅРёРєРё, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ РІ РјРѕРјРµРЅС‚ РїРµСЂРІРѕРіРѕ
    /// layout-Р°. `apply_loaded_page` СЃРїР°РІРЅРёС‚ С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє РґР»СЏ РєР°Р¶РґРѕРіРѕ;
    /// СЂРµР·СѓР»СЊС‚Р°С‚ РїСЂРёС…РѕРґРёС‚ РєР°Рє `LoadEvent::FontLoaded` в†’ relayout СЃ FOUT.
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ РѕС‚ JS (location.href= Рё С‚.Рї.), РІС‹РїРѕР»РЅРµРЅРЅС‹Р№
    /// РІ РїСЂРѕС†РµСЃСЃРµ Р·Р°РіСЂСѓР·РєРё. РћР±СЂР°Р±Р°С‚С‹РІР°РµС‚СЃСЏ РІ `about_to_wait`.
    pub(crate) js_navigate: Option<JsNavigateRequest>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues РїРѕ РєР°Р¶РґРѕРјСѓ `<video>` СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) page_tracks: tracks::PageTracks,
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` вЂ” РґРµСЂР¶Р°С‚ JS-РєРѕРЅС‚РµРєСЃС‚С‹
    /// Рё DOM РґРµС‚РµР№ РґРѕ Р·Р°РјРµРЅС‹ СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) frames: Vec<FrameHandle>,
}

impl LoadedPage {
    pub(crate) fn empty() -> Self {
        Self {
            display_list: DisplayList::new(),
            title: None,
            images: Vec::new(),
            animated_gifs: Vec::new(),
            lazy_pairs: Vec::new(),
            layout_box: lumen_layout::LayoutBox {
                node: NodeId::from_index(0),
                rect: Rect::ZERO,
                style: std::sync::Arc::new(lumen_layout::style::ComputedStyle::root()),
                kind: lumen_layout::BoxKind::Block,
                children: Vec::new(),
                col_span: 1,
                row_span: 1,
                svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                dirty: lumen_layout::DirtyBits::CLEAN,
                origin: lumen_layout::BoxOrigin { node: None, role: lumen_layout::BoxRole::Placeholder },
            },
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate: None,
            page_tracks: tracks::PageTracks::default(),
            frames: Vec::new(),
        }
    }
}

/// Р РµР·СѓР»СЊС‚Р°С‚ С„Р°Р· `decode в†’ parse в†’ layout` вЂ” РѕР±С‰Р°СЏ С‡Р°СЃС‚СЊ РґР»СЏ РѕРєРѕРЅРЅРѕРіРѕ Рё
/// dump-СЂРµР¶РёРјРѕРІ. РџРѕР»СЏ РІР»Р°РґРµСЋС‚ СЃРІРѕРёРјРё РґР°РЅРЅС‹РјРё вЂ” РЅРµС‚ СЃСЃС‹Р»РѕРє РЅР°СЂСѓР¶Сѓ.
pub(crate) struct ParsedPage {
    /// Parsed DOM вЂ” shared with JS closures via Arc so event handlers can
    /// mutate the document without rebuilding the entire page.
    pub(crate) document: Arc<Mutex<Document>>,
    pub(crate) stylesheet: lumen_css_parser::Stylesheet,
    pub(crate) layout: LayoutBox,
    pub(crate) title: Option<String>,
    pub(crate) rule_count: usize,
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Рµ РёР·РѕР±СЂР°Р¶РµРЅРёСЏ, РЅР°Р№РґРµРЅРЅС‹Рµ РїСЂРё РѕР±С…РѕРґРµ DOM. РЎРј. [`LoadedPage::images`].
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    /// Multi-frame GIF animations found in the DOM. See [`LoadedPage::animated_gifs`].
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
    /// `(node_id_u32, url)` pairs for `<img loading="lazy">` elements вЂ” skipped by
    /// the eager fetch pass; registered with JS `_lumen_init_lazy_images` after load.
    pub(crate) lazy_pairs: Vec<(u32, String)>,
    /// Subresource-С…РёРЅС‚С‹, РЅР°Р№РґРµРЅРЅС‹Рµ preload-СЃРєР°РЅРµСЂРѕРј Р”Рћ DOM-РїР°СЂСЃРёРЅРіР°.
    /// Source-order: РїРµСЂРІС‹Рµ С…РёРЅС‚С‹ РІР°Р¶РЅРµРµ (РёС… fetch СЃС‚Р°СЂС‚СѓРµС‚ РїРµСЂРІС‹Рј).
    pub(crate) preload_hints: Vec<lumen_html_parser::PreloadHint>,
    /// Decoded UTF-8 HTML source вЂ” stored for bfcache snapshot.
    pub(crate) html_source: String,
    /// @font-face local()-С€СЂРёС„С‚С‹ + СЃРёСЃС‚РµРјРЅС‹Рµ С€СЂРёС„С‚С‹. РџРµСЂРµРґР°С‘С‚СЃСЏ СЂРµРЅРґРµСЂСѓ.
    /// PH3-19: РєРѕРЅРєСЂРµС‚РЅС‹Р№ `FontRegistry` (РЅРµ С‚СЂРµР№С‚-РѕР±СЉРµРєС‚) РґР»СЏ РґРѕСЂРµРіРёСЃС‚СЂР°С†РёРё
    /// web-С€СЂРёС„С‚РѕРІ РїРѕСЃР»Рµ `FontLoaded` Р±РµР· РґР°СѓРЅРєР°СЃС‚Р°.
    pub(crate) font_registry: Arc<lumen_font::FontRegistry>,
    /// PH3-19: @font-face url()-РёСЃС‚РѕС‡РЅРёРєРё, РµС‰С‘ РЅРµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ; РїРµСЂРµРґР°СЋС‚СЃСЏ РІ
    /// `LoadedPage` Рё РґР°Р»РµРµ РІ С„РѕРЅРѕРІС‹Рµ РїРѕС‚РѕРєРё С‡РµСЂРµР· `apply_loaded_page`.
    pub(crate) pending_web_fonts: Vec<PendingWebFont>,
    /// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ, РІС‹СЃС‚Р°РІР»РµРЅРЅС‹Р№ JS РІРѕ РІСЂРµРјСЏ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ.
    pub(crate) js_navigate: Option<JsNavigateRequest>,
    /// Persistent JS context (V8) kept alive after page load so that
    /// event handlers registered via `addEventListener` continue to work.
    /// `None` when the v8 feature is disabled or script init failed.
    ///
    /// ADR-016 M2.2c-2b: `Arc` (РЅРµ `Box`), С‡С‚РѕР±С‹ С…СЌРЅРґР» РјРѕР¶РЅРѕ Р±С‹Р»Рѕ СЂР°Р·РґРµР»РёС‚СЊ СЃ
    /// РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј (`EngineJsState`) РЅР° РІСЂРµРјСЏ РјРёРіСЂР°С†РёРё `js_ctx` РЅР° РЅРµРіРѕ.
    pub(crate) js_ctx: Option<Arc<dyn PersistentJs>>,
    /// P3-webvtt СЃСЂРµР· 3: WebVTT-cues, Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ РёР· `<track>` РєР°Р¶РґРѕРіРѕ `<video>`.
    pub(crate) page_tracks: tracks::PageTracks,
    /// BUG-743: РЅРµРёР·РјРµРЅСЏРµРјР°СЏ С‡Р°СЃС‚СЊ CSS + РѕС‚РїРµС‡Р°С‚РѕРє РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`,
    /// С‡С‚РѕР±С‹ РїРѕР·РґРЅСЏСЏ РІСЃС‚Р°РІРєР° Р»РёСЃС‚Р° РїРµСЂРµСЃРѕР±СЂР°Р»Р° РєР°СЃРєР°Рґ Р±РµР· СЃРµС‚Рё.
    pub(crate) dynamic_css: DynamicCssBase,
    /// BUG-480 СЃСЂРµР· 1: Р¶РёРІС‹Рµ sub-РґРѕРєСѓРјРµРЅС‚С‹ `<iframe>` СЌС‚РѕР№ СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) frames: Vec<FrameHandle>,
}

/// РСЃС‚РѕС‡РЅРёРє РґР»СЏ РїРѕРІС‚РѕСЂРЅРѕРіРѕ layout Р±РµР· РїРѕРІС‚РѕСЂРЅРѕР№ Р·Р°РіСЂСѓР·РєРё/РїР°СЂСЃРёРЅРіР°.
/// РҐСЂР°РЅРёС‚СЃСЏ РІ `Lumen`; РѕР±РЅРѕРІР»СЏРµС‚СЃСЏ С‚РѕР»СЊРєРѕ РїСЂРё reload/load РЅРѕРІРѕР№ СЃС‚СЂР°РЅРёС†С‹.
pub(crate) struct LayoutSource {
    /// DOM вЂ” shared with the persistent JS runtime via Arc<Mutex> so that
    /// JS event handlers can mutate it between repaints.
    pub(crate) document: Arc<Mutex<Document>>,
    /// Parsed stylesheet, shared as an immutable `Arc` snapshot (ADR-016 M2.2b):
    /// off-thread relayout jobs clone the handle (`Arc::clone`) instead of deep-
    /// cloning the whole `Stylesheet` on every submit. Replaced wholesale on
    /// reload/thaw, never mutated in place.
    pub(crate) stylesheet: Arc<lumen_css_parser::Stylesheet>,
    /// Decoded HTML source captured after encoding detection. Used by bfcache
    /// to restore the page without a network round-trip.
    #[allow(dead_code)]
    pub(crate) html_source: Option<String>,
    /// `Cache-Control: no-store` on the response that produced this page.
    /// Checked by [`Lumen::bfcache_eligible`] on navigate-away; `true` routes
    /// the page to the HTML-snapshot bfcache fallback instead of a full
    /// freeze. `false` for non-network sources (file/thaw/sidebar/hibernate
    /// restore) вЂ” no header to check, so the page is treated as cacheable.
    pub(crate) cache_control_no_store: bool,
    /// BUG-743: С‡Р°СЃС‚СЊ CSS, РЅРµ Р·Р°РІРёСЃСЏС‰Р°СЏ РѕС‚ РёРЅР»Р°Р№РЅРѕРІС‹С… `<style>`, РїР»СЋСЃ РѕС‚РїРµС‡Р°С‚РѕРє
    /// С‚РµС… Р±Р»РѕРєРѕРІ, РёР· РєРѕС‚РѕСЂС‹С… СЃРѕР±СЂР°РЅ С‚РµРєСѓС‰РёР№ [`Self::stylesheet`]. `Some` РЅР°
    /// РѕР±С‹С‡РЅРѕРј РїСѓС‚Рё Р·Р°РіСЂСѓР·РєРё; `None` РЅР° РїСѓС‚СЏС… РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ (bfcache-thaw,
    /// СЂР°Р·РјРѕСЂРѕР·РєР° РІРєР»Р°РґРєРё, sidebar), РіРґРµ РёСЃС…РѕРґРЅС‹Рµ С‡Р°СЃС‚Рё CSS РЅРµ СЃРѕС…СЂР°РЅРµРЅС‹ вЂ” С‚Р°Рј
    /// РєР°СЃРєР°Рґ РІРµРґС‘С‚ СЃРµР±СЏ РєР°Рє РґРѕ BUG-743 Рё РїРѕР·РґРЅРёР№ `<style>` РЅРµ РїРѕРґС…РІР°С‚С‹РІР°РµС‚СЃСЏ.
    pub(crate) dynamic_css: Option<DynamicCssBase>,
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
pub(crate) fn parse_and_layout(
    bytes: &[u8],
    content_type: Option<&str>,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: Size,
    preload_seen: &mut std::collections::HashSet<String>,
    ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
    ss_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
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
    media_print: bool,
) -> Result<ParsedPage, Box<dyn Error>> {
    // РљРѕРґРёСЂРѕРІРєСѓ РѕРїСЂРµРґРµР»СЏРµРј РїРѕ BOM -> <meta charset> -> СЌРІСЂРёСЃС‚РёРєРµ. Р­С‚Рѕ РїРѕРєСЂС‹РІР°РµС‚
    // Рё UTF-8 (Р±РѕР»СЊС€РёРЅСЃС‚РІРѕ), Рё СЃС‚Р°СЂС‹Рµ cp1251 / koi8-r / cp866 С„Р°Р№Р»С‹.
    let encoding = lumen_encoding::detect(bytes, content_type);
    let source = lumen_encoding::decode(encoding, bytes);
    eprintln!("РљРѕРґРёСЂРѕРІРєР°: {}", encoding.name());

    // Preload-СЃРєР°РЅРµСЂ Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ Р”Рћ DOM-РїР°СЂСЃРёРЅРіР° (HTML LS В§13.2.6.4.7).
    // `preload_seen` вЂ” cross-call dedup: РµСЃР»Рё streaming СѓР¶Рµ РѕС‚РїСЂР°РІРёР» <head>-С…РёРЅС‚С‹
    // С‡РµСЂРµР· EarlyPreloadHints, С„РёРЅР°Р»СЊРЅС‹Р№ scan РїСЂРѕРїСѓСЃС‚РёС‚ РёС… Рё РґРѕР±Р°РІРёС‚ С‚РѕР»СЊРєРѕ РЅРѕРІС‹Рµ
    // (body-images, lazy-loaded resources Рё С‚.Рї.).
    let preload_hints = lumen_html_parser::scan_preload_hints(&source);
    dispatch_preload_hints(&preload_hints, base, sink, preload_seen);

    let mut doc = {
        let _s = lumen_core::trace::span("parse-html", "parse");
        lumen_html_parser::parse(&source)
    };
    // BUG-358: stamp the document with what it was actually decoded as / served
    // as, so `document.characterSet`/`charset`/`inputEncoding`/`contentType`
    // read real per-load state instead of `undefined`.
    doc.set_character_set(encoding.canonical_name().to_string());
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or(ct).trim();
        if !mime.is_empty() {
            doc.set_content_type(mime.to_string());
        }
    }
    let title = extract_title(&doc);

    // Р“РµР№С‚ РІС‹РїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ: top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
    // QuickJS + install_dom РґР°СЋС‚ СЃРєСЂРёРїС‚Р°Рј РїРѕР»РЅС‹Р№ РґРѕСЃС‚СѓРї Рє DOM-РґРµСЂРµРІСѓ.
    // fetch_provider РїСЂРѕР±СЂР°СЃС‹РІР°РµС‚СЃСЏ РІ window.fetch(); ws_provider вЂ” РІ new WebSocket();
    // sse_provider вЂ” РІ new EventSource(). Р’СЃРµ С‚СЂРё РёСЃРїРѕР»СЊР·СѓСЋС‚ РѕРґРёРЅ HttpClient.
    let (fetch_provider, ws_provider, sse_provider) = match base {
        ResourceBase::Url(_) => {
            let client = base.http_client_for_subresource(Arc::clone(sink), cookie_jar.clone());
            let arc_client = Arc::new(client);
            let fp: Option<Arc<dyn lumen_core::ext::JsFetchProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsFetchProvider>);
            let wp: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>> =
                Some(Arc::clone(&arc_client) as Arc<dyn lumen_core::ext::JsWebSocketProvider>);
            let sp: Option<Arc<dyn lumen_core::ext::JsSseProvider>> =
                Some(arc_client as Arc<dyn lumen_core::ext::JsSseProvider>);
            (fp, wp, sp)
        }
        ResourceBase::File(_) => (None, None, None),
    };
    // URL СЃС‚СЂР°РЅРёС†С‹ РґР»СЏ РёРЅРёС†РёР°Р»РёР·Р°С†РёРё window.location РІ JS.
    let page_url = base_url_string(base);
    // Extension content scripts: collect JS sources that match the page URL.
    let ext_registry = extensions::ExtensionRegistry::load();
    let ext_scripts = ext_registry.content_scripts_for_url(&page_url);
    // BUG-164: collect classic + module scripts in document order and fetch
    // external `<script src>` bodies via the subresource fetcher, so SPA
    // bundles execute (lenta.ru owlBundle.js etc.), not just inline scripts.
    let (classic_scripts, module_scripts) = {
        let _s = lumen_core::trace::span("fetch-scripts", "net");
        let mut classic_items = Vec::new();
        let mut module_items = Vec::new();
        collect_scripts_ordered(&doc, doc.root(), &mut classic_items, &mut module_items);
        (
            resolve_script_sources(&classic_items, base, sink, cookie_jar.clone()),
            resolve_script_sources(&module_items, base, sink, cookie_jar.clone()),
        )
    };
    let run_scripts_span = lumen_core::trace::span("run-scripts", "script");
    // BUG-480 СЃСЂРµР· 1: РєР»РѕРЅС‹ РїСЂРѕРІР°Р№РґРµСЂРѕРІ/С…СЂР°РЅРёР»РёС‰ РґР»СЏ sub-РґРѕРєСѓРјРµРЅС‚РѕРІ <iframe> вЂ”
    // РѕСЃРЅРѕРІРЅС‹Рµ СѓС…РѕРґСЏС‚ РІ run_scripts_with_dom РїРѕ Р·РЅР°С‡РµРЅРёСЋ.
    let (frame_fp, frame_wp, frame_sp) =
        (fetch_provider.clone(), ws_provider.clone(), sse_provider.clone());
    let (frame_ls, frame_ss, frame_idb) =
        (ls_store.clone(), ss_store.clone(), idb_backend.clone());
    let (frame_sw, frame_sww, frame_cache) =
        (sw_backend.clone(), sw_worker_store.clone(), cache_backend.clone());
    let frame_cookie_jar = cookie_jar.clone();
    let (doc_arc, js_nav, js_ctx) = run_scripts_with_dom(
        doc,
        lumen_core::SandboxFlags::empty(),
        &page_url,
        fetch_provider,
        ws_provider,
        sse_provider,
        ls_store,
        ss_store,
        idb_backend,
        sw_backend,
        sw_worker_store,
        cache_backend,
        cookie_banner_dismiss,
        deterministic,
        cross_origin_isolated,
        &ext_scripts,
        classic_scripts,
        module_scripts,
        false,
    );
    drop(run_scripts_span);
    // HTML LS В§8.2.3 вЂ” after HTML parse + inline scripts: readyState в†’ "interactive"
    // + DOMContentLoaded event. Fires before images/fonts are decoded.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        js.notify_dom_content_loaded();
    }

    // CSS Selectors L4 В§9.6 `:target`: set current target from URL fragment so
    // the matcher has the correct target_id before style cascade in layout.
    let page_fragment = if let ResourceBase::Url(u) = base {
        lumen_core::url::Url::parse(u)
            .ok()
            .and_then(|u| u.fragment().map(str::to_owned))
    } else {
        None
    };
    {
        let mut d = doc_arc.lock().unwrap();
        d.set_target(page_fragment.as_deref());
        // Р“РµР№С‚ РѕС‚РїСЂР°РІРєРё С„РѕСЂРј: Phase 0 вЂ” top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
        check_form_gate(&d, lumen_core::SandboxFlags::empty());
        // Р“РµР№С‚ РЅР°РІРёРіР°С†РёРё: Phase 0 вЂ” top-level РґРѕРєСѓРјРµРЅС‚ РЅРµ sandboxed.
        check_navigation_gate(&d, lumen_core::SandboxFlags::empty());
        // РџСЂРёРјРµРЅСЏРµРј sandbox-РѕРіСЂР°РЅРёС‡РµРЅРёСЏ РёР· <iframe sandbox> СЌР»РµРјРµРЅС‚РѕРІ.
        // Phase 0: iframe sub-РґРѕРєСѓРјРµРЅС‚С‹ РЅРµ Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ вЂ” РїСЂРёРјРµРЅСЏРµРј РіРµР№С‚С‹
        // Рє СЃР°РјРѕРјСѓ iframe-СЌР»РµРјРµРЅС‚Сѓ, Р»РѕРіРёСЂСѓРµРј РѕРіСЂР°РЅРёС‡РµРЅРёСЏ РґР»СЏ Р±СѓРґСѓС‰РµРіРѕ Phase 1.
        apply_iframe_sandbox_gates(&d);
    }

    // BUG-480 СЃСЂРµР· 1: Р·Р°РіСЂСѓР·РєР° sub-РґРѕРєСѓРјРµРЅС‚РѕРІ <iframe>. Р›РѕРєРё РІРЅСѓС‚СЂРё С„СѓРЅРєС†РёРё
    // РєРѕСЂРѕС‚РєРёРµ вЂ” СЃРєСЂРёРїС‚С‹ РґРµС‚РµР№ Рё `load` С…РѕСЃС‚Р° РёРґСѓС‚ Р±РµР· СѓРґРµСЂР¶Р°РЅРёСЏ РґРµСЂРµРІР°.
    // РЎСЂРµР· 3: РґРѕРєСѓРјРµРЅС‚/Р±Р°Р·Р° СЃС‚СЂР°РЅРёС†С‹ РїРµСЂРµРґР°СЋС‚СЃСЏ Рё РєР°Рє top вЂ” Сѓ С„СЂРµР№РјРѕРІ
    // РїРµСЂРІРѕРіРѕ СѓСЂРѕРІРЅСЏ parent === top, РіР»СѓР±Р¶Рµ top РІСЃРµРіРґР° РєРѕСЂРµРЅСЊ.
    // РЎСЂРµР· 11: СЌРєСЂР°РЅРЅС‹Р№ media-РіРµР№С‚ `<link>` Рё РІСЊСЋРїРѕСЂС‚ picker-Р° РєР°СЂС‚РёРЅРѕРә вЂ”
    // С‚Рµ Р¶Рµ, СЃ РєР°РєРёРјРё СЃС‚СЂР°РЅРёС†Р° РіСЂСѓР·РёС‚ СЃРІРѕРё РїРѕРґСЂРµСЃСѓСЂСЃС‹ (print-РіРµР№С‚
    // С„СЂРµР№РјР°Рј РЅРµ РЅСѓР¶РµРЅ — РїРµС‡Р°С‚СЊ PDF РїРѕРґ-РґРѕРєСѓРјРµРЅС‚РѕРІ РІРЅРµ СЃСЂРµР·Р°).
    let mut frames = {
        let _s = lumen_core::trace::span("fetch-iframes", "net");
        load_frame_sub_documents(
            &doc_arc,
            0,
            base,
            &doc_arc,
            base,
            &screen_media_context(viewport, dark_mode),
            viewport,
            sink,
            frame_cookie_jar,
            frame_fp,
            frame_wp,
            frame_sp,
            frame_ls,
            frame_ss,
            frame_idb,
            frame_sw,
            frame_sww,
            frame_cache,
            cookie_banner_dismiss,
            deterministic,
            cross_origin_isolated,
            js_ctx.as_ref(),
            target,
        )
    };

    // Fetch + decode <img src>. Р”РѕР»Р¶РЅРѕ РёРґС‚Рё Р”Рћ layout, РїРѕС‚РѕРјСѓ С‡С‚Рѕ intrinsic
    // dimensions РёР· РґРµРєРѕРґРёСЂРѕРІР°РЅРЅРѕРіРѕ РёР·РѕР±СЂР°Р¶РµРЅРёСЏ РїСЂРѕСЃС‚Р°РІР»СЏСЋС‚СЃСЏ РєР°Рє HTML
    // presentational hints (width/height attribute) Рё РїРѕС‚РѕРј РїРѕРґС…РІР°С‚С‹РІР°СЋС‚СЃСЏ
    // style cascade. Errors silently РїСЂРѕРїСѓСЃРєР°СЋС‚СЃСЏ вЂ” Р±РёС‚Р°СЏ РєР°СЂС‚РёРЅРєР° РЅРµ РІР°Р»РёС‚
    // РІСЃСЋ СЃС‚СЂР°РЅРёС†Сѓ, layout РЅР°СЂРёСЃСѓРµС‚ СЃРµСЂС‹Р№ placeholder.
    // loading="lazy" РёР·РѕР±СЂР°Р¶РµРЅРёСЏ РІРѕР·РІСЂР°С‰Р°СЋС‚СЃСЏ РІ lazy_pairs Рё РЅРµ Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ СЃРµР№С‡Р°СЃ.
    let (images, animated_gifs, lazy_pairs) = {
        let _s = lumen_core::trace::span("fetch-images", "net");
        let mut d = doc_arc.lock().unwrap();
        fetch_and_decode_images(&mut d, base, sink, viewport, cookie_jar.clone(), target)
    };

    // P3-webvtt СЃСЂРµР· 3: Р·Р°РіСЂСѓР·РєР° WebVTT-СЃСѓР±С‚РёС‚СЂРѕРІ РёР· <track> РєР°Р¶РґРѕРіРѕ <video>.
    // РћС€РёР±РєРё С„РµС‚С‡Р°/РїР°СЂСЃРёРЅРіР° РЅРµ РІР°Р»СЏС‚ СЃС‚СЂР°РЅРёС†Сѓ вЂ” РІРёРґРµРѕ РїСЂРѕСЃС‚Рѕ РѕСЃС‚Р°С‘С‚СЃСЏ Р±РµР· cues.
    let page_tracks = {
        let d = doc_arc.lock().unwrap();
        tracks::load_video_tracks(&d, &|src| {
            fetch_vtt_text(src, base, sink, cookie_jar.clone())
        })
    };

    // Register decoded <img> bitmaps with the JS runtime so Canvas 2D
    // drawImage(imgElement, вЂ¦) can read the pixels. Collect nidв†’url from DOM
    // (same traversal fetch_and_decode_images used), join with decoded images by
    // URL, and share the decoded `Arc<Image>` into img_bitmap_store on the JS thread.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx {
        let img_reqs = {
            let d = doc_arc.lock().unwrap();
            lumen_layout::collect_image_requests(&d, viewport)
        };
        // BUG-272 СЃСЂРµР· 20: share the decoded `Arc<Image>` with the JS canvas
        // drawImage store instead of eagerly copying an RGBA8 buffer per image.
        // The store converts to RGBA8 lazily, only for images a canvas actually
        // draws вЂ” images never used as a drawImage source cost zero extra bytes.
        let url_to_img: std::collections::HashMap<&str, &std::sync::Arc<lumen_image::Image>> =
            images.iter().map(|(url, img)| (url.as_str(), img)).collect();
        let bitmaps: Vec<(u32, std::sync::Arc<lumen_image::Image>)> = img_reqs
            .iter()
            .filter_map(|req| {
                let img = url_to_img.get(req.url.as_str())?;
                Some((req.node_id.index() as u32, std::sync::Arc::clone(img)))
            })
            .collect();
        if !bitmaps.is_empty() {
            js.register_img_bitmaps(bitmaps);
        }
    }

    // Р’СЃС‚СЂРѕРµРЅРЅС‹Рµ <style> + РІРЅРµС€РЅРёРµ <link rel=stylesheet>.
    let (css, dynamic_css, link_outcomes) = {
        let _s = lumen_core::trace::span("fetch-css", "net");
        let d = doc_arc.lock().unwrap();
        let link_media_ctx = if media_print {
            print_media_context(viewport, dark_mode)
        } else {
            screen_media_context(viewport, dark_mode)
        };
        // РРЅР»Р°Р№РЅРѕРІС‹Рµ <style>: РёС… `@import` СЂРµР·РѕР»РІСЏС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ Р±Р°Р·С‹
        // РґРѕРєСѓРјРµРЅС‚Р° (CSS-SPECS В§@import). Р’РЅРµС€РЅРёРµ <link> СЂРµР·РѕР»РІСЏС‚ СЃРѕР±СЃС‚РІРµРЅРЅС‹Рµ
        // `@import` РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ СЃРІРѕРµРіРѕ URL РІРЅСѓС‚СЂРё load_linked_stylesheets.
        let inline = extract_style_blocks(&d);
        let mut css = inline_css_imports(
            &inline,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
            &mut std::collections::HashSet::new(),
            0,
        );
        // BUG-743: РІСЃС‘, С‡С‚Рѕ РЅРµ РїСЂРёС€Р»Рѕ РёР· РёРЅР»Р°Р№РЅРѕРІС‹С… <style>, РѕС‚РєР»Р°РґС‹РІР°РµС‚СЃСЏ
        // РѕС‚РґРµР»СЊРЅРѕ вЂ” С‚Р°Рє РїРѕР·РґРЅРёР№ РґРёРЅР°РјРёС‡РµСЃРєРёР№ <style> РїРµСЂРµСЃРѕР±РёСЂР°РµС‚ РєР°СЃРєР°Рґ Р±РµР·
        // РµРґРёРЅРѕРіРѕ СЃРµС‚РµРІРѕРіРѕ Р·Р°РїСЂРѕСЃР°. `inline_css_imports` РІРѕР·РІСЂР°С‰Р°РµС‚
        // `<РёРјРїРѕСЂС‚С‹> + <РёСЃС…РѕРґРЅС‹Р№ С‚РµРєСЃС‚>`, РїРѕСЌС‚РѕРјСѓ РїСЂРµС„РёРєСЃ = РІСЃС‘ РґРѕ С…РІРѕСЃС‚Р°.
        let imports_prefix = css[..css.len() - inline.len()].to_owned();
        let (linked, link_outcomes) = load_linked_stylesheets(
            &d,
            base,
            sink,
            cookie_jar.clone(),
            &link_media_ctx,
        );
        css.push_str(&linked);
        let dyn_css = DynamicCssBase {
            imports_prefix,
            linked,
            inline_fp: inline_style_fingerprint(&d),
        };
        (css, dyn_css, link_outcomes)
    };

    // BUG-804: HTML LS В§4.6.7 В«process the linked resourceВ» вЂ” РєР°Р¶РґС‹Р№
    // `<link rel=stylesheet>` РѕР±СЏР·Р°РЅ СЃРѕРѕР±С‰РёС‚СЊ СЃС‚СЂР°РЅРёС†Рµ `load` РёР»Рё `error`.
    // РћС‚С‡С‘С‚ СѓС…РѕРґРёС‚ РѕС‚СЃСЋРґР°, Р° РЅРµ РёР· С€РёРјР°: Р»РёСЃС‚ РіСЂСѓР·РёС‚ РїСЂРѕС…РѕРґ РІС‹С€Рµ, Рё С‚РѕР»СЊРєРѕ РѕРЅ
    // Р·РЅР°РµС‚ РёСЃС…РѕРґ вЂ” РїРѕРІС‚РѕСЂРЅС‹Р№ С„РµС‚С‡ РёР· JS РґР°Р» Р±С‹ РІС‚РѕСЂРѕР№ Р·Р°РїСЂРѕСЃ Рё РІСЃС‘ СЂР°РІРЅРѕ РЅРµ
    // РѕС‚Р»РёС‡РёР» Р±С‹ В«Р»РёСЃС‚ РІ РєР°СЃРєР°РґРµВ» РѕС‚ В«Р±Р°Р№С‚С‹ РїСЂРёС€Р»РёВ». Р­Р»РµРјРµРЅС‚, РєРѕС‚РѕСЂС‹Р№ СѓР¶Рµ
    // РѕС‚С‡РёС‚Р°Р»СЃСЏ СЃР°Рј (РІСЃС‚Р°РІР»РµРЅРЅС‹Р№ СЃРєСЂРёРїС‚РѕРј вЂ” РѕРЅ РїСЂРѕС…РѕРґРёС‚ С‡РµСЂРµР·
    // `_lumen_link_prepare` Р•Р©РЃ Р”Рћ СЌС‚РѕРіРѕ РїСЂРѕС…РѕРґР°, СЃРєСЂРёРїС‚С‹ РІС‹РїРѕР»РЅСЏСЋС‚СЃСЏ СЂР°РЅСЊС€Рµ),
    // РѕС‚СЃРµРєР°РµС‚СЃСЏ РѕР±С‰РёРј РїРµСЂ-СѓР·Р»РѕРІС‹Рј С„Р»Р°РіРѕРј РЅР° JS-СЃС‚РѕСЂРѕРЅРµ.
    #[cfg(feature = "v8")]
    if let Some(js) = &js_ctx
        && !link_outcomes.is_empty()
    {
        use std::fmt::Write as _;
        let mut arg = String::with_capacity(link_outcomes.len() * 8 + 40);
        arg.push_str("_lumen_deliver_parser_link_events([");
        for (i, (node, ok)) in link_outcomes.iter().enumerate() {
            if i > 0 {
                arg.push(',');
            }
            let _ = write!(arg, "{},{}", node.index(), u8::from(*ok));
        }
        arg.push_str("]);");
        js.eval_js(&arg);
    }

    let sheet = {
        let _s = lumen_core::trace::span("parse-css", "parse");
        lumen_css_parser::parse(&css)
    };

    // PH3-19: @font-face Р·Р°РіСЂСѓР·РєР° СЂР°Р·РґРµР»РµРЅР° РЅР° РґРІР° РїСЂРѕС…РѕРґР°.
    // local()-РёСЃС‚РѕС‡РЅРёРєРё Р·Р°РіСЂСѓР¶Р°СЋС‚СЃСЏ СЃРёРЅС…СЂРѕРЅРЅРѕ (РёР· СЃРёСЃС‚РµРјРЅРѕРіРѕ РёРЅРґРµРєСЃР°, Р±С‹СЃС‚СЂРѕ).
    // url()-РёСЃС‚РѕС‡РЅРёРєРё вЂ” С‚РѕР»СЊРєРѕ СЃРѕР±РёСЂР°РµРј РІ pending_web_fonts; С„РѕРЅРѕРІС‹Р№ РїРѕС‚РѕРє
    // fetch+decode СЃРїР°РІРЅРёС‚СЃСЏ РІ apply_loaded_page в†’ РїРµСЂРІС‹Р№ paint РЅРµ Р¶РґС‘С‚ СЃРµС‚Рё.
    let (font_registry, pending_web_fonts) = {
        // PERF-12: this stretch вЂ” @font-face resolution through to the measurer's
        // system faces below вЂ” was the single largest unnamed hole in the
        // `--trace-nav` waterfall (114 ms of a 128 ms `navigation` on
        // samples/page.html, against a `layout` span of 0.6 ms). It is dominated
        // by the lazy system-font index build that PERF-11 caches.
        let _s = lumen_core::trace::span("font-faces", "font");
        load_font_faces(&sheet.font_faces, base, sink, cookie_jar.clone())
    };

    // Populate document.fonts with FontFace objects from @font-face rules.
    // local() вЂ” immediately Loaded; url() вЂ” Loading (Р±СѓРґРµС‚ Loaded РїРѕ FontLoaded).
    {
        let mut d = doc_arc.lock().unwrap();
        for rule in &sheet.font_faces {
            let mut font_face = rule_to_font_face(rule);
            // local() rules already resolved вЂ” mark Loaded; url() rules stay Loading.
            let has_local = rule.sources.iter().any(|s| {
                s.kind == lumen_css_parser::FontFaceSourceKind::Local
                    && font_registry.face_bytes_for_family(&rule.family).is_some()
            });
            if has_local {
                font_face.status = lumen_dom::FontFaceStatus::Loaded;
            }
            d.fonts_mut().add(font_face);
        }
    }

    let font = lumen_font::Font::parse(INTER_FONT)
        .map_err(|e| format!("РѕС€РёР±РєР° СЂР°Р·Р±РѕСЂР° С€СЂРёС„С‚Р°: {e}"))?;
    // РњРЅРѕРіРѕС€СЂРёС„С‚РѕРІС‹Р№ РёР·РјРµСЂРёС‚РµР»СЊ: Inter РєР°Рє fallback + СѓР¶Рµ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Рµ local()-СЃРµРјСЊРё.
    // url()-СЃРµРјСЊРё РґРѕР±Р°РІСЏС‚СЃСЏ РїРѕР·Р¶Рµ С‡РµСЂРµР· FontLoaded + relayout_with_web_fonts.
    let mut measurer = lumen_paint::MultiFontMeasurer::new(&font)
        .map_err(|e| format!("РѕС€РёР±РєР° РјРµС‚СЂРёРє С€СЂРёС„С‚Р°: {e}"))?;
    // BUG-128: СЃРёСЃС‚РµРјРЅС‹Рµ face-С‹ вЂ” С‚Рµ Р¶Рµ, С‡С‚Рѕ РІС‹Р±РµСЂРµС‚ СЂРµРЅРґРµСЂ.
    {
        // PERF-11/PERF-12: `system_font_faces()` is where the lazy system font
        // index is built on first use вЂ” hundreds of files parsed, once per
        // process. Named separately from `font-faces` so the trace attributes
        // the cost to the index rather than to @font-face handling.
        let _s = lumen_core::trace::span("system-fonts", "font");
        measurer.set_system_faces(system_font_faces());
    }
    for rule in &sheet.font_faces {
        if !rule.family.is_empty()
            && let Some(bytes) = font_registry.face_bytes_for_family(&rule.family)
        {
            // CSS Fonts L4 В§5.1: РїРµСЂРµРґР°С‘Рј unicode-range РёР· @font-face РґРµСЃРєСЂРёРїС‚РѕСЂР°.
            let ranges = rule.unicode_range.as_deref()
                .map(lumen_font::parse_unicode_ranges)
                .unwrap_or_default();
            measurer.register_family_with_ranges(&rule.family, bytes, ranges);
        }
    }
    let font_provider = Arc::new(font_registry);

    // BUG-270: РїРµС‡Р°С‚СЊ РІ PDF С„РёР»СЊС‚СЂСѓРµС‚ РєР°СЃРєР°Рґ РїРѕ media_type="print" С‡РµСЂРµР·
    // sticky thread-local. Р¤Р»Р°Рі per-pass, РїРѕСЌС‚РѕРјСѓ СЃР±СЂР°СЃС‹РІР°РµРј СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ layout,
    // С‡С‚РѕР±С‹ РїРѕСЃР»РµРґСѓСЋС‰РёРµ СЌРєСЂР°РЅРЅС‹Рµ РїСЂРѕС…РѕРґС‹ РЅР° СЌС‚РѕРј Р¶Рµ РїРѕС‚РѕРєРµ РЅРµ РЅР°СЃР»РµРґРѕРІР°Р»Рё print.
    lumen_layout::set_print_media(media_print);
    let layout = {
        let _s = lumen_core::trace::span("layout", "layout");
        let d = doc_arc.lock().unwrap();
        lumen_layout::layout_measured_hyp(&d, &sheet, viewport, &measurer, hp, dark_mode)
    };
    lumen_layout::set_print_media(false);

    // BUG-480 срез 13: размер host-бокса каждого `<iframe>` известен только
    // теперь — пересчитываем layout под-документов под него (срез 12 считал их
    // на UA-дефолтных 300×150, потому что шёл до этой строки).
    crate::frames::sync_frame_viewports(&mut frames, &layout);

    // CSS Backgrounds L3 В§3.10 вЂ” СЃРѕР±РёСЂР°РµРј `background-image: url(...)` СѓР¶Рµ
    // РїРѕСЃР»Рµ layout-Р° (РєР°СЂС‚РёРЅРєРё С„РѕРЅР° РЅРµ РІР»РёСЏСЋС‚ РЅР° СЂР°СЃС‡С‘С‚ РєРѕСЂРѕР±РѕРє). Р”РµРєРѕРґРёСЂСѓРµРј
    // Рё РґРѕР±Р°РІР»СЏРµРј Рє `images` С‚РµРј Р¶Рµ РєР»СЋС‡РѕРј, С‡С‚Рѕ СЌРјРёС‚С‚РµСЂ РєР»Р°РґС‘С‚ РІ
    // `DisplayCommand::DrawBackgroundImage.src`.
    let mut images = images;
    {
        let _s = lumen_core::trace::span("fetch-bg-images", "net");
        for (src, image) in fetch_and_decode_background_images(&layout, base, sink, cookie_jar.clone(), target) {
            images.push((src, image));
        }
    }
    // BUG-480 срез 15: картинки под-документов фреймов едут в ОБЩИЙ список
    // страницы. Их ключи разрешены относительно базы ребёнка
    // (`frames::frame_image_key`), поэтому со своими ключами страницы они не
    // сталкиваются, а все существующие точки регистрации (`apply_loaded_page`,
    // `reload`, `pending_images`, CPU-кэш снимков) подхватывают их без правок.
    for h in &frames {
        images.extend(h.images.iter().map(|(k, i)| (k.clone(), Arc::clone(i))));
    }

    let rule_count = sheet.rules.len();
    Ok(ParsedPage {
        document: doc_arc,
        stylesheet: sheet,
        layout,
        title,
        rule_count,
        images,
        animated_gifs,
        lazy_pairs,
        preload_hints,
        html_source: source,
        font_registry: font_provider,
        pending_web_fonts,
        js_navigate: js_nav,
        js_ctx,
        page_tracks,
        dynamic_css,
        frames,
    })
}

/// Р“РѕС‚РѕРІС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ С„РёРЅР°Р»СЊРЅРѕРіРѕ pipeline: display-list-СЃС‚СЂР°РЅРёС†Р°, РёСЃС‚РѕС‡РЅРёРє РґР»СЏ
/// relayout Рё Р¶РёРІРѕР№ JS-С…СЌРЅРґР» (РµСЃР»Рё РІРєР»СЋС‡С‘РЅ QuickJS). РўРёРї-Р°Р»РёР°СЃ, С‡С‚РѕР±С‹ РІС‹РЅРµСЃС‚Рё
/// СЃР»РѕР¶РЅСѓСЋ С‚СЂРѕР№РєСѓ РёР· СЃРёРіРЅР°С‚СѓСЂ (`render_bytes`, `RenderOutcome`).
pub(crate) type RenderedPage = (LoadedPage, LayoutSource, Option<Arc<dyn PersistentJs>>);

/// BUG-171 СЌС‚Р°Рї 2: СЂРµР·СѓР»СЊС‚Р°С‚ С„РёРЅР°Р»СЊРЅРѕРіРѕ off-UI-thread СЂРµРЅРґРµСЂР° (`render_bytes`),
/// РїРµСЂРµСЃС‹Р»Р°РµРјС‹Р№ РЅР°Р·Р°Рґ РЅР° UI-РїРѕС‚РѕРє С‡РµСЂРµР· `LoadEvent::RenderDone`.
///
/// Р’СЃРµ РїРѕР»СЏ `Send`: `LoadedPage`/`LayoutSource` вЂ” РѕР±С‹С‡РЅС‹Рµ РґР°РЅРЅС‹Рµ; `js_ctx` вЂ”
/// С…СЌРЅРґР» QuickJS (`Send + Sync` РїРѕ ADR-014, СЃРѕР·РґР°РЅ РЅР° СЂРµРЅРґРµСЂ-РїРѕС‚РѕРєРµ);
/// `preload_dispatched` РІСЂРµРјРµРЅРЅРѕ Р·Р°Р±СЂР°РЅ РёР· `Lumen` РЅР° РІСЂРµРјСЏ СЂРµРЅРґРµСЂР° (РѕРЅ РµРіРѕ
/// РґРµРґСѓРїР»РёС†РёСЂСѓРµС‚) Рё РІРѕР·РІСЂР°С‰Р°РµС‚СЃСЏ РґР»СЏ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёСЏ.
pub(crate) struct RenderOutcome {
    /// Р“РѕС‚РѕРІР°СЏ СЃС‚СЂР°РЅРёС†Р° + РёСЃС‚РѕС‡РЅРёРє layout + Р¶РёРІРѕР№ JS-С…СЌРЅРґР»; Р»РёР±Рѕ С‚РµРєСЃС‚ РѕС€РёР±РєРё
    /// (`Box<dyn Error>` РЅРµ `Send`, РїРѕСЌС‚РѕРјСѓ РєРѕРЅРІРµСЂС‚РёСЂСѓРµС‚СЃСЏ РІ `String`).
    pub(crate) result: Result<RenderedPage, String>,
    /// РќР°Р±РѕСЂ СѓР¶Рµ СЂР°Р·РѕСЃР»Р°РЅРЅС‹С… preload-С…РёРЅС‚РѕРІ, Р·Р°Р±СЂР°РЅРЅС‹Р№ РёР·
    /// `Lumen::preload_dispatched` РЅР° РІСЂРµРјСЏ СЂРµРЅРґРµСЂР°.
    pub(crate) preload_dispatched: std::collections::HashSet<String>,
}
