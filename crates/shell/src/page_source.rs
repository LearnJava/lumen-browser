//! Where a document's bytes come from: the [`PageSource`] enum, its loading
//! methods, the [`RawPage`] they return, and the two helpers that turn an
//! automation- or JS-supplied URL string into a `PageSource`.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3b); behaviour and
//! signatures are unchanged.

use crate::*;

/// Источник страницы. Запоминается в `Lumen`, чтобы reload (F5/Ctrl+R) мог
/// заново выполнить fetch/parse/layout/paint без аргументов командной строки.
#[derive(Debug, Clone)]
pub(crate) enum PageSource {
    /// Без аргументов — рисуем пустое окно. Reload no-op (грузить нечего).
    Empty,
    File(PathBuf),
    /// Сетевая страница. `body` — тело навигации (E2E-1): `None` у обычного
    /// перехода по ссылке/адресной строке, `Some` ровно у одной навигации —
    /// той, которую сейчас порождает отправка формы методом POST
    /// (`form_submit.rs`).
    ///
    /// Тело живёт **в источнике**, а не отдельным полем `Lumen`, потому что
    /// поток загрузки получает именно клон `PageSource`: разъехаться адресу и
    /// телу так просто негде. Обратная сторона — тело обязано быть
    /// одноразовым: `reload()` стирает его сразу после того, как передал
    /// источник загрузчику ([`PageSource::forget_nav_body`]), поэтому ни F5,
    /// ни back/forward, ни восстановление сессии не повторяют POST.
    Url { url: String, body: Option<Box<lumen_network::NavigationBody>> },
    /// `about:blank` — пустой документ без сетевого запроса (HTML spec §7.5).
    /// `url_str()` возвращает "about:blank" для адресной строки и истории.
    AboutBlank,
    /// Страница восстанавливается из bfcache: HTML уже есть в памяти,
    /// сетевой запрос не нужен. `base_url` — оригинальный URL страницы
    /// (для разрешения относительных ссылок внутри HTML).
    Snapshot { html: String, base_url: String },
    /// Внутренняя статическая страница (`about:newtab`): HTML генерируется
    /// в памяти, сетевой запрос не нужен. `url` — канонический about-URL,
    /// показывается в адресной строке и истории.
    Static { html: String, url: String },
}

/// If `url` is a `javascript:` URL (HTML LS §7.4.5), return the source code
/// after the scheme — the scheme match is ASCII case-insensitive (RFC 3986
/// §3.1). GAP-NAVCTX срез 1 (BUG-884): no percent-decoding of the code —
/// unlike a `data:` URL body, a `javascript:` URL's source rarely carries
/// `%XX` escapes in practice, and skipping it keeps this a plain substring
/// operation; revisit if a WPT id needs it.
pub(crate) fn javascript_url_code(url: &str) -> Option<&str> {
    const PREFIX: &str = "javascript:";
    let head = url.get(..PREFIX.len())?;
    head.eq_ignore_ascii_case(PREFIX).then(|| &url[PREFIX.len()..])
}

impl PageSource {
    /// Обычная GET-навигация на `url` — источник без тела запроса.
    ///
    /// Единственный способ собрать `PageSource::Url` вне отправки формы:
    /// оставляет `body` невыраженным в каждом из десятка call-site-ов, где
    /// тела заведомо нет (адресная строка, история, вкладки, автоматизация).
    pub(crate) fn url(url: impl Into<String>) -> Self {
        PageSource::Url { url: url.into(), body: None }
    }

    /// Тело навигации, если этот источник — отправка формы методом POST.
    pub(crate) fn nav_body(&self) -> Option<&lumen_network::NavigationBody> {
        match self {
            PageSource::Url { body, .. } => body.as_deref(),
            _ => None,
        }
    }

    /// Забыть тело навигации, оставив адрес.
    ///
    /// Вызывается ровно один раз — из `reload()`, сразу после того, как
    /// источник ушёл загрузчику. С этого момента запись истории, снимок
    /// сессии и любая последующая перезагрузка того же адреса — обычный GET:
    /// повторная отправка формы не происходит нигде и никогда (HTML LS не
    /// обязывает браузер её предлагать, а тихо ре-постить — худшее из
    /// поведений: платёж или регистрация ушли бы дважды).
    pub(crate) fn forget_nav_body(&mut self) {
        if let PageSource::Url { body, .. } = self {
            *body = None;
        }
    }

