//! Authority Information Access: fetching a missing intermediate certificate
//! (COMPAT-TRUST-AIA, RFC 5280 §4.2.2.1, `docs/research/site-compat/report.md`
//! §7.6 п.2).
//!
//! A server that sends only its leaf certificate leaves `webpki` without a path
//! to a root: `UnknownIssuer`, which the user sees as the "attackers may be
//! intercepting your passwords" screen. Chrome, Firefox and Windows all close
//! the gap the same way — the leaf names its issuer's certificate in the AIA
//! extension (`caIssuers`, an `http://` URL) and the client downloads it.
//! [`super::verifier::LumenVerifier`] calls [`Aia::issuers_of`] only after
//! `webpki` already answered `UnknownIssuer`, then re-verifies with the
//! certificates found.
//!
//! What this grants and what it does not:
//! - **No new trust.** A downloaded certificate is an untrusted *intermediate*:
//!   it must still chain to a trust anchor and be signed correctly, exactly like
//!   one the server sent. The response is also filtered to certificates whose
//!   subject is the issuer being sought, so garbage costs nothing.
//! - **Plain `http://` only**, no TLS (a certificate fetch over TLS would recurse
//!   into this verifier), at most [`MAX_REDIRECTS`] redirects, also `http://`.
//!   No cookies, no credentials, no referrer: the request carries nothing but
//!   `Host`.
//! - **Bounded**: [`MAX_BODY`] bytes, [`TOTAL_TIMEOUT`] for the whole fetch.
//!   Results are cached by URL for the process lifetime; a failure is cached for
//!   [`FAILURE_TTL`] so a page with fifty subresources on one broken host waits
//!   once, not fifty times.
//! - **Direct connections only.** Behind a proxy or Tor the fetch would bypass
//!   it, so the shell turns the whole mechanism off ([`set_enabled`]).
//!
//! Where candidates come from, in order: the OS `CA` store
//! ([`super::os_roots::os_intermediates`], no network), then the AIA URLs of the
//! certificate whose issuer is missing. The response is a DER certificate, a PEM
//! bundle or a PKCS#7 `certs-only` message (`.p7c`, what many CAs publish);
//! BER indefinite lengths in PKCS#7 are not understood and yield nothing.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rustls::pki_types::CertificateDer;

use super::cert_fields::{self, Der};
use super::os_roots;

/// Largest response body accepted.
pub const MAX_BODY: usize = 1 << 20;
/// Wall-clock budget of one URL fetch (connect + request + response).
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a failed URL is not retried.
pub const FAILURE_TTL: Duration = Duration::from_secs(120);
/// `Location` hops followed (all `http://`).
const MAX_REDIRECTS: usize = 3;
/// `caIssuers` URLs tried per certificate.
const MAX_URLS: usize = 3;
/// Missing-issuer lookups per handshake: leaf → intermediate → cross-signed root path.
pub const MAX_LOOKUPS: usize = 4;
/// URLs remembered; the cache is dropped wholesale when full.
const MAX_CACHE_ENTRIES: usize = 64;
/// Header block allowed before the body.
const MAX_HEADERS: usize = 16 * 1024;

const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_INTEGER: u8 = 0x02;
const TAG_OID: u8 = 0x06;
const TAG_CONTEXT_0: u8 = 0xA0;
/// `uniformResourceIdentifier [6] IA5String` of a `GeneralName`.
const TAG_URI: u8 = 0x86;

/// `id-pe-authorityInfoAccess` (1.3.6.1.5.5.7.1.1).
const OID_AIA: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x01];
/// `id-ad-caIssuers` (1.3.6.1.5.5.7.48.2).
const OID_CA_ISSUERS: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x02];
/// `id-signedData` (1.2.840.113549.1.7.2).
const OID_SIGNED_DATA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02];

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turn AIA fetching on or off for the process. The shell turns it off when
/// traffic goes through a proxy or Tor: the fetch is a direct connection.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether AIA fetching is on ([`set_enabled`]); on by default.
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The process-wide fetcher with its cache.
pub fn global() -> &'static Aia {
    static GLOBAL: OnceLock<Aia> = OnceLock::new();
    GLOBAL.get_or_init(Aia::new)
}

