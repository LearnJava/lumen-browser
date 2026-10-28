//! Proxy, `base64`, `data:`/`file:` URL tests of the crate root (moved out of
//! `network/lib.rs` verbatim, file-size gate docs/lint-policy.md §5.1).

use super::*;

#[test]
fn http_proxy_new_no_auth() {
    let proxy = HttpProxy::new("proxy.local".to_string(), 3128);
    assert_eq!(proxy.host, "proxy.local");
    assert_eq!(proxy.port, 3128);
    assert_eq!(proxy.auth, None);
}

#[test]
fn http_proxy_with_basic_auth() {
    let proxy = HttpProxy::new("proxy.local".to_string(), 3128)
        .with_basic_auth("user", "pass");
    assert_eq!(proxy.host, "proxy.local");
    assert_eq!(proxy.port, 3128);
    assert!(proxy.auth.is_some());
    // Basic auth for "user:pass" should be base64-encoded "dXNlcjpwYXNz"
    assert_eq!(proxy.auth.as_ref().unwrap(), "dXNlcjpwYXNz");
}

#[test]
fn http_client_with_proxy() {
    let proxy = Arc::new(HttpProxy::new("proxy.local".to_string(), 3128));
    let client = HttpClient::new().with_proxy(Arc::clone(&proxy));
    // Verify that the proxy was attached (no public accessor, so we just verify it doesn't crash)
    assert!(client.proxy.is_some());
}

// ── BUG-1209: pool key за HTTP-прокси ключуется целевым origin-ом ──────

#[test]
fn pool_key_direct_route_uses_target_host() {
    // Без прокси ключ — всегда реальный (target) host:port, как раньше.
    let key = pool_key_for_fetch(false, true, "a.example", 443, "a.example", 443, true);
    assert_eq!(key.host, "a.example");
    assert_eq!(key.port, 443);
    assert!(key.is_tls);
}

#[test]
fn pool_key_https_over_proxy_uses_target_not_proxy() {
    // За HTTP-прокси, после CONNECT-туннеля к TLS-хосту, ключ обязан
    // называть реальный target — connect_host/connect_port здесь всегда
    // адрес прокси (см. fetch_single), а ключ должен их игнорировать.
    let key = pool_key_for_fetch(true, true, "a.example", 443, "proxy.local", 3128, false);
    assert_eq!(key.host, "a.example");
    assert_eq!(key.port, 443);
    assert!(key.is_tls);
}

#[test]
fn pool_key_https_over_proxy_distinguishes_two_hosts() {
    // Регрессия BUG-1209: два разных TLS-хоста за одним прокси не
    // должны получить один и тот же ключ пула (иначе acquire() отдаёт
    // TLS-сессию host-A запросу к host-B → 421 Misdirected Request).
    let key_a = pool_key_for_fetch(true, true, "a.example", 443, "proxy.local", 3128, false);
    let key_b = pool_key_for_fetch(true, true, "b.example", 443, "proxy.local", 3128, false);
    assert_ne!(key_a, key_b);
}

#[test]
fn pool_key_plain_http_over_proxy_uses_proxy_address() {
    // Обычный (не-TLS) relay через прокси остаётся общим по адресу
    // прокси: одно TCP-соединение легитимно обслуживает разные Host
    // через absolute-URI в строке запроса.
    let key = pool_key_for_fetch(true, false, "a.example", 80, "proxy.local", 3128, false);
    assert_eq!(key.host, "proxy.local");
    assert_eq!(key.port, 3128);
    assert!(!key.is_tls);
}

#[test]
fn base64_encode_empty_string() {
    let encoded = base64_encode("");
    assert_eq!(encoded, "");
}

#[test]
fn base64_encode_single_byte() {
    let encoded = base64_encode("a");
    assert_eq!(encoded, "YQ==");
}

#[test]
fn base64_encode_user_pass() {
    let encoded = base64_encode("user:pass");
    assert_eq!(encoded, "dXNlcjpwYXNz");
}

#[test]
fn base64_decode_roundtrips_with_encode() {
    for s in ["", "a", "ab", "abc", "user:pass", "Hello, World!"] {
        let encoded = base64_encode(s);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, s.as_bytes());
    }
}

#[test]
fn base64_decode_rejects_bad_length() {
    assert!(base64_decode("abcde").is_err());
}

#[test]
fn base64_decode_rejects_bad_alphabet() {
    assert!(base64_decode("ab!=").is_err());
}

#[test]
fn base64_decode_rejects_interior_padding() {
    assert!(base64_decode("A=BC").is_err());
}

#[test]
fn base64_decode_rejects_padding_before_last_chunk() {
    assert!(base64_decode("AA==YWJj").is_err());
}

#[test]
fn percent_decode_bytes_basic() {
    assert_eq!(percent_decode_bytes("Hello%2C%20World%21"), b"Hello, World!");
}

