//! HTTP cache integration tests of `HttpClient` (moved out of `network/lib.rs`
//! verbatim, file-size gate docs/lint-policy.md §5.1).

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::thread;

fn mock_server<F>(accept_count: usize, responder: F) -> (u16, thread::JoinHandle<()>)
where
    F: Fn(usize) -> Vec<u8> + Send + 'static,
{
    let listener = bind_ephemeral_listener();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        for i in 1..=accept_count {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                if line == "\r\n" || line == "\n" || line.is_empty() { break; }
            }
            let _ = sock.write_all(&responder(i));
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
    });
    (port, handle)
}

#[test]
fn http_cache_fresh_hit_skips_network() {
    // Server accepts exactly 1 connection. Cache has a fresh entry.
    // If the cache is used, the server never gets a request → test passes.
    // If cache is bypassed, the server closes after 1 response and the
    // second request would also go there (we only start 1 server accept).
    let cache: Arc<dyn http_cache::HttpCacheBackend> =
        Arc::new(http_cache::HttpCache::new());
    let url = "http://127.0.0.1:0/resource"; // port 0 → unused, never connected
    // Pre-populate cache with a fresh entry (max-age=3600).
    cache.store(
        url,
        200,
        b"cached body".to_vec(),
        &[
            ("Cache-Control".to_owned(), "max-age=3600".to_owned()),
            ("ETag".to_owned(), "\"v1\"".to_owned()),
        ],
    );
    let parsed = Url::parse(url).unwrap();
    // HttpClient with the cache — should return the cached body without
    // connecting (port 0 would fail to connect).
    let client = HttpClient::new().with_http_cache(cache);
    let result = client.fetch(&parsed).unwrap();
    assert_eq!(result, b"cached body");
}

#[test]
fn http_cache_miss_fetches_and_stores() {
    // Server returns 200 with Cache-Control: max-age=60 and ETag.
    // First request should hit network and populate cache.
    // Second request should be served from cache (server only accepts once).
    let (port, server) = mock_server(1, |_| {
        b"HTTP/1.1 200 OK\r\nCache-Control: max-age=60\r\nETag: \"v1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
            .to_vec()
    });

    let mem_cache = Arc::new(http_cache::HttpCache::new());
    let cache: Arc<dyn http_cache::HttpCacheBackend> = Arc::clone(&mem_cache) as _;
    let url = format!("http://127.0.0.1:{port}/data");
    let parsed = Url::parse(&url).unwrap();

    let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

    // First fetch — goes to network.
    let body1 = client.fetch(&parsed).unwrap();
    assert_eq!(body1, b"body");
    assert_eq!(mem_cache.len(), 1, "should have stored the response");

    // Second fetch — served from cache (server thread has exited).
    let body2 = client.fetch(&parsed).unwrap();
    assert_eq!(body2, b"body");

    server.join().unwrap();
}

/// Server that answers once and hands back the raw request head.
fn recording_server(response: &'static [u8]) -> (u16, Arc<std::sync::Mutex<String>>, thread::JoinHandle<()>) {
    let listener = bind_ephemeral_listener();
    let port = listener.local_addr().unwrap().port();
    let head = Arc::new(std::sync::Mutex::new(String::new()));
    let head_w = Arc::clone(&head);
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(sock.try_clone().unwrap());
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
            if line == "\r\n" || line == "\n" { break; }
            head_w.lock().unwrap().push_str(&line);
        }
        let _ = sock.write_all(response);
        let _ = sock.shutdown(std::net::Shutdown::Both);
    });
    (port, head, handle)
}

const FRESH_CACHE_HEADERS: &[(&str, &str)] = &[("Cache-Control", "max-age=3600")];

fn cache_with_fresh_entry(url: &str) -> Arc<http_cache::HttpCache> {
    let cache = Arc::new(http_cache::HttpCache::new());
    let headers: Vec<(String, String)> = FRESH_CACHE_HEADERS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    cache.store(url, 200, b"stale".to_vec(), &headers);
    cache
}

const FRESH_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nContent-Length: 5\r\nConnection: close\r\n\r\nfresh";

#[test]
fn hard_reload_navigation_skips_fresh_cache_entry() {
    let (port, head, server) = recording_server(FRESH_RESPONSE);
    let url = format!("http://127.0.0.1:{port}/page");
    let mem_cache = cache_with_fresh_entry(&url);
    let cache: Arc<dyn http_cache::HttpCacheBackend> = Arc::clone(&mem_cache) as _;
    let parsed = Url::parse(&url).unwrap();

    // A normal navigation is served from the fresh entry — no network.
    let plain = HttpClient::new().with_http_cache(Arc::clone(&cache));
    assert_eq!(plain.fetch_page(&parsed, None, false).unwrap().body, b"stale");

    let hard = HttpClient::new().with_http_cache(Arc::clone(&cache)).with_cache_bypass(true);
    assert_eq!(hard.fetch_page(&parsed, None, false).unwrap().body, b"fresh");
    server.join().unwrap();

    let head = head.lock().unwrap().to_ascii_lowercase();
    assert!(head.contains("cache-control: no-cache\r\n"), "{head}");
    assert!(head.contains("pragma: no-cache\r\n"), "{head}");
    assert_eq!(head.matches("cache-control:").count(), 1, "the profile default must be replaced: {head}");
    // The fresh response replaces the entry for the next, ordinary load.
    assert_eq!(plain.fetch_page(&parsed, None, false).unwrap().body, b"fresh");
}

