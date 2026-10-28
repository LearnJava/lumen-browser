//! `HttpClient::fetch_request` through the shared HTTP cache (FETCH-HTTP-CACHE):
//! the request cache modes, revalidation and invalidation over a real socket.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Mutex;
use std::thread;

/// Accepts `replies.len()` connections, answers them in order and records each
/// request head (request line + headers, `\r\n`-separated, lowercased names).
fn scripted_server(replies: Vec<&'static [u8]>) -> (u16, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = bind_ephemeral_listener();
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let heads_w = Arc::clone(&heads);
    let handle = thread::spawn(move || {
        for reply in replies {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut head = String::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
                head.push_str(&line);
            }
            let mut body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
            heads_w.lock().unwrap().push(head);
            let _ = sock.write_all(reply);
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
    });
    (port, heads, handle)
}

fn get(client: &HttpClient, url: &str, cache: &str, headers: &[(String, String)]) -> JsFetchResult {
    client
        .fetch_request(&JsFetchRequest {
            url,
            method: "GET",
            headers,
            body: None,
            mode: "",
            destination: "",
            cache,
            token: None,
        })
        .expect("fetch_request")
}

fn client_with(cache: &Arc<http_cache::HttpCache>) -> HttpClient {
    let backend: Arc<dyn http_cache::HttpCacheBackend> = Arc::clone(cache) as _;
    HttpClient::new().with_http_cache(backend)
}

const FRESH: &[u8] =
    b"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody";

fn header_of<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

#[test]
fn fetch_serves_a_fresh_response_from_the_cache_the_second_time() {
    let (port, heads, server) = scripted_server(vec![FRESH]);
    let url = format!("http://127.0.0.1:{port}/data");
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));

    assert_eq!(get(&client, &url, "", &[]).body, b"body");
    // The server thread accepted once and is gone: only the cache can answer.
    let second = get(&client, &url, "default", &[]);
    assert_eq!((second.status, second.body.as_slice()), (200, b"body".as_slice()));
    server.join().unwrap();
    assert_eq!(heads.lock().unwrap().len(), 1);
}

#[test]
fn default_mode_does_not_send_the_navigation_max_age_zero() {
    let (port, heads, server) = scripted_server(vec![FRESH]);
    let url = format!("http://127.0.0.1:{port}/data");
    get(&client_with(&Arc::new(http_cache::HttpCache::new())), &url, "", &[]);
    server.join().unwrap();
    assert_eq!(header_of(&heads.lock().unwrap()[0], "cache-control"), None);
}

#[test]
fn cache_modes_put_their_request_lines_on_the_wire() {
    let (port, heads, server) = scripted_server(vec![FRESH, FRESH, FRESH]);
    let url = format!("http://127.0.0.1:{port}/data");
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));
    get(&client, &url, "no-cache", &[]); // empty cache: plain request carrying max-age=0
    get(&client, &url, "no-store", &[]);
    let author = vec![("pragma".to_owned(), "foo".to_owned())];
    get(&client, &url, "reload", &author);
    server.join().unwrap();
    let heads = heads.lock().unwrap();
    assert_eq!(header_of(&heads[0], "cache-control"), Some("max-age=0"));
    assert_eq!(header_of(&heads[1], "cache-control"), Some("no-cache"));
    assert_eq!(header_of(&heads[1], "pragma"), Some("no-cache"));
    assert_eq!(header_of(&heads[2], "pragma"), Some("foo"), "the author's Pragma stays");
}

#[test]
fn no_store_mode_neither_reads_nor_writes_the_cache() {
    let (port, heads, server) = scripted_server(vec![FRESH, FRESH, FRESH]);
    let url = format!("http://127.0.0.1:{port}/data");
    let mem = Arc::new(http_cache::HttpCache::new());
    let client = client_with(&mem);
    get(&client, &url, "no-store", &[]);
    assert_eq!(mem.len(), 0);
    // A stored copy exists now; `no-store` still goes to the origin.
    get(&client, &url, "default", &[]);
    assert_eq!(mem.len(), 1);
    get(&client, &url, "no-store", &[]);
    server.join().unwrap();
    assert_eq!(heads.lock().unwrap().len(), 3, "every no-store fetch reaches the origin");
}

#[test]
fn stale_entry_is_revalidated_and_a_304_serves_the_merged_stored_copy() {
    let first: &[u8] = b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nTest-Header: A\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody";
    let second: &[u8] = b"HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nTest-Header: B\r\nCache-Control: max-age=600\r\nConnection: close\r\n\r\n";
    let (port, heads, server) = scripted_server(vec![first, second]);
    let url = format!("http://127.0.0.1:{port}/data");
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));

    assert_eq!(get(&client, &url, "", &[]).status, 200);
    let revalidated = get(&client, &url, "", &[]);
    server.join().unwrap();

    assert_eq!(header_of(&heads.lock().unwrap()[1], "if-none-match"), Some("\"v1\""));
    assert_eq!(revalidated.status, 200, "the page never sees the 304");
    assert_eq!(revalidated.body, b"body");
    let test_header = revalidated.headers.iter().find(|(k, _)| k == "test-header").map(|(_, v)| v.as_str());
    assert_eq!(test_header, Some("B"), "headers of the 304 win");
    // Fresh again after the 304's max-age: a third fetch needs no server.
    assert_eq!(get(&client, &url, "", &[]).body, b"body");
}