#[test]
fn percent_decode_bytes_passes_through_lone_percent() {
    // A trailing `%` with no (or invalid) hex digits after it is not an
    // error — it passes through unchanged, same leniency the rest of the
    // URL parsing in this crate already takes.
    assert_eq!(percent_decode_bytes("100%"), b"100%");
    assert_eq!(percent_decode_bytes("100%2"), b"100%2");
    assert_eq!(percent_decode_bytes("100%zz"), b"100%zz");
}

#[test]
fn data_url_base64() {
    let url = Url::parse("data:text/plain;base64,SGVsbG8=").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain");
    assert_eq!(body, b"Hello");
}

#[test]
fn data_url_percent_encoded() {
    let url = Url::parse("data:text/plain,Hello%2C%20World%21").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain");
    assert_eq!(body, b"Hello, World!");
}

#[test]
fn data_url_default_mediatype() {
    // RFC 2397: absent mediatype defaults to text/plain;charset=US-ASCII.
    let url = Url::parse("data:,hello").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain;charset=US-ASCII");
    assert_eq!(body, b"hello");
}

#[test]
fn data_url_base64_with_empty_mediatype() {
    let url = Url::parse("data:;base64,SGVsbG8=").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain;charset=US-ASCII");
    assert_eq!(body, b"Hello");
}

#[test]
fn data_url_empty_body() {
    let url = Url::parse("data:text/plain,").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain");
    assert!(body.is_empty());
}

#[test]
fn data_url_missing_comma_fails() {
    let url = Url::parse("data:text/plain").unwrap();
    assert!(parse_data_url(&url).is_err());
}

#[test]
fn data_url_bad_base64_fails() {
    let url = Url::parse("data:text/plain;base64,not-valid-base64!!!").unwrap();
    assert!(parse_data_url(&url).is_err());
}

#[test]
fn data_url_base64_with_whitespace_is_forgiving() {
    // Real-world data: URLs are frequently wrapped across multiple lines
    // in CSS/HTML source; the base64 payload picks up newlines/spaces.
    let url = Url::parse("data:image/png;base64,aGVs\nbG8=").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "image/png");
    assert_eq!(body, b"hello");
}

#[test]
fn data_url_query_char_preserved_in_body() {
    // A literal `?` inside the (non-base64) data must survive — the
    // WHATWG URL parser would otherwise split it off as a query
    // component; `path_and_query()` must splice it back before the
    // comma search runs.
    let url = Url::parse("data:text/plain,a?b=c").unwrap();
    let (content_type, body) = parse_data_url(&url).unwrap();
    assert_eq!(content_type, "text/plain");
    assert_eq!(body, b"a?b=c");
}

#[test]
fn fetch_subresource_data_url_returns_body_without_network() {
    // Port 0 in the URL would fail to connect — proving this never
    // touches the network. Covers the "one path for every subresource
    // type" requirement: images, CSS, fonts and scripts all call
    // `fetch_subresource`.
    let client = HttpClient::new();
    let url = Url::parse("data:text/css,body%7Bcolor:red%7D").unwrap();
    let body = client.fetch_subresource(&url, RequestDestination::Style).unwrap();
    assert_eq!(body, b"body{color:red}");
}

#[test]
fn fetch_page_data_url_returns_body_without_network() {
    let client = HttpClient::new();
    let url = Url::parse("data:text/html,%3Ch1%3Ehi%3C%2Fh1%3E").unwrap();
    let page = client.fetch_page(&url, None, false).unwrap();
    assert_eq!(page.body, b"<h1>hi</h1>");
}

#[test]
fn guess_file_content_type_covers_script_and_style() {
    assert_eq!(guess_file_content_type(std::path::Path::new("a.js")), "text/javascript");
    assert_eq!(guess_file_content_type(std::path::Path::new("a.css")), "text/css");
    assert_eq!(guess_file_content_type(std::path::Path::new("a.unknownext")), "application/octet-stream");
}

#[test]
fn fetch_request_file_url_reads_local_disk_without_network() {
    // BUG-723: before the fix, any `file:` URL died on
    // `require_http_scheme` ("unsupported scheme: file") — a page opened
    // from disk could never `fetch()`/dynamically `<script src>`/`<link>`
    // its own neighbouring files. Port 0 would fail to connect, so a pass
    // here proves the network was never touched.
    let dir = std::env::temp_dir().join(format!(
        "lumen-bug723-{:?}-{}",
        std::thread::current().id(),
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("b723.js");
    std::fs::write(&file_path, b"window.__b723_ran = true;").unwrap();

    let url = lumen_core::url::Url::parse(
        &format!("file:///{}", file_path.display().to_string().replace('\\', "/")),
    )
    .unwrap();
    let client = HttpClient::new();
    let result = client
        .fetch_request(&JsFetchRequest {
            url: url.as_str(),
            method: "GET",
            headers: &[],
            body: None,
            mode: "",
            destination: "",
            cache: "",
            token: None,
        })
        .unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body, b"window.__b723_ran = true;");
    assert!(
        result.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type")
            && v == "text/javascript"),
        "headers: {:?}",
        result.headers
    );

    std::fs::remove_dir_all(&dir).ok();
}