    pub(crate) fn from_arg(arg: Option<&str>) -> Self {
        match arg {
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                PageSource::url(s)
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
            PageSource::Empty => "(пустая вкладка)".to_owned(),
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url { url, .. } => url.clone(),
            PageSource::AboutBlank => "about:blank".to_owned(),
            PageSource::Snapshot { base_url, .. } => format!("[bfcache] {base_url}"),
            PageSource::Static { url, .. } => url.clone(),
        }
    }

    /// Origin string (scheme+host+port) for localStorage partitioning.
    /// Returns `None` for file: and empty sources (no cross-origin storage needed).
    pub(crate) fn origin_str(&self) -> Option<String> {
        let url_s = match self {
            PageSource::Url { url, .. } => url.as_str(),
            PageSource::Snapshot { base_url, .. } => base_url.as_str(),
                _ => return None,
        };
        lumen_core::url::Url::parse(url_s).ok().map(|u| {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host(), port)
        })
    }

    /// URL-строка страницы для bfcache-ключа. `None` если нет URL (пустая вкладка, файл).
    pub(crate) fn url_str(&self) -> Option<&str> {
        match self {
            PageSource::Url { url, .. } => Some(url.as_str()),
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
            PageSource::Url { url, .. } => Some(ResourceBase::Url(url.clone())),
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

    /// Прочитать байты страницы с диска или из сети, плюс вернуть базу для
    /// относительных URL и подсказку о content-type. Используется и обычным
    /// `load`, и dump-режимами.
    pub(crate) fn load_bytes(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    ) -> Result<RawPage, Box<dyn Error>> {
        match self {
            PageSource::Empty => Err("источник пуст — нечего загружать".into()),
            PageSource::AboutBlank => Ok(RawPage {
                bytes: b"<!DOCTYPE html><html><head></head><body></body></html>".to_vec(),
                base: ResourceBase::Url("about:blank".to_owned()),
                content_type: Some("text/html".to_owned()),
                cross_origin_isolated: false,
                cache_control_no_store: false,
                csp_header: None,
                status: 0,
                redirected: false,
            }),
            PageSource::File(path) => {
                let bytes = std::fs::read(path)?;
                Ok(RawPage {
                    bytes,
                    base: ResourceBase::File(path.clone()),
                    content_type: None,
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                    csp_header: None,
                    status: 0,
                    redirected: false,
                })
            }
            PageSource::Url { url, body } => {
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
                let lumen_network::PageResponse { body: bytes, headers: resp_headers, final_url, status } =
                    client.fetch_page(&lumen_url, body.as_deref())?;
                // BUG-640: redirect signal — the only one obtainable without
                // a `lumen-network` change (`fetch_with_redirect`'s hop
                // countdown is never surfaced as a count).
                let redirected = final_url != lumen_url;
                fetch_span.set_bytes(bytes.len());
                eprintln!("Получено {} байт", bytes.len());
                let coop = resp_headers.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-opener-policy"))
                    .map(|(_, v)| v.as_str());
                let coep = resp_headers.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-embedder-policy"))
                    .map(|(_, v)| v.as_str());
                let cross_origin_isolated = lumen_network::CrossOriginIsolationState::from_headers(coop, coep).is_cross_origin_isolated();
                Ok(RawPage {
                    bytes,
                    // BUG-757: база документа — адрес, с которого пришёл
                    // финальный ответ, а не аргумент навигации. После
                    // серверного редиректа они разные, и от базы зависят
                    // `location.*`/`document.baseURI`, разрешение
                    // относительных подресурсов и origin хранилищ.
                    base: ResourceBase::Url(final_url.to_string()),
                    content_type: response_content_type(&resp_headers),
                    cross_origin_isolated,
                    cache_control_no_store: cache_control_no_store(&resp_headers),
                    csp_header: content_security_policy_header(&resp_headers),
                    status,
                    redirected,
                })
            }
            PageSource::Snapshot { html, base_url } => {
                // bfcache restoration: HTML already in memory, no network request.
                Ok(RawPage {
                    bytes: html.as_bytes().to_vec(),
                    base: ResourceBase::Url(base_url.clone()),
                    content_type: Some("text/html".to_owned()),
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                    csp_header: None,
                    status: 0,
                    redirected: false,
                })
            }
            PageSource::Static { html, url } => {
                // Internal about: page: HTML generated in memory, no network request.
                Ok(RawPage {
                    bytes: html.as_bytes().to_vec(),
                    base: ResourceBase::Url(url.clone()),
                    content_type: Some("text/html".to_owned()),
                    cross_origin_isolated: false,
                    cache_control_no_store: false,
                    csp_header: None,
                    status: 0,
                    redirected: false,
                })
            }
        }
    }

    /// Как `load_bytes`, но для сетевых (URL) источников тело финального
    /// 2xx-ответа стримится: каждая декодированная порция передаётся в
    /// `on_chunk` ещё до полного скачивания (PH1-2a). Для несетевых источников
    /// (File/Snapshot/Static) делегирует в `load_bytes` без вызовов `on_chunk`
    /// — caller сам нарежет уже-загруженное тело. Возвращаемый `RawPage.bytes`
    /// — полное декодированное тело (как у `load_bytes`).
    ///
    /// Второй аргумент `on_chunk` — URL, с которого течёт тело: после
    /// редиректа он отличается от запрошенного, и относительные ссылки в
    /// потоке (preload-хинты) обязаны резолвиться именно от него (BUG-757).
    pub(crate) fn load_bytes_streaming(
        &self,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
        on_chunk: &mut dyn FnMut(&[u8], &lumen_core::url::Url),
    ) -> Result<RawPage, Box<dyn Error>> {
        let PageSource::Url { url, body } = self else {
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
        let lumen_network::PageResponse { body: bytes, headers: resp_headers, final_url, status } =
            client.fetch_page_streaming(&lumen_url, on_chunk, body.as_deref())?;
        // BUG-640: see `load_bytes` for why this can't be an exact hop count.
        let redirected = final_url != lumen_url;
        eprintln!("Получено {} байт (streaming)", bytes.len());
        let coop = resp_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-opener-policy"))
            .map(|(_, v)| v.as_str());
        let coep = resp_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("cross-origin-embedder-policy"))
            .map(|(_, v)| v.as_str());
        let cross_origin_isolated = lumen_network::CrossOriginIsolationState::from_headers(coop, coep)
            .is_cross_origin_isolated();
        Ok(RawPage {
            // BUG-757: см. `load_bytes` — база из финального URL ответа. Тот же
            // адрес приходит и в `on_chunk` (вторым аргументом), поэтому
            // preload-хинты из потока резолвятся от той же базы, что документ.
            base: ResourceBase::Url(final_url.to_string()),
            bytes,
            content_type: response_content_type(&resp_headers),
            cross_origin_isolated,
            cache_control_no_store: cache_control_no_store(&resp_headers),
            csp_header: content_security_policy_header(&resp_headers),
            status,
            redirected,
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
            render_bytes(&raw.bytes, raw.content_type.as_deref(), &raw.base, sink, viewport, &mut std::collections::HashSet::new(), ls_store, ss_store, idb_backend, sw_backend, hp, cookie_banner_dismiss, deterministic::DetConfig::default(), false, None, raw.cross_origin_isolated, None, None, lumen_core::ColorSpace::Srgb, raw.cache_control_no_store, raw.status, raw.redirected, raw.csp_header.as_deref())?;
        Ok((page, Some(layout_source), js_ctx))
    }
}

/// Сырые байты страницы + контекст, необходимый для последующего парсинга и
/// разрешения относительных ссылок. Возвращается `PageSource::load_bytes`.
pub(crate) struct RawPage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) base: ResourceBase,
    pub(crate) content_type: Option<String>,
    /// True when the server sent `Cross-Origin-Opener-Policy: same-origin` +
    /// `Cross-Origin-Embedder-Policy: require-corp` on this document, enabling
    /// `window.crossOriginIsolated` and unlocking SharedArrayBuffer / high-res timers.
    pub(crate) cross_origin_isolated: bool,
    /// True when the response carried `Cache-Control: no-store`. Disqualifies
    /// the page from a full bfcache freeze (HTML LS §8.6) — the shell falls
    /// back to the existing HTML-snapshot bfcache path on navigate-away.
    pub(crate) cache_control_no_store: bool,
    /// Raw `Content-Security-Policy` header of the response, if any
    /// (GAP-CSPENF срез 5). Stamped onto the parsed [`Document`] so that every
    /// enforcement point — which only ever gets a `&Document` — sees the
    /// header alongside the document's `<meta>` policies. `None` for every
    /// non-network source (file / snapshot / `about:` page).
    pub(crate) csp_header: Option<String>,
    /// HTTP status of the response, or `0` for a non-network source
    /// (`File`/`Snapshot`/`Static`/`AboutBlank`) or a fresh HTTP-cache hit.
    /// Threaded from `lumen_network::PageResponse::status` for
    /// `PerformanceNavigationTiming.responseStatus` (BUG-640).
    pub(crate) status: u16,
    /// Whether the final URL differs from the originally-requested one — see
    /// `nav_timing`'s doc comment for why this can't be an exact hop count.
    pub(crate) redirected: bool,
}

/// Whether `resp_headers` carry `Cache-Control: no-store`, per RFC 9111 §5.2.
///
/// Extracted as a free function (rather than inline in `load_bytes`) so it is
/// unit-testable without a network round-trip.
pub(crate) fn cache_control_no_store(resp_headers: &[(String, String)]) -> bool {
    resp_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("cache-control"))
        .is_some_and(|(_, v)| lumen_storage::http_cache::CacheControl::parse(v).no_store)
}

