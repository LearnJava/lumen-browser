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

/// Откуда загружена страница — нужно для разрешения относительных URL в `<link>`.
#[derive(Clone)]
pub(crate) enum ResourceBase {
    /// Страница загружена из файла. `href` разрешается относительно директории файла.
    File(PathBuf),
    /// Страница загружена по URL. `href` разрешается относительно этого URL.
    Url(String),
}

impl ResourceBase {
    pub(crate) fn resolve(&self, href: &str) -> ResolvedResource {
        if href.starts_with("http://") || href.starts_with("https://") {
            return ResolvedResource::Url(href.to_owned());
        }
        match self {
            ResourceBase::File(base_path) => {
                // BUG-440: `href` may carry its own scheme (`about:`, `data:`,
                // `mailto:`, `file:`, ...) — that addresses a resource outside
                // this file's directory, not a path relative to it, so it must
                // not be joined onto `dir` below.
                if let Some(scheme) = href_scheme(href) {
                    // `file:` is the one foreign scheme that still names a
                    // local file, so it resolves to a path instead: handing the
                    // whole `file://…` string on as a URL only moves the defect
                    // one caller along, where `PathBuf` receives it verbatim.
                    if scheme.eq_ignore_ascii_case("file")
                        && let Some(path) = file_url_to_path(href)
                    {
                        return ResolvedResource::File(path);
                    }
                    return ResolvedResource::Url(href.to_owned());
                }
                let dir = base_path.parent().unwrap_or(std::path::Path::new("."));
                // BUG-440: a GET form submission appends `?query` to the
                // action, and a bare `#fragment` reference is legal too — a
                // filesystem path never contains either, so both are cut
                // before the join, and the percent-escapes the rest is spelled
                // with are decoded (a URL reference names `my file.html` as
                // `my%20file.html`). An empty remainder (`"?q=1"`, `"#sec"`)
                // means "this same file" (RFC 3986 §5.3's empty-reference
                // case), which `ResourceBase::File` cannot express any other
                // way since it carries no query/fragment of its own.
                let path_part = url_path_component(href);
                if path_part.is_empty() {
                    ResolvedResource::File(base_path.clone())
                } else {
                    ResolvedResource::File(dir.join(path_part))
                }
            }
            ResourceBase::Url(base_url) => {
                // Resolve через структурированный Url из lumen-core; при сбое
                // base (не должно случаться — base сами и положили в загрузке
                // страницы) откатываемся на raw href, чтобы из-за одного
                // битого <link> не валить весь рендер.
                let resolved = lumen_core::url::Url::parse(base_url)
                    .and_then(|u| u.resolve(href))
                    .map(|u| u.as_str().to_owned())
                    .unwrap_or_else(|_| href.to_owned());
                ResolvedResource::Url(resolved)
            }
        }
    }

    /// Резолвить `href` относительно base и вернуть строковое представление.
    /// Для `File` base — абсолютный путь; для `Url` base — абсолютный URL.
    /// Используется в preload-dispatcher, где нужна строка (не `ResolvedResource`).
    pub(crate) fn resolve_str(&self, href: &str) -> String {
        match self.resolve(href) {
            ResolvedResource::File(p) => p.to_string_lossy().into_owned(),
            ResolvedResource::Url(u) => u,
        }
    }

    /// Извлечь Origin страницы, если base — URL (не файл).
    pub(crate) fn origin(&self) -> Option<lumen_network::Origin> {
        if let ResourceBase::Url(base_url) = self
            && let Ok(url) = lumen_core::url::Url::parse(base_url)
        {
            return lumen_network::Origin::from_url(&url).ok();
        }
        None
    }