/// The `http://` `caIssuers` URLs of a certificate, in certificate order. Other
/// schemes (`ldap://`, `https://`) and other access methods (OCSP) are skipped.
#[must_use]
pub fn ca_issuer_urls(cert_der: &[u8]) -> Vec<String> {
    let Some(value) = cert_fields::extension_value(cert_der, OID_AIA) else {
        return Vec::new();
    };
    let Some(descriptions) = Der::new(value).read_tagged(TAG_SEQUENCE) else {
        return Vec::new();
    };
    let mut descriptions = Der::new(descriptions);
    let mut urls = Vec::new();
    while let Some(description) = descriptions.read_tagged(TAG_SEQUENCE) {
        let mut description = Der::new(description);
        let (Some(method), Some((tag, location))) = (description.read_tagged(TAG_OID), description.read_tlv())
        else {
            continue;
        };
        if method == OID_CA_ISSUERS && tag == TAG_URI {
            let url = String::from_utf8_lossy(location).into_owned();
            if parse_http_url(&url).is_some() {
                urls.push(url);
            }
        }
    }
    urls
}

/// The certificate of the chain `leaf` + `pool` that has no issuer in it — the
/// one whose issuer must be found. `None` when the chain already ends at a
/// self-issued certificate (nothing to look for) or cannot be read.
#[must_use]
pub fn chain_top<'a>(leaf: &'a [u8], pool: &'a [CertificateDer<'_>]) -> Option<&'a [u8]> {
    let mut current = leaf;
    for _ in 0..=pool.len() {
        let (issuer, subject) = cert_fields::issuer_and_subject_der(current)?;
        if issuer == subject {
            return None;
        }
        let next = pool.iter().map(AsRef::as_ref).find(|candidate| {
            *candidate != current
                && cert_fields::issuer_and_subject_der(candidate).is_some_and(|(_, s)| s == issuer)
        });
        match next {
            Some(next) => current = next,
            None => return Some(current),
        }
    }
    None
}

/// Downloads and caches issuer certificates ([`global`] is the one the verifier uses).
#[derive(Debug, Default)]
pub struct Aia {
    cache: Mutex<HashMap<String, CacheEntry>>,
}

#[derive(Debug, Clone)]
enum CacheEntry {
    Found(Vec<Vec<u8>>),
    Failed(Instant),
}

impl Aia {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Candidate certificates for the issuer of `cert_der`: the OS `CA` store's
    /// matches if any, otherwise what its AIA URLs serve. Only certificates
    /// whose subject equals `cert_der`'s issuer are returned.
    #[must_use]
    pub fn issuers_of(&self, cert_der: &[u8]) -> Vec<CertificateDer<'static>> {
        let Some((issuer, _)) = cert_fields::issuer_and_subject_der(cert_der) else {
            return Vec::new();
        };
        let wanted = |der: &[u8]| cert_fields::issuer_and_subject_der(der).is_some_and(|(_, s)| s == issuer);
        let local: Vec<_> = os_roots::os_intermediates().iter().filter(|c| wanted(c.as_ref())).cloned().collect();
        if !local.is_empty() {
            return local;
        }
        for url in ca_issuer_urls(cert_der).into_iter().take(MAX_URLS) {
            let found: Vec<_> = self
                .certs_at(&url)
                .into_iter()
                .filter(|der| wanted(der))
                .map(CertificateDer::from)
                .collect();
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// The certificates served at `url`, from the cache or one fresh download.
    fn certs_at(&self, url: &str) -> Vec<Vec<u8>> {
        if let Some(entry) = self.cache.lock().ok().and_then(|c| c.get(url).cloned()) {
            match entry {
                CacheEntry::Found(certs) => return certs,
                CacheEntry::Failed(at) if at.elapsed() < FAILURE_TTL => return Vec::new(),
                CacheEntry::Failed(_) => {}
            }
        }
        let certs = fetch(url).map(|body| parse_certificates(&body)).unwrap_or_default();
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= MAX_CACHE_ENTRIES {
                cache.clear();
            }
            let entry = if certs.is_empty() { CacheEntry::Failed(Instant::now()) } else { CacheEntry::Found(certs.clone()) };
            cache.insert(url.to_owned(), entry);
        }
        certs
    }
}

// ── HTTP ────────────────────────────────────────────────────────────────────

/// A parsed `http://host[:port]/path` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str) -> Option<HttpUrl> {
    let rest = url.get(..7).filter(|p| p.eq_ignore_ascii_case("http://")).map(|_| &url[7..])?;
    if rest.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return None;
    }
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_owned()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, String::from("/")),
    };
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            let port = match after.strip_prefix(':') {
                Some(p) => p.parse().ok()?,
                None if after.is_empty() => 80,
                None => return None,
            };
            (host, port)
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (authority, 80),
        },
    };
    (!host.is_empty()).then(|| HttpUrl { host: host.to_owned(), port, path })
}