/// The response's own `Content-Type` header (GAP-XMLDOC срез 32, BUG-786/
/// BUG-685): both network `RawPage` constructors used to hardcode
/// `Some("text/html")` regardless of what the server actually sent, which made
/// `is_xml_flavoured_document`'s Content-Type branch dead code for every
/// network load — an `.xhtml`/`.svg`-flavoured response was only ever
/// recognised by its URL's extension, never by this header, even though the
/// header was already sitting in `resp_headers` for COOP/COEP to read two
/// lines below each call site. `None` (missing header) falls through to the
/// same URL-extension heuristic `is_xml_flavoured_document` already has.
pub(crate) fn response_content_type(resp_headers: &[(String, String)]) -> Option<String> {
    resp_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
}

/// The response's `Content-Security-Policy` header text (GAP-CSPENF срез 5),
/// or `None` when the response carried none.
///
/// `Content-Security-Policy-Report-Only` is deliberately **not** matched: this
/// slice enforces, and a report-only policy must never block anything — the
/// exact-name comparison keeps it out (a `starts_with` would swallow it).
///
/// A response may repeat the header, and each occurrence is an independent
/// policy per CSP3 §3.4. They are joined with `"; "` here, the same
/// simplification `csp_enforce::document_csp_policy` already applies to
/// multiple `<meta>` policies — for a single policy (the overwhelming majority)
/// the result is identical, for several policies with interacting relaxations
/// it can be more permissive than the spec.
///
/// Extracted as a free function so it is unit-testable without a network
/// round-trip, like its two neighbours above.
pub(crate) fn content_security_policy_header(
    resp_headers: &[(String, String)],
) -> Option<String> {
    let parts: Vec<&str> = resp_headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("; "))
}