    /// Построить `HttpClient` для загрузки подресурсов. Если страница загружена
    /// по HTTPS, подключает mixed-content enforcement (SpecDefault по W3C Mixed
    /// Content spec). Caller выбирает `RequestDestination` и вызывает
    /// `fetch_subresource`, а не `fetch`.
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

/// The URI scheme `s` begins with (RFC 3986 §3.1: `ALPHA *( ALPHA / DIGIT /
/// "+" / "-" / "." ) ":"`, the `:` not included) — `about`, `data`, `mailto`,
/// `file` — or `None` for a scheme-less reference. Mirrors `lumen_core::url`'s
/// private `has_scheme` (not exposed outside that crate) rather than depending
/// on it, to keep this a self-contained shell-side fix.
///
/// One deliberate narrowing against that function: a **single-letter** prefix
/// is not a scheme here, because on Windows it is a drive letter — reading
/// `D:/docs/x.html` as a `d:` URL would send a perfectly good local href to
/// the network layer. No scheme this engine can act on is one character long,
/// so the narrowing costs nothing.
fn href_scheme(s: &str) -> Option<&str> {
    let end = s.find(':')?;
    let scheme = &s[..end];
    if scheme.len() < 2 {
        return None;
    }
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some(scheme)
}

/// A `file://` URL as a filesystem path, or `None` when `url` is not one.
///
/// Shared by [`ResourceBase::resolve`] (a `file:` href on a local page) and
/// `page_source_for_automation_url` (a BiDi/MCP navigation), so the two cannot
/// disagree about what a `file://` URL names. On Windows a bare
/// `strip_prefix("file://")` leaves a slash in front of the drive letter
/// (`/D:/foo`), which does not resolve; that slash is dropped only when a
/// drive letter follows, so `file:///home/x` — where the slash IS the root —
/// is untouched. Query and fragment are cut and percent-escapes decoded for
/// the same reason as in `resolve`: this is a URL, not a path.
pub(crate) fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest
        .strip_prefix('/')
        .filter(|p| p.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(rest);
    Some(PathBuf::from(url_path_component(rest)))
}

/// The path component of a URL reference, percent-decoded: everything before
/// the first `?` or `#`, with `%XX` turned back into the byte it stands for.
///
/// A `%` that is not followed by two hex digits is kept verbatim rather than
/// dropped — a literal `%` is a legal character in a filename on every
/// filesystem this runs on, and silently eating it would send the caller
/// looking for a different file than the one that exists.
fn url_path_component(href: &str) -> String {
    let cut = href.find(['?', '#']).unwrap_or(href.len());
    let path = &href[..cut];
    if !path.contains('%') {
        return path.to_owned();
    }
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    // A decode that does not spell UTF-8 is not a path we can name; keeping the
    // encoded form at least leaves a readable diagnostic for the caller.
    String::from_utf8(out).unwrap_or_else(|_| path.to_owned())
}

/// Session-global Service Worker fetch interceptor (PH3-20).
///
/// Set once in `run_window_mode` after the `Lumen` state (which owns the shared
/// `sw_worker_store` + `cache_store`) is built. `http_client_for_subresource`
/// reads it to attach SW interception to every subresource/`fetch()` client.
/// `None` in headless modes — the interception loop simply stays open there.
pub(crate) static SW_FETCH_INTERCEPTOR: std::sync::OnceLock<Arc<dyn lumen_core::ext::FetchInterceptor>> =
    std::sync::OnceLock::new();

/// Read the session-global SW fetch interceptor, if one was installed.
fn sw_fetch_interceptor() -> Option<Arc<dyn lumen_core::ext::FetchInterceptor>> {
    SW_FETCH_INTERCEPTOR.get().cloned()
}

/// What a `href` resolved to: a path to read off disk, or a URL to fetch.
///
/// `Debug` so a test that got the wrong variant can name what it got — the
/// BUG-440 cases differ only in which variant they land on, and an assertion
/// that can only say "expected File" is one bisect step short of useful.
#[derive(Debug)]
pub(crate) enum ResolvedResource {
    File(PathBuf),
    Url(String),
}