/// Resolve `host:port`. Tests point every host at a local server.
fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    #[cfg(test)]
    if let Some(addr) = tests::TEST_ADDR.with(std::cell::Cell::get) {
        return Some(addr);
    }
    (host, port).to_socket_addrs().ok()?.next()
}

/// GET `url`, following up to [`MAX_REDIRECTS`] `http://` redirects, and return
/// the 200 body. `None` on any failure, timeout or oversize response.
fn fetch(url: &str) -> Option<Vec<u8>> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let mut url = parse_http_url(url)?;
    for _ in 0..=MAX_REDIRECTS {
        let response = get_once(&url, deadline)?;
        match response.status {
            200 => return Some(response.body),
            301 | 302 | 303 | 307 | 308 => url = parse_http_url(response.location.as_deref()?)?,
            _ => return None,
        }
    }
    None
}

#[derive(Debug)]
struct Response {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())
}

fn get_once(url: &HttpUrl, deadline: Instant) -> Option<Response> {
    let addr = resolve(&url.host, url.port)?;
    let mut stream = TcpStream::connect_timeout(&addr, remaining(deadline)?).ok()?;
    stream.set_write_timeout(remaining(deadline)).ok()?;
    let host_header = if url.port == 80 { url.host.clone() } else { format!("{}:{}", url.host, url.port) };
    let host_header = if url.host.contains(':') { format!("[{}]", url.host) } else { host_header };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {host_header}\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        url.path
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        stream.set_read_timeout(remaining(deadline)).ok()?;
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            // A reset or timeout after a complete reply still leaves it usable;
            // `parse_response` rejects a truncated one by `Content-Length`.
            Err(_) => break,
        }
        if raw.len() > MAX_BODY + MAX_HEADERS {
            return None;
        }
    }
    parse_response(&raw)
}

/// Split a raw HTTP/1.x reply into status, `Location` and body.
fn parse_response(raw: &[u8]) -> Option<Response> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    if split > MAX_HEADERS {
        return None;
    }
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let mut lines = head.split("\r\n");
    let mut status_line = lines.next()?.split(' ');
    if !status_line.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let status: u16 = status_line.next()?.parse().ok()?;
    let (mut location, mut content_length, mut chunked) = (None, None, false);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "location" => location = Some(value.to_owned()),
            "content-length" => content_length = value.parse::<usize>().ok(),
            "transfer-encoding" => chunked = value.to_ascii_lowercase().contains("chunked"),
            _ => {}
        }
    }
    let body = &raw[split + 4..];
    let body = if chunked {
        decode_chunked(body)?
    } else {
        match content_length {
            Some(n) if body.len() < n => return None,
            Some(n) => body[..n].to_vec(),
            None => body.to_vec(),
        }
    };
    (body.len() <= MAX_BODY).then_some(Response { status, location, body })
}

