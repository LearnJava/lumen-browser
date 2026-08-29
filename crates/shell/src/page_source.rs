//! Where a document's bytes come from: the [`PageSource`] enum, its loading
//! methods, the [`RawPage`] they return, and the two helpers that turn an
//! automation- or JS-supplied URL string into a `PageSource`.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3b); behaviour and
//! signatures are unchanged.

use crate::*;

/// РСЃС‚РѕС‡РЅРёРє СЃС‚СЂР°РЅРёС†С‹. Р—Р°РїРѕРјРёРЅР°РµС‚СЃСЏ РІ `Lumen`, С‡С‚РѕР±С‹ reload (F5/Ctrl+R) РјРѕРі
/// Р·Р°РЅРѕРІРѕ РІС‹РїРѕР»РЅРёС‚СЊ fetch/parse/layout/paint Р±РµР· Р°СЂРіСѓРјРµРЅС‚РѕРІ РєРѕРјР°РЅРґРЅРѕР№ СЃС‚СЂРѕРєРё.
#[derive(Debug, Clone)]
pub(crate) enum PageSource {
    /// Р‘РµР· Р°СЂРіСѓРјРµРЅС‚РѕРІ вЂ” СЂРёСЃСѓРµРј РїСѓСЃС‚РѕРµ РѕРєРЅРѕ. Reload no-op (РіСЂСѓР·РёС‚СЊ РЅРµС‡РµРіРѕ).
    Empty,
    File(PathBuf),
    Url(String),
    /// `about:blank` вЂ” РїСѓСЃС‚РѕР№ РґРѕРєСѓРјРµРЅС‚ Р±РµР· СЃРµС‚РµРІРѕРіРѕ Р·Р°РїСЂРѕСЃР° (HTML spec В§7.5).
    /// `url_str()` РІРѕР·РІСЂР°С‰Р°РµС‚ "about:blank" РґР»СЏ Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРё Рё РёСЃС‚РѕСЂРёРё.
    AboutBlank,
    /// РЎС‚СЂР°РЅРёС†Р° РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°РµС‚СЃСЏ РёР· bfcache: HTML СѓР¶Рµ РµСЃС‚СЊ РІ РїР°РјСЏС‚Рё,
    /// СЃРµС‚РµРІРѕР№ Р·Р°РїСЂРѕСЃ РЅРµ РЅСѓР¶РµРЅ. `base_url` вЂ” РѕСЂРёРіРёРЅР°Р»СЊРЅС‹Р№ URL СЃС‚СЂР°РЅРёС†С‹
    /// (РґР»СЏ СЂР°Р·СЂРµС€РµРЅРёСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… СЃСЃС‹Р»РѕРє РІРЅСѓС‚СЂРё HTML).
    Snapshot { html: String, base_url: String },
    /// Р’РЅСѓС‚СЂРµРЅРЅСЏСЏ СЃС‚Р°С‚РёС‡РµСЃРєР°СЏ СЃС‚СЂР°РЅРёС†Р° (`about:newtab`): HTML РіРµРЅРµСЂРёСЂСѓРµС‚СЃСЏ
    /// РІ РїР°РјСЏС‚Рё, СЃРµС‚РµРІРѕР№ Р·Р°РїСЂРѕСЃ РЅРµ РЅСѓР¶РµРЅ. `url` вЂ” РєР°РЅРѕРЅРёС‡РµСЃРєРёР№ about-URL,
    /// РїРѕРєР°Р·С‹РІР°РµС‚СЃСЏ РІ Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРµ Рё РёСЃС‚РѕСЂРёРё.
    Static { html: String, url: String },
}