/// Resolve an `AutomationCommand::Navigate` URL string to a `PageSource` (SDC-2/SDC-3).
///
/// Mirrors `PageSource::from_arg`'s http(s)/`about:blank` cases, but also
/// parses a `file://` prefix into a real filesystem path. Automation callers
/// (BiDi/MCP/graphic_tests) pass full `file:///abs/path` URLs, not bare CLI
/// paths — `from_arg`'s "anything else is a literal path" fallback would
/// otherwise hand `PathBuf` the whole `file://...` string, which doesn't
/// exist on disk (and on Windows, a naive `strip_prefix("file://")` alone
/// leaves a leading slash before the drive letter — `/D:/foo` — which also
/// doesn't resolve; this strips that slash only when a drive letter follows,
/// so `file:///home/x` (POSIX, where the slash IS the root) is untouched).
pub(crate) fn page_source_for_automation_url(url: &str) -> PageSource {
    if url.starts_with("http://") || url.starts_with("https://") {
        return PageSource::url(url);
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
/// Every other URL — http(s), `about:*`, and relative URLs already resolved to
/// absolute by the JS engine — keeps the existing `PageSource::Url` path
/// untouched.
///
/// Security: a web page (`opener` is an http/https `PageSource::Url`) may not
/// navigate to a local `file://` resource — that returns `Err(reason)` and the
/// caller surfaces a clear diagnostic instead of loading the file. `file→file`
/// (a local page opening another local page) and non-web openers are allowed.
pub(crate) fn resolve_js_navigation(url: &str, opener: &PageSource) -> Result<PageSource, String> {
    if !url.starts_with("file://") {
        return Ok(PageSource::url(url));
    }
    let opener_is_web = matches!(
        opener,
        PageSource::Url { url: u, .. } if u.starts_with("http://") || u.starts_with("https://")
    );
    if opener_is_web {
        return Err(format!(
            "переход web-страницы на локальный файл заблокирован политикой безопасности: {url}"
        ));
    }
    Ok(page_source_for_automation_url(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_url_code_extracts_source() {
        assert_eq!(javascript_url_code("javascript:alert(1)"), Some("alert(1)"));
        assert_eq!(javascript_url_code("JavaScript:void(0)"), Some("void(0)"));
        assert_eq!(javascript_url_code("javascript:"), Some(""));
    }

    #[test]
    fn javascript_url_code_rejects_other_schemes() {
        assert_eq!(javascript_url_code("https://example.com"), None);
        assert_eq!(javascript_url_code("data:text/html,x"), None);
        assert_eq!(javascript_url_code("java"), None);
        assert_eq!(javascript_url_code(""), None);
    }

    // ---- GAP-XMLDOC срез 32 (BUG-786/BUG-685): response_content_type ----

    #[test]
    fn response_content_type_reads_the_real_header() {
        let headers = vec![
            ("Server".to_owned(), "nginx".to_owned()),
            ("Content-Type".to_owned(), "image/svg+xml".to_owned()),
        ];
        assert_eq!(response_content_type(&headers).as_deref(), Some("image/svg+xml"));
    }

    #[test]
    fn response_content_type_is_case_insensitive_on_the_header_name() {
        let headers = vec![("content-type".to_owned(), "application/xhtml+xml".to_owned())];
        assert_eq!(response_content_type(&headers).as_deref(), Some("application/xhtml+xml"));
    }

    #[test]
    fn response_content_type_none_when_header_absent() {
        let headers = vec![("Server".to_owned(), "nginx".to_owned())];
        assert_eq!(response_content_type(&headers), None);
    }

    /// Corpus-measured regression: `svg/struct/reftests/support/
    /// html-resource-with-symbol-and-content-type-svg.html` is served with
    /// `Content-Type: image/svg+xml` despite its `.html` extension — before
    /// this срез, both network `RawPage` constructors hardcoded
    /// `content_type: Some("text/html")`, so `is_xml_flavoured_document`
    /// never saw this header and fell through to the (non-matching)
    /// extension check.
    #[test]
    fn xml_mime_wins_over_a_non_matching_extension_like_the_corpus_fixture() {
        let base = ResourceBase::Url(
            "http://example.test/svg/struct/reftests/support/html-resource-with-symbol-and-content-type-svg.html".to_owned(),
        );
        let headers = vec![("Content-Type".to_owned(), "image/svg+xml".to_owned())];
        assert!(crate::page_pipeline::is_xml_flavoured_document(
            response_content_type(&headers).as_deref(),
            &base
        ));
    }

    // ---- GAP-CSPENF срез 5: content_security_policy_header ----

    #[test]
    fn csp_header_is_read_from_the_response() {
        let headers = vec![
            ("Server".to_owned(), "nginx".to_owned()),
            ("Content-Security-Policy".to_owned(), "script-src 'none'".to_owned()),
        ];
        assert_eq!(
            content_security_policy_header(&headers).as_deref(),
            Some("script-src 'none'")
        );
    }

    #[test]
    fn csp_header_name_match_is_case_insensitive() {
        let headers = vec![("content-security-POLICY".to_owned(), "img-src 'self'".to_owned())];
        assert_eq!(content_security_policy_header(&headers).as_deref(), Some("img-src 'self'"));
    }

    #[test]
    fn csp_header_none_when_absent() {
        let headers = vec![("Server".to_owned(), "nginx".to_owned())];
        assert_eq!(content_security_policy_header(&headers), None);
    }

    /// Several `Content-Security-Policy` headers are independent policies
    /// (CSP3 §3.4); this срез merges them the same way it merges several
    /// `<meta>` policies.
    #[test]
    fn csp_header_repeated_is_merged() {
        let headers = vec![
            ("Content-Security-Policy".to_owned(), "script-src 'none'".to_owned()),
            ("Content-Security-Policy".to_owned(), "img-src 'self'".to_owned()),
        ];
        assert_eq!(
            content_security_policy_header(&headers).as_deref(),
            Some("script-src 'none'; img-src 'self'")
        );
    }

    /// Report-only must never block: it is a different header name and this
    /// срез enforces, so it is not picked up here.
    #[test]
    fn csp_report_only_header_is_not_enforced() {
        let headers = vec![(
            "Content-Security-Policy-Report-Only".to_owned(),
            "script-src 'none'".to_owned(),
        )];
        assert_eq!(content_security_policy_header(&headers), None);
    }

    #[test]
    fn csp_header_empty_value_is_ignored() {
        let headers = vec![("Content-Security-Policy".to_owned(), "   ".to_owned())];
        assert_eq!(content_security_policy_header(&headers), None);
    }
}