/// Decode a `Transfer-Encoding: chunked` body; `None` if it is malformed or
/// the terminating chunk never arrived.
fn decode_chunked(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = data.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..eol]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[eol + 2..];
        if size == 0 {
            return Some(out);
        }
        if data.len() < size + 2 || out.len() + size > MAX_BODY {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

// ── Response formats ────────────────────────────────────────────────────────

/// The certificates in an AIA response: one DER certificate, a PEM bundle or a
/// PKCS#7 `certs-only` message. Anything else yields an empty list.
#[must_use]
pub fn parse_certificates(body: &[u8]) -> Vec<Vec<u8>> {
    if body.trim_ascii_start().starts_with(b"-----BEGIN") {
        return rustls_pemfile::certs(&mut { body }).filter_map(Result::ok).map(|c| c.as_ref().to_vec()).collect();
    }
    let mut outer = Der::new(body);
    let Some(contents) = outer.read_tagged(TAG_SEQUENCE) else {
        return Vec::new();
    };
    match Der::new(contents).peek_tag() {
        Some(TAG_SEQUENCE) => vec![body[..outer.consumed()].to_vec()],
        Some(TAG_OID) => pkcs7_certificates(contents).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The `certificates [0]` of a PKCS#7 `ContentInfo { signedData, SignedData }`
/// (RFC 2315 §9.1), given the `ContentInfo`'s contents.
fn pkcs7_certificates(content_info: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut content_info = Der::new(content_info);
    if content_info.read_tagged(TAG_OID)? != OID_SIGNED_DATA {
        return None;
    }
    let signed_data = Der::new(content_info.read_tagged(TAG_CONTEXT_0)?).read_tagged(TAG_SEQUENCE)?;
    let mut signed_data = Der::new(signed_data);
    signed_data.read_tagged(TAG_INTEGER)?; // version
    signed_data.read_tagged(TAG_SET)?; // digestAlgorithms
    signed_data.read_tagged(TAG_SEQUENCE)?; // contentInfo
    let certificates = signed_data.read_tagged(TAG_CONTEXT_0)?;
    let mut reader = Der::new(certificates);
    let mut out = Vec::new();
    while !reader.is_empty() {
        let start = reader.consumed();
        let (tag, _) = reader.read_tlv()?;
        if tag == TAG_SEQUENCE {
            out.push(certificates[start..reader.consumed()].to_vec());
        }
    }
    Some(out)
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::Cell;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use super::*;

    thread_local! {
        /// Test hook: every host resolves to this address on the calling thread.
        pub(in crate::tls) static TEST_ADDR: Cell<Option<SocketAddr>> = const { Cell::new(None) };
    }

    pub(in crate::tls) const ROOT_PEM: &str = include_str!("testdata/aia/root.pem");
    pub(in crate::tls) const LEAF_PEM: &str = include_str!("testdata/aia/leaf.pem");
    pub(in crate::tls) const LEAF_P7C_PEM: &str = include_str!("testdata/aia/leaf2.pem");
    pub(in crate::tls) const INTERMEDIATE_DER: &[u8] = include_bytes!("testdata/aia/int.cer");
    pub(in crate::tls) const INTERMEDIATE_P7C: &[u8] = include_bytes!("testdata/aia/int.p7c");

    pub(in crate::tls) fn pem_to_der(pem: &str) -> Vec<u8> {
        rustls_pemfile::certs(&mut pem.as_bytes()).next().expect("one certificate").expect("valid PEM").as_ref().to_vec()
    }

    /// A one-thread HTTP server; `reply` maps the request path to the full raw
    /// response. Returns its address and the number of requests served.
    pub(in crate::tls) fn serve_with(
        reply: impl Fn(&str) -> Vec<u8> + Send + 'static,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0u8; 2048];
                let n = stream.read(&mut request).unwrap_or(0);
                counter.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&request[..n]).into_owned();
                let path = text.split(' ').nth(1).unwrap_or("/");
                let _ = stream.write_all(&reply(path));
            }
        });
        (addr, hits)
    }

    /// Raw HTTP/1.1 reply: `status` line text, extra `headers` (without CRLF), `body`.
    pub(in crate::tls) fn reply(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
        for header in headers {
            out.push_str(header);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut out = out.into_bytes();
        out.extend_from_slice(body);
        out
    }

    /// A server answering every request with `status` and `body`.
    pub(in crate::tls) fn serve(status: &'static str, body: Vec<u8>) -> (SocketAddr, Arc<AtomicUsize>) {
        serve_with(move |_| reply(status, &[], &body))
    }

    #[test]
    fn caissuers_url_is_read_from_the_leaf() {
        assert_eq!(ca_issuer_urls(&pem_to_der(LEAF_PEM)), ["http://aia.test.invalid/int.cer"]);
        assert_eq!(ca_issuer_urls(&pem_to_der(LEAF_P7C_PEM)), ["http://aia.test.invalid/int.p7c"]);
        assert!(ca_issuer_urls(&pem_to_der(ROOT_PEM)).is_empty());
        assert!(ca_issuer_urls(b"junk").is_empty());
    }

    #[test]
    fn url_parsing() {
        let u = parse_http_url("http://crt.example.com/a/b.cer").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("crt.example.com", 80, "/a/b.cer"));
        let u = parse_http_url("HTTP://h:8080").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path.as_str()), ("h", 8080, "/"));
        assert_eq!(parse_http_url("http://[::1]:81/x").unwrap().host, "::1");
        assert_eq!(parse_http_url("http://h?q=1").unwrap().path, "/?q=1");
        for bad in ["https://h/x", "ldap://h/x", "http://", "http://u@h/", "http://h/a b", "http://h:99999/", "x"] {
            assert!(parse_http_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn chain_top_finds_the_certificate_missing_its_issuer() {
        let leaf = pem_to_der(LEAF_PEM);
        let intermediate = CertificateDer::from(INTERMEDIATE_DER.to_vec());
        // Leaf alone: the leaf itself lacks an issuer.
        assert_eq!(chain_top(&leaf, &[]), Some(leaf.as_slice()));
        // With the intermediate the top is the intermediate (its issuer, the root, is absent).
        assert_eq!(chain_top(&leaf, std::slice::from_ref(&intermediate)), Some(INTERMEDIATE_DER));
        // A self-issued top has nothing to look for.
        let root = CertificateDer::from(pem_to_der(ROOT_PEM));
        assert_eq!(chain_top(&leaf, &[intermediate, root]), None);
    }

    #[test]
    fn response_formats_are_all_understood() {
        assert_eq!(parse_certificates(INTERMEDIATE_DER), [INTERMEDIATE_DER.to_vec()]);
        assert_eq!(parse_certificates(INTERMEDIATE_P7C), [INTERMEDIATE_DER.to_vec()]);
        let pem = format!("{ROOT_PEM}\n{LEAF_PEM}");
        assert_eq!(parse_certificates(pem.as_bytes()), [pem_to_der(ROOT_PEM), pem_to_der(LEAF_PEM)]);
        assert!(parse_certificates(b"<html>404</html>").is_empty());
        assert!(parse_certificates(&INTERMEDIATE_DER[..40]).is_empty());
        assert!(parse_certificates(&[]).is_empty());
    }

    #[test]
    fn http_reply_parsing() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, b"abc".as_slice()));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc").is_none(), "truncated");
        let r = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"abcde");
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc").is_none());
        let r = parse_response(b"HTTP/1.1 302 Found\r\nLocation: http://h/x\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.location.as_deref()), (302, Some("http://h/x")));
        assert!(parse_response(b"garbage").is_none());
    }

    #[test]
    fn oversize_body_is_refused() {
        let (addr, _) = serve("200 OK", vec![0x30; MAX_BODY + 1]);
        TEST_ADDR.with(|a| a.set(Some(addr)));
        assert!(fetch("http://aia.test.invalid/big.cer").is_none());
    }

    #[test]
    fn issuer_is_downloaded_once_and_then_cached() {
        let (addr, hits) = serve("200 OK", INTERMEDIATE_DER.to_vec());
        TEST_ADDR.with(|a| a.set(Some(addr)));
        let aia = Aia::new();
        let leaf = pem_to_der(LEAF_PEM);
        for _ in 0..3 {
            let found = aia.issuers_of(&leaf);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].as_ref(), INTERMEDIATE_DER);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_certificate_that_is_not_the_issuer_is_dropped() {
        let (addr, _) = serve("200 OK", pem_to_der(ROOT_PEM));
        TEST_ADDR.with(|a| a.set(Some(addr)));
        assert!(Aia::new().issuers_of(&pem_to_der(LEAF_PEM)).is_empty());
    }

    #[test]
    fn a_failed_download_is_not_retried_at_once() {
        let (addr, hits) = serve("404 Not Found", b"nope".to_vec());
        TEST_ADDR.with(|a| a.set(Some(addr)));
        let aia = Aia::new();
        let leaf = pem_to_der(LEAF_PEM);
        assert!(aia.issuers_of(&leaf).is_empty());
        assert!(aia.issuers_of(&leaf).is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn redirect_to_http_is_followed() {
        let (addr, hits) = serve_with(|path| {
            if path == "/start.cer" {
                reply("302 Found", &["Location: http://aia.test.invalid/real.cer"], b"")
            } else {
                reply("200 OK", &[], INTERMEDIATE_DER)
            }
        });
        TEST_ADDR.with(|a| a.set(Some(addr)));
        assert_eq!(fetch("http://aia.test.invalid/start.cer").as_deref(), Some(INTERMEDIATE_DER));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_redirect_to_https_is_not_followed() {
        let (addr, _) = serve_with(|_| reply("301 Moved", &["Location: https://aia.test.invalid/real.cer"], b""));
        TEST_ADDR.with(|a| a.set(Some(addr)));
        assert!(fetch("http://aia.test.invalid/start.cer").is_none());
    }

    /// Live check against a server that sends only its leaf:
    /// `cargo test -p lumen-network --lib aia::tests::live -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs internet access"]
    fn live_incomplete_chain_host_opens() {
        let probe = crate::tls::probe::probe_host("incomplete-chain.badssl.com", 443);
        eprintln!("{}", probe.to_json_line());
        assert!(probe.handshake_ok, "handshake failed: {}", probe.error);
    }
}