impl PageSource {
    pub(crate) fn from_arg(arg: Option<&str>) -> Self {
        match arg {
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                PageSource::Url(s.to_owned())
            }
            Some("about:blank") => PageSource::AboutBlank,
            Some(s) if s == chrome_preview::URL => PageSource::Static {
                html: chrome_preview::HTML.to_owned(),
                url: chrome_preview::URL.to_owned(),
            },
            Some(s) => PageSource::File(PathBuf::from(s)),
            None => PageSource::Empty,
        }
    }

    pub(crate) fn describe(&self) -> String {
        match self {
            PageSource::Empty => "(РїСѓСЃС‚Р°СЏ РІРєР»Р°РґРєР°)".to_owned(),
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::AboutBlank => "about:blank".to_owned(),
            PageSource::Snapshot { base_url, .. } => format!("[bfcache] {base_url}"),
            PageSource::Static { url, .. } => url.clone(),
        }
    }

    /// Origin string (scheme+host+port) for localStorage partitioning.
    /// Returns `None` for file: and empty sources (no cross-origin storage needed).
    pub(crate) fn origin_str(&self) -> Option<String> {
        let url_s = match self {
            PageSource::Url(u) => u.as_str(),
            PageSource::Snapshot { base_url, .. } => base_url.as_str(),
                _ => return None,
        };
        lumen_core::url::Url::parse(url_s).ok().map(|u| {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host(), port)
        })
    }

    /// URL-СЃС‚СЂРѕРєР° СЃС‚СЂР°РЅРёС†С‹ РґР»СЏ bfcache-РєР»СЋС‡Р°. `None` РµСЃР»Рё РЅРµС‚ URL (РїСѓСЃС‚Р°СЏ РІРєР»Р°РґРєР°, С„Р°Р№Р»).
    pub(crate) fn url_str(&self) -> Option<&str> {
        match self {
            PageSource::Url(u) => Some(u.as_str()),
            PageSource::Snapshot { base_url, .. } => Some(base_url.as_str()),
            PageSource::AboutBlank => Some("about:blank"),
            PageSource::Static { url, .. } => Some(url.as_str()),
            _ => None,
        }
    }

    /// Base URL/path used to resolve this page's subresources (images, CSS).
    /// `None` for sources without a base (`Empty`/`AboutBlank`/`Static`).
    pub(crate) fn resource_base(&self) -> Option<ResourceBase> {
        match self {
            PageSource::File(p) => Some(ResourceBase::File(p.clone())),
            PageSource::Url(u) => Some(ResourceBase::Url(u.clone())),
            PageSource::Snapshot { base_url, .. } => Some(ResourceBase::Url(base_url.clone())),
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => None,
        }
    }

    /// Resolve a relative or absolute `href` against this page's base URL/path.
    /// Returns the resolved string (absolute URL or absolute file path string).
    /// Falls back to the raw `href` when the base is `Empty` or resolution fails.
    pub(crate) fn resolve_href(&self, href: &str) -> String {
        match self.resource_base() {
            Some(base) => base.resolve_str(href),
            None => href.to_owned(),
        }
    }

    /// РџСЂРѕС‡РёС‚Р°С‚СЊ Р±Р°Р№С‚С‹ СЃС‚СЂР°РЅРёС†С‹ СЃ РґРёСЃРєР° РёР»Рё РёР· СЃРµС‚Рё, РїР»СЋСЃ РІРµСЂРЅСѓС‚СЊ Р±Р°Р·Сѓ РґР»СЏ
    /// РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… URL Рё РїРѕРґСЃРєР°Р·РєСѓ Рѕ content-type. РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ Рё РѕР±С‹С‡РЅС‹Рј
    /// `load`, Рё dump-СЂРµР¶РёРјР°РјРё.
    pub(crate) fn load_bytes(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    ) -> Result<RawPage, Box<dyn Error>> {
        match self {
            PageSource::Empty => Err("РёСЃС‚РѕС‡РЅРёРє РїСѓСЃС‚ вЂ” РЅРµС‡РµРіРѕ Р·Р°РіСЂСѓР¶Р°С‚СЊ".into()),
            PageSource::AboutBlank => Ok(RawPage {
                bytes: b"<!DOCTYPE html><html><head></head><body></body></html>".to_vec(),
                base: ResourceBase::Url("about:blank".to_owned()),
                content_type: Some("text/html"),
                cross_origin_isolated: false,
                cache_control_no_store: false,
            }),
            PageSource::File(path) => {
                let bytes = std::fs::read(path)?;
                Ok(RawPage {
                    bytes,
                    base: ResourceBase::File(path.clone()),
                    content_type: None,
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                })
            }
            PageSource::Url(url) => {
                use lumen_core::url::Url;
                use lumen_network::{
                    BrotliContentDecoder, DeflateContentDecoder, GzipContentDecoder, HttpClient,
                };

                let lumen_url = Url::parse(url)?;
                let mut builder = HttpClient::new()
                    .with_sink(sink)
                    .with_content_decoder(std::sync::Arc::new(BrotliContentDecoder::new()))
                    .with_content_decoder(std::sync::Arc::new(GzipContentDecoder::new()))
                    .with_content_decoder(std::sync::Arc::new(DeflateContentDecoder::new()));
                if let Some(jar) = cookie_jar {
                    builder = builder.with_cookie_jar(
                        Arc::new(lumen_storage::CookieJarProvider::new(jar)),
                        None,
                    );
                }
                let client = crate::config::global().apply_http(builder);
                // PERF-1: HTTP request for the main document (nested inside the
                // `fetch-document` span); its `size` arg is the response body.
                let mut fetch_span = lumen_core::trace::span(format!("GET {url}"), "net");
                let lumen_network::PageResponse { body: bytes, headers: resp_headers, final_url } =
                    client.fetch_page(&lumen_url)?;
                fetch_span.set_bytes(bytes.len());
                eprintln!("РџРѕР»СѓС‡РµРЅРѕ {} Р±Р°Р№С‚", bytes.len());
                let coop = resp_headers.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-opener-policy"))
                    .map(|(_, v)| v.as_str());
                let coep = resp_headers.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-embedder-policy"))
                    .map(|(_, v)| v.as_str());
                let cross_origin_isolated = lumen_network::CrossOriginIsolationState::from_headers(coop, coep).is_cross_origin_isolated();
                Ok(RawPage {
                    bytes,
                    // BUG-757: Р±Р°Р·Р° РґРѕРєСѓРјРµРЅС‚Р° вЂ” Р°РґСЂРµСЃ, СЃ РєРѕС‚РѕСЂРѕРіРѕ РїСЂРёС€С‘Р»
                    // С„РёРЅР°Р»СЊРЅС‹Р№ РѕС‚РІРµС‚, Р° РЅРµ Р°СЂРіСѓРјРµРЅС‚ РЅР°РІРёРіР°С†РёРё. РџРѕСЃР»Рµ
                    // СЃРµСЂРІРµСЂРЅРѕРіРѕ СЂРµРґРёСЂРµРєС‚Р° РѕРЅРё СЂР°Р·РЅС‹Рµ, Рё РѕС‚ Р±Р°Р·С‹ Р·Р°РІРёСЃСЏС‚
                    // `location.*`/`document.baseURI`, СЂР°Р·СЂРµС€РµРЅРёРµ
                    // РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ Рё origin С…СЂР°РЅРёР»РёС‰.
                    base: ResourceBase::Url(final_url.to_string()),
                    content_type: Some("text/html"),
                    cross_origin_isolated,
                    cache_control_no_store: cache_control_no_store(&resp_headers),
                })
            }
            PageSource::Snapshot { html, base_url } => {
                // bfcache restoration: HTML already in memory, no network request.
                Ok(RawPage {
                    bytes: html.as_bytes().to_vec(),
                    base: ResourceBase::Url(base_url.clone()),
                    content_type: Some("text/html"),
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                })
            }
            PageSource::Static { html, url } => {
                // Internal about: page: HTML generated in memory, no network request.
                Ok(RawPage {
                    bytes: html.as_bytes().to_vec(),
                    base: ResourceBase::Url(url.clone()),
                    content_type: Some("text/html"),
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                })
            }
        }
    }

    /// РљР°Рє `load_bytes`, РЅРѕ РґР»СЏ СЃРµС‚РµРІС‹С… (URL) РёСЃС‚РѕС‡РЅРёРєРѕРІ С‚РµР»Рѕ С„РёРЅР°Р»СЊРЅРѕРіРѕ
    /// 2xx-РѕС‚РІРµС‚Р° СЃС‚СЂРёРјРёС‚СЃСЏ: РєР°Р¶РґР°СЏ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅР°СЏ РїРѕСЂС†РёСЏ РїРµСЂРµРґР°С‘С‚СЃСЏ РІ
    /// `on_chunk` РµС‰С‘ РґРѕ РїРѕР»РЅРѕРіРѕ СЃРєР°С‡РёРІР°РЅРёСЏ (PH1-2a). Р”Р»СЏ РЅРµСЃРµС‚РµРІС‹С… РёСЃС‚РѕС‡РЅРёРєРѕРІ
    /// (File/Snapshot/Static) РґРµР»РµРіРёСЂСѓРµС‚ РІ `load_bytes` Р±РµР· РІС‹Р·РѕРІРѕРІ `on_chunk`
    /// вЂ” caller СЃР°Рј РЅР°СЂРµР¶РµС‚ СѓР¶Рµ-Р·Р°РіСЂСѓР¶РµРЅРЅРѕРµ С‚РµР»Рѕ. Р’РѕР·РІСЂР°С‰Р°РµРјС‹Р№ `RawPage.bytes`
    /// вЂ” РїРѕР»РЅРѕРµ РґРµРєРѕРґРёСЂРѕРІР°РЅРЅРѕРµ С‚РµР»Рѕ (РєР°Рє Сѓ `load_bytes`).
    ///
    /// Р’С‚РѕСЂРѕР№ Р°СЂРіСѓРјРµРЅС‚ `on_chunk` вЂ” URL, СЃ РєРѕС‚РѕСЂРѕРіРѕ С‚РµС‡С‘С‚ С‚РµР»Рѕ: РїРѕСЃР»Рµ
    /// СЂРµРґРёСЂРµРєС‚Р° РѕРЅ РѕС‚Р»РёС‡Р°РµС‚СЃСЏ РѕС‚ Р·Р°РїСЂРѕС€РµРЅРЅРѕРіРѕ, Рё РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹Рµ СЃСЃС‹Р»РєРё РІ
    /// РїРѕС‚РѕРєРµ (preload-С…РёРЅС‚С‹) РѕР±СЏР·Р°РЅС‹ СЂРµР·РѕР»РІРёС‚СЊСЃСЏ РёРјРµРЅРЅРѕ РѕС‚ РЅРµРіРѕ (BUG-757).
    pub(crate) fn load_bytes_streaming(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
        on_chunk: &mut dyn FnMut(&[u8], &lumen_core::url::Url),
    ) -> Result<RawPage, Box<dyn Error>> {
        let PageSource::Url(url) = self else {
            return self.load_bytes(sink, cookie_jar);
        };
        use lumen_core::url::Url;
        use lumen_network::{
            BrotliContentDecoder, DeflateContentDecoder, GzipContentDecoder, HttpClient,
        };

        let lumen_url = Url::parse(url)?;
        let mut builder = HttpClient::new()
            .with_sink(sink)
            .with_content_decoder(std::sync::Arc::new(BrotliContentDecoder::new()))
            .with_content_decoder(std::sync::Arc::new(GzipContentDecoder::new()))
            .with_content_decoder(std::sync::Arc::new(DeflateContentDecoder::new()));
        if let Some(jar) = cookie_jar {
            builder = builder.with_cookie_jar(
                Arc::new(lumen_storage::CookieJarProvider::new(jar)),
                None,
            );
        }
        let client = crate::config::global().apply_http(builder);
        let lumen_network::PageResponse { body: bytes, headers: resp_headers, final_url } =
            client.fetch_page_streaming(&lumen_url, on_chunk)?;
        eprintln!("РџРѕР»СѓС‡РµРЅРѕ {} Р±Р°Р№С‚ (streaming)", bytes.len());
        let coop = resp_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-opener-policy"))
            .map(|(_, v)| v.as_str());
        let coep = resp_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-embedder-policy"))
            .map(|(_, v)| v.as_str());
        let cross_origin_isolated = lumen_network::CrossOriginIsolationState::from_headers(coop, coep)
            .is_cross_origin_isolated();
        Ok(RawPage {
            // BUG-757: СЃРј. `load_bytes` вЂ” Р±Р°Р·Р° РёР· С„РёРЅР°Р»СЊРЅРѕРіРѕ URL РѕС‚РІРµС‚Р°. РўРѕС‚ Р¶Рµ
            // Р°РґСЂРµСЃ РїСЂРёС…РѕРґРёС‚ Рё РІ `on_chunk` (РІС‚РѕСЂС‹Рј Р°СЂРіСѓРјРµРЅС‚РѕРј), РїРѕСЌС‚РѕРјСѓ
            // preload-С…РёРЅС‚С‹ РёР· РїРѕС‚РѕРєР° СЂРµР·РѕР»РІСЏС‚СЃСЏ РѕС‚ С‚РѕР№ Р¶Рµ Р±Р°Р·С‹, С‡С‚Рѕ РґРѕРєСѓРјРµРЅС‚.
            base: ResourceBase::Url(final_url.to_string()),
            bytes,
            content_type: Some("text/html"),
            cross_origin_isolated,
            cache_control_no_store: cache_control_no_store(&resp_headers),
        })
    }

    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    pub(crate) fn load(
        &self,
        sink: Arc<dyn EventSink>,
        viewport: Size,
        ls_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
        ss_store: Option<Arc<std::sync::Mutex<lumen_core::WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
        hp: &dyn HyphenationProvider,
        cookie_banner_dismiss: bool,
    ) -> Result<(LoadedPage, Option<LayoutSource>, Option<Arc<dyn PersistentJs>>), Box<dyn Error>> {
        if matches!(self, PageSource::Empty | PageSource::AboutBlank) {
            return Ok((LoadedPage::empty(), None, None));
        }
        let raw = self.load_bytes(sink.clone(), None)?;
        let (page, layout_source, js_ctx) =
            render_bytes(&raw.bytes, raw.content_type, &raw.base, sink, viewport, &mut std::collections::HashSet::new(), ls_store, ss_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic::DetConfig::default(), false, None, raw.cross_origin_isolated, None, None, lumen_core::ColorSpace::Srgb, raw.cache_control_no_store)?;
        Ok((page, Some(layout_source), js_ctx))
    }
}