#[test]
fn force_cache_serves_a_stale_entry_and_only_if_cached_errors_on_a_miss() {
    let stale: &[u8] = b"HTTP/1.1 200 OK\r\nCache-Control: max-age=0\r\nETag: \"v1\"\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody";
    let (port, _heads, server) = scripted_server(vec![stale]);
    let url = format!("http://127.0.0.1:{port}/data");
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));
    let miss = client.fetch_request(&JsFetchRequest {
        url: &url,
        method: "GET",
        headers: &[],
        body: None,
        mode: "",
        destination: "",
        cache: "only-if-cached",
        token: None,
    });
    assert!(miss.is_err(), "only-if-cached with nothing stored is a network error");
    get(&client, &url, "", &[]);
    server.join().unwrap();
    assert_eq!(get(&client, &url, "force-cache", &[]).body, b"body");
    assert_eq!(get(&client, &url, "only-if-cached", &[]).body, b"body");
}

#[test]
fn cache_control_only_if_cached_request_without_an_entry_is_a_504() {
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));
    let headers = vec![("cache-control".to_owned(), "only-if-cached".to_owned())];
    let r = get(&client, "http://127.0.0.1:1/never-connected", "", &headers);
    assert_eq!((r.status, r.body.len()), (504, 0));
}

#[test]
fn vary_mismatch_goes_back_to_the_origin() {
    let varying: &[u8] = b"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nVary: Foo\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody";
    let (port, heads, server) = scripted_server(vec![varying, varying]);
    let url = format!("http://127.0.0.1:{port}/data");
    let client = client_with(&Arc::new(http_cache::HttpCache::new()));
    let foo = |v: &str| vec![("foo".to_owned(), v.to_owned())];
    get(&client, &url, "", &foo("1"));
    get(&client, &url, "", &foo("1")); // same variant: from the cache
    let other = get(&client, &url, "", &foo("2")); // other variant: second request
    server.join().unwrap();
    assert_eq!(heads.lock().unwrap().len(), 2);
    assert!(
        other.headers.iter().all(|(k, _)| k != fetch_cache::VARY_MARKER_HEADER),
        "the cache's bookkeeping header never reaches the page"
    );
}

#[test]
fn a_successful_unsafe_request_invalidates_the_stored_response() {
    let created: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    let failed: &[u8] = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let (port, _heads, server) = scripted_server(vec![FRESH, failed, created]);
    let url = format!("http://127.0.0.1:{port}/data");
    let mem = Arc::new(http_cache::HttpCache::new());
    let client = client_with(&mem);
    get(&client, &url, "", &[]);
    assert_eq!(mem.len(), 1);

    let post = |client: &HttpClient| {
        client
            .fetch_request(&JsFetchRequest {
                url: &url,
                method: "POST",
                headers: &[],
                body: Some(JsFetchBody { content_type: "text/plain", bytes: b"abc" }),
                mode: "",
                destination: "",
                cache: "",
                token: None,
            })
            .expect("post")
    };
    assert_eq!(post(&client).status, 500);
    assert_eq!(mem.len(), 1, "a failed unsafe request leaves the cache alone");
    assert_eq!(post(&client).status, 200);
    server.join().unwrap();
    assert_eq!(mem.len(), 0, "a successful one drops the entry");
}

#[test]
fn drop_marker_strips_the_profile_cache_control_on_both_wire_formats() {
    let extra = "x-lumen-drop: cache-control\r\nfoo: bar\r\n";
    let h1 = http::headers::build_request_headers("e.test", "", extra, HttpProfile::Chrome);
    assert!(!h1.to_ascii_lowercase().contains("cache-control"), "{h1}");
    assert!(!h1.to_ascii_lowercase().contains("x-lumen-drop"), "{h1}");
    assert!(h1.contains("foo: bar"));
    let with_default = http::headers::build_request_headers("e.test", "", "foo: bar\r\n", HttpProfile::Chrome);
    assert!(with_default.contains("Cache-Control: max-age=0"), "control: the default is there");

    let h2 = build_h2_headers(HttpProfile::Chrome, None, extra);
    assert!(h2.iter().all(|(k, _)| k != b"cache-control" && k != b"x-lumen-drop"));
    assert!(h2.iter().any(|(k, v)| k == b"foo" && v == b"bar"));
}
