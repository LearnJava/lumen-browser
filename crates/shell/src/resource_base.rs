//! Where a page's relative URLs resolve from, and the HTTP client its
//! subresources are fetched with.
//!
//! [`ResourceBase`] is the page's own address — a file path or a URL — so a
//! `<link href>`, `<script src>` or `background-image: url(...)` can be turned
//! into a [`ResolvedResource`]. It also builds the `HttpClient` every
//! subresource pass uses, which is why the session-global Service Worker fetch
//! interceptor (PH3-20) lives next to it rather than in the shell's worker
//! code: `http_client_for_subresource` is its only reader.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3d); behaviour and
//! signatures are unchanged.

use crate::*;

/// РћС‚РєСѓРґР° Р·Р°РіСЂСѓР¶РµРЅР° СЃС‚СЂР°РЅРёС†Р° вЂ” РЅСѓР¶РЅРѕ РґР»СЏ СЂР°Р·СЂРµС€РµРЅРёСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅС‹С… URL РІ `<link>`.
#[derive(Clone)]
pub(crate) enum ResourceBase {
    /// РЎС‚СЂР°РЅРёС†Р° Р·Р°РіСЂСѓР¶РµРЅР° РёР· С„Р°Р№Р»Р°. `href` СЂР°Р·СЂРµС€Р°РµС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ РґРёСЂРµРєС‚РѕСЂРёРё С„Р°Р№Р»Р°.
    File(PathBuf),
    /// РЎС‚СЂР°РЅРёС†Р° Р·Р°РіСЂСѓР¶РµРЅР° РїРѕ URL. `href` СЂР°Р·СЂРµС€Р°РµС‚СЃСЏ РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ СЌС‚РѕРіРѕ URL.
    Url(String),
}

impl ResourceBase {
    pub(crate) fn resolve(&self, href: &str) -> ResolvedResource {
        if href.starts_with("http://") || href.starts_with("https://") {
            return ResolvedResource::Url(href.to_owned());
        }
        match self {
            ResourceBase::File(base_path) => {
                let dir = base_path.parent().unwrap_or(std::path::Path::new("."));
                ResolvedResource::File(dir.join(href))
            }
            ResourceBase::Url(base_url) => {
                // Resolve С‡РµСЂРµР· СЃС‚СЂСѓРєС‚СѓСЂРёСЂРѕРІР°РЅРЅС‹Р№ Url РёР· lumen-core; РїСЂРё СЃР±РѕРµ
                // base (РЅРµ РґРѕР»Р¶РЅРѕ СЃР»СѓС‡Р°С‚СЊСЃСЏ вЂ” base СЃР°РјРё Рё РїРѕР»РѕР¶РёР»Рё РІ Р·Р°РіСЂСѓР·РєРµ
                // СЃС‚СЂР°РЅРёС†С‹) РѕС‚РєР°С‚С‹РІР°РµРјСЃСЏ РЅР° raw href, С‡С‚РѕР±С‹ РёР·-Р·Р° РѕРґРЅРѕРіРѕ
                // Р±РёС‚РѕРіРѕ <link> РЅРµ РІР°Р»РёС‚СЊ РІРµСЃСЊ СЂРµРЅРґРµСЂ.
                let resolved = lumen_core::url::Url::parse(base_url)
                    .and_then(|u| u.resolve(href))
                    .map(|u| u.as_str().to_owned())
                    .unwrap_or_else(|_| href.to_owned());
                ResolvedResource::Url(resolved)
            }
        }
    }

    /// Р РµР·РѕР»РІРёС‚СЊ `href` РѕС‚РЅРѕСЃРёС‚РµР»СЊРЅРѕ base Рё РІРµСЂРЅСѓС‚СЊ СЃС‚СЂРѕРєРѕРІРѕРµ РїСЂРµРґСЃС‚Р°РІР»РµРЅРёРµ.
    /// Р”Р»СЏ `File` base вЂ” Р°Р±СЃРѕР»СЋС‚РЅС‹Р№ РїСѓС‚СЊ; РґР»СЏ `Url` base вЂ” Р°Р±СЃРѕР»СЋС‚РЅС‹Р№ URL.
    /// РСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ РІ preload-dispatcher, РіРґРµ РЅСѓР¶РЅР° СЃС‚СЂРѕРєР° (РЅРµ `ResolvedResource`).
    pub(crate) fn resolve_str(&self, href: &str) -> String {
        match self.resolve(href) {
            ResolvedResource::File(p) => p.to_string_lossy().into_owned(),
            ResolvedResource::Url(u) => u,
        }
    }