/// РЎС‹СЂС‹Рµ Р±Р°Р№С‚С‹ СЃС‚СЂР°РЅРёС†С‹ + РєРѕРЅС‚РµРєСЃС‚, РЅРµРѕР±С…РѕРґРёРјС‹Р№ РґР»СЏ РїРѕСЃР»РµРґСѓСЋС‰РµРіРѕ РїР°СЂСЃРёРЅРіР° Рё
/// СЂР°Р·СЂРµС€РµРЅРёСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… СЃСЃС‹Р»РѕРє. Р’РѕР·РІСЂР°С‰Р°РµС‚СЃСЏ `PageSource::load_bytes`.
pub(crate) struct RawPage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) base: ResourceBase,
    pub(crate) content_type: Option<&'static str>,
    /// True when the server sent `Cross-Origin-Opener-Policy: same-origin` +
    /// `Cross-Origin-Embedder-Policy: require-corp` on this document, enabling
    /// `window.crossOriginIsolated` and unlocking SharedArrayBuffer / high-res timers.
    pub(crate) cross_origin_isolated: bool,
    /// True when the response carried `Cache-Control: no-store`. Disqualifies
    /// the page from a full bfcache freeze (HTML LS В§8.6) вЂ” the shell falls
    /// back to the existing HTML-snapshot bfcache path on navigate-away.
    pub(crate) cache_control_no_store: bool,
}