#[test]
fn hard_reload_subresource_skips_fresh_cache_entry() {
    let (port, head, server) = recording_server(FRESH_RESPONSE);
    let url = format!("http://127.0.0.1:{port}/style.css");
    let mem_cache = cache_with_fresh_entry(&url);
    let cache: Arc<dyn http_cache::HttpCacheBackend> = Arc::clone(&mem_cache) as _;
    let parsed = Url::parse(&url).unwrap();

    let hard = HttpClient::new().with_http_cache(cache).with_cache_bypass(true);
    let (body, _) = hard
        .fetch_subresource_with_content_type(&parsed, RequestDestination::Style)
        .unwrap();
    assert_eq!(body, b"fresh");
    server.join().unwrap();
    let head = head.lock().unwrap().to_ascii_lowercase();
    assert!(head.contains("cache-control: no-cache\r\n"), "{head}");
    assert!(head.contains("pragma: no-cache\r\n"), "{head}");
}

#[test]
fn ordinary_request_carries_no_pragma() {
    let (port, head, server) = recording_server(FRESH_RESPONSE);
    let parsed = Url::parse(&format!("http://127.0.0.1:{port}/page")).unwrap();
    HttpClient::new().fetch_page(&parsed, None, false).unwrap();
    server.join().unwrap();
    let head = head.lock().unwrap().to_ascii_lowercase();
    assert!(!head.contains("pragma:"), "{head}");
    assert!(!head.contains("cache-control: no-cache"), "{head}");
}

#[test]
fn http_cache_stale_sends_conditional_get_304() {
    // Server sequence: first request fills cache (max-age=0, ETag=v1).
    // Second request is conditional GET → server returns 304.
    // Client should return the cached body.
    let (port, server) = mock_server(2, |i| match i {
        1 => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
            .to_vec(),
        _ => b"HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n"
            .to_vec(),
    });

    let cache: Arc<dyn http_cache::HttpCacheBackend> =
        Arc::new(http_cache::HttpCache::new());
    let url = format!("http://127.0.0.1:{port}/data");
    let parsed = Url::parse(&url).unwrap();
    let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

    // First fetch.
    let body1 = client.fetch(&parsed).unwrap();
    assert_eq!(body1, b"body");

    // Second fetch — stale entry, sends conditional GET, gets 304.
    let body2 = client.fetch(&parsed).unwrap();
    assert_eq!(body2, b"body", "should return cached body after 304");

    server.join().unwrap();
}

#[test]
fn http_cache_stale_new_200_updates_cache() {
    // Server returns 200 twice (max-age=0, ETag changes between responses).
    // Second fetch gets a new 200 (not 304); cache should be updated.
    let (port, server) = mock_server(2, |i| match i {
        1 => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\nv1"
            .to_vec(),
        _ => b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v2\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\nv2"
            .to_vec(),
    });

    let cache: Arc<dyn http_cache::HttpCacheBackend> =
        Arc::new(http_cache::HttpCache::new());
    let url = format!("http://127.0.0.1:{port}/data");
    let parsed = Url::parse(&url).unwrap();
    let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

    let body1 = client.fetch(&parsed).unwrap();
    assert_eq!(body1, b"v1");

    let body2 = client.fetch(&parsed).unwrap();
    assert_eq!(body2, b"v2", "cache should update on new 200");

    let snap = cache.get(&url).unwrap();
    assert_eq!(snap.etag.as_deref(), Some("\"v2\""));

    server.join().unwrap();
}

#[test]
fn http_cache_no_store_never_cached() {
    let (port, server) = mock_server(2, |_| {
        b"HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata"
            .to_vec()
    });

    let mem_cache = Arc::new(http_cache::HttpCache::new());
    let cache: Arc<dyn http_cache::HttpCacheBackend> = Arc::clone(&mem_cache) as _;
    let url = format!("http://127.0.0.1:{port}/secret");
    let parsed = Url::parse(&url).unwrap();
    let client = HttpClient::new().with_http_cache(Arc::clone(&cache));

    client.fetch(&parsed).unwrap();
    assert_eq!(mem_cache.len(), 0, "no-store must not be cached");

    // Second fetch also goes to network (not cached).
    client.fetch(&parsed).unwrap();

    server.join().unwrap();
}