    /// РР·РІР»РµС‡СЊ Origin СЃС‚СЂР°РЅРёС†С‹, РµСЃР»Рё base вЂ” URL (РЅРµ С„Р°Р№Р»).
    pub(crate) fn origin(&self) -> Option<lumen_network::Origin> {
        if let ResourceBase::Url(base_url) = self
            && let Ok(url) = lumen_core::url::Url::parse(base_url)
        {
            return lumen_network::Origin::from_url(&url).ok();
        }
        None
    }

    /// РџРѕСЃС‚СЂРѕРёС‚СЊ `HttpClient` РґР»СЏ Р·Р°РіСЂСѓР·РєРё РїРѕРґСЂРµСЃСѓСЂСЃРѕРІ. Р•СЃР»Рё СЃС‚СЂР°РЅРёС†Р° Р·Р°РіСЂСѓР¶РµРЅР°
    /// РїРѕ HTTPS, РїРѕРґРєР»СЋС‡Р°РµС‚ mixed-content enforcement (SpecDefault РїРѕ W3C Mixed
    /// Content spec). Caller РІС‹Р±РёСЂР°РµС‚ `RequestDestination` Рё РІС‹Р·С‹РІР°РµС‚
    /// `fetch_subresource`, Р° РЅРµ `fetch`.
    pub(crate) fn http_client_for_subresource(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    ) -> lumen_network::HttpClient {
        use lumen_network::{
            BrotliContentDecoder, DeflateContentDecoder, GzipContentDecoder, HttpClient,
            MixedContentMode,
        };
        let mut builder = HttpClient::new()
            .with_sink(sink)
            .with_content_decoder(Arc::new(BrotliContentDecoder::new()))
            .with_content_decoder(Arc::new(GzipContentDecoder::new()))
            .with_content_decoder(Arc::new(DeflateContentDecoder::new()));
        if let Some(jar) = cookie_jar {
            builder = builder.with_cookie_jar(
                Arc::new(lumen_storage::CookieJarProvider::new(jar)),
                None,
            );
        }
        let mut client = crate::config::global().apply_http(builder);
        // PH3-20: attach the session Service Worker fetch interceptor so that
        // subresource + page `fetch()`/XHR requests are served cache-first by an
        // active SW execution thread before hitting the network. Set once in
        // `run_window_mode`; absent in headless/PDF/dump modes (loop stays open).
        if let Some(interceptor) = sw_fetch_interceptor() {
            client = client.with_interceptor(interceptor);
        }
        if let Some(origin) = self.origin()
            && origin.is_potentially_trustworthy()
        {
            return client.with_mixed_content_policy(origin, MixedContentMode::SpecDefault);
        }
        client
    }
}

/// Session-global Service Worker fetch interceptor (PH3-20).
///
/// Set once in `run_window_mode` after the `Lumen` state (which owns the shared
/// `sw_worker_store` + `cache_store`) is built. `http_client_for_subresource`
/// reads it to attach SW interception to every subresource/`fetch()` client.
/// `None` in headless modes вЂ” the interception loop simply stays open there.
pub(crate) static SW_FETCH_INTERCEPTOR: std::sync::OnceLock<Arc<dyn lumen_core::ext::FetchInterceptor>> =
    std::sync::OnceLock::new();

/// Read the session-global SW fetch interceptor, if one was installed.
fn sw_fetch_interceptor() -> Option<Arc<dyn lumen_core::ext::FetchInterceptor>> {
    SW_FETCH_INTERCEPTOR.get().cloned()
}

pub(crate) enum ResolvedResource {
    File(PathBuf),
    Url(String),
}