/// Whether `resp_headers` carry `Cache-Control: no-store`, per RFC 9111 В§5.2.
///
/// Extracted as a free function (rather than inline in `load_bytes`) so it is
/// unit-testable without a network round-trip.
pub(crate) fn cache_control_no_store(resp_headers: &[(String, String)]) -> bool {
    resp_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("cache-control"))
        .is_some_and(|(_, v)| lumen_storage::http_cache::CacheControl::parse(v).no_store)
}

/// Resolve an `AutomationCommand::Navigate` URL string to a `PageSource` (SDC-2/SDC-3).
///
/// Mirrors `PageSource::from_arg`'s http(s)/`about:blank` cases, but also
/// parses a `file://` prefix into a real filesystem path. Automation callers
/// (BiDi/MCP/graphic_tests) pass full `file:///abs/path` URLs, not bare CLI
/// paths вЂ” `from_arg`'s "anything else is a literal path" fallback would
/// otherwise hand `PathBuf` the whole `file://...` string, which doesn't
/// exist on disk (and on Windows, a naive `strip_prefix("file://")` alone
/// leaves a leading slash before the drive letter вЂ” `/D:/foo` вЂ” which also
/// doesn't resolve; this strips that slash only when a drive letter follows,
/// so `file:///home/x` (POSIX, where the slash IS the root) is untouched).
pub(crate) fn page_source_for_automation_url(url: &str) -> PageSource {
    if url.starts_with("http://") || url.starts_with("https://") {
        return PageSource::Url(url.to_owned());
    }
    if url == "about:blank" {
        return PageSource::AboutBlank;
    }
    if url == chrome_preview::URL {
        return PageSource::Static {
            html: chrome_preview::HTML.to_owned(),
            url: chrome_preview::URL.to_owned(),
        };
    }
    // BUG-440: the `file://`-to-path rule itself lives in
    // `resource_base::file_url_to_path`, shared with a `file:` href resolved
    // against a local page, so the two callers cannot disagree about what a
    // `file://` URL names. The bare-path fallback below stays a path: a CLI
    // argument is not a URL, so its `?`, `#` and `%` are literal characters of
    // a filename and must not be cut or decoded.
    if let Some(path) = crate::resource_base::file_url_to_path(url) {
        return PageSource::File(path);
    }
    PageSource::File(PathBuf::from(url))
}

/// Resolve a JS-initiated navigation URL (`window.open`, `location.href=`,
/// `location.assign/replace`) to a `PageSource`, honouring `file://` (BUG-293).
///
/// Only `file://` URLs get special treatment: they resolve to a
/// `PageSource::File` so the local page loads from disk instead of hitting the
/// http-only network path (which rejects them as `unsupported scheme: file`).
/// Every other URL вЂ” http(s), `about:*`, and relative URLs already resolved to
/// absolute by the JS engine вЂ” keeps the existing `PageSource::Url` path
/// untouched.
///
/// Security: a web page (`opener` is an http/https `PageSource::Url`) may not
/// navigate to a local `file://` resource вЂ” that returns `Err(reason)` and the
/// caller surfaces a clear diagnostic instead of loading the file. `fileв†’file`
/// (a local page opening another local page) and non-web openers are allowed.
pub(crate) fn resolve_js_navigation(url: &str, opener: &PageSource) -> Result<PageSource, String> {
    if !url.starts_with("file://") {
        return Ok(PageSource::Url(url.to_owned()));
    }
    let opener_is_web = matches!(
        opener,
        PageSource::Url(u) if u.starts_with("http://") || u.starts_with("https://")
    );
    if opener_is_web {
        return Err(format!(
            "РїРµСЂРµС…РѕРґ web-СЃС‚СЂР°РЅРёС†С‹ РЅР° Р»РѕРєР°Р»СЊРЅС‹Р№ С„Р°Р№Р» Р·Р°Р±Р»РѕРєРёСЂРѕРІР°РЅ РїРѕР»РёС‚РёРєРѕР№ Р±РµР·РѕРїР°СЃРЅРѕСЃС‚Рё: {url}"
        ));
    }
    Ok(page_source_for_automation_url(url))
}
