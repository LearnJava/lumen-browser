//! COMPAT-PRIVACY-UNBREAK-LOCALE: the shell-provided `Accept-Language` reaches
//! the wire on both the HTTP/1.1 and the HTTP/2 (and H3) header paths, and
//! never reaches a Tor Browser request.
//!
//! The value is a process-global, so every test of this binary that touches it
//! (or asserts the built-in default) holds [`ACCEPT_LANGUAGE_LOCK`].

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;

use lumen_core::url::Url;
use lumen_network::http::{
    accept_language_header, build_request_headers, expand_language_list, h2_fingerprint_headers,
    set_global_accept_language,
};
use lumen_network::{HttpClient, HttpProfile};

/// Serialises tests that set or read the process-global `Accept-Language`.
pub static ACCEPT_LANGUAGE_LOCK: Mutex<()> = Mutex::new(());

const RU: &str = "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7";

/// Restores the built-in default when a test ends, even by panic.
struct ClearOnDrop;

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        set_global_accept_language(None);
    }
}

fn ru_header() -> String {
    let langs = ["ru-RU".to_owned(), "en-US".to_owned()];
    accept_language_header(&expand_language_list(&langs))
}

/// One-shot echo server: answers 200 and hands back the request head.
fn echo_server() -> (u16, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(sock.try_clone().expect("clone"));
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let blank = line == "\r\n";
            head.push_str(&line);
            if blank {
                break;
            }
        }
        let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        head
    });
    (port, handle)
}

#[test]
fn expanded_windows_list_becomes_the_chrome_header() {
    assert_eq!(ru_header(), RU);
}

#[test]
fn h1_request_on_the_wire_carries_the_shell_locale() {
    let _lock = ACCEPT_LANGUAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _clear = ClearOnDrop;
    set_global_accept_language(Some(ru_header()));
    let (port, server) = echo_server();
    let url = Url::parse(&format!("http://127.0.0.1:{port}/")).expect("url");
    let page = HttpClient::new().fetch_page(&url, None, false).expect("navigation");
    assert_eq!(page.body, b"ok");
    let head = server.join().expect("server");
    assert!(head.contains(&format!("Accept-Language: {RU}\r\n")), "head: {head}");
    assert_eq!(head.matches("Accept-Language").count(), 1, "head: {head}");
}

#[test]
fn h2_headers_carry_the_same_locale_as_h1() {
    let _lock = ACCEPT_LANGUAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _clear = ClearOnDrop;
    set_global_accept_language(Some(ru_header()));
    for profile in [
        HttpProfile::Chrome,
        HttpProfile::Strict,
        HttpProfile::Edge,
        HttpProfile::Firefox,
        HttpProfile::Safari,
        HttpProfile::Lumen,
    ] {
        let h2 = h2_fingerprint_headers(profile, "");
        let value = h2.iter().find(|(n, _)| n == "accept-language").map(|(_, v)| v.as_str());
        assert_eq!(value, Some(RU), "{profile:?}");
        let h1 = build_request_headers("example.com", "", "", profile);
        assert!(h1.contains(&format!("Accept-Language: {RU}\r\n")), "{profile:?}: {h1}");
    }
}

#[test]
fn tor_browser_never_takes_the_override() {
    let _lock = ACCEPT_LANGUAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _clear = ClearOnDrop;
    set_global_accept_language(Some(ru_header()));
    let h1 = build_request_headers("example.com", "", "", HttpProfile::TorBrowser);
    assert!(h1.contains("Accept-Language: en-US,en;q=0.5\r\n"), "{h1}");
    let h2 = h2_fingerprint_headers(HttpProfile::TorBrowser, "");
    assert!(h2.contains(&("accept-language".to_owned(), "en-US,en;q=0.5".to_owned())), "{h2:?}");
}

#[test]
fn author_accept_language_still_wins_and_clearing_restores_the_default() {
    let _lock = ACCEPT_LANGUAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _clear = ClearOnDrop;
    set_global_accept_language(Some(ru_header()));
    let h1 = build_request_headers("example.com", "", "Accept-Language: fr\r\n", HttpProfile::Chrome);
    assert!(h1.contains("Accept-Language: fr\r\n"), "{h1}");
    assert_eq!(h1.matches("Accept-Language").count(), 1, "{h1}");

    set_global_accept_language(None);
    let h1 = build_request_headers("example.com", "", "", HttpProfile::Chrome);
    assert!(h1.contains("Accept-Language: en-US,en;q=0.9\r\n"), "{h1}");
}

#[test]
fn header_injection_through_the_setter_is_refused() {
    let _lock = ACCEPT_LANGUAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _clear = ClearOnDrop;
    set_global_accept_language(Some("ru\r\nX-Evil: 1".to_owned()));
    let h1 = build_request_headers("example.com", "", "", HttpProfile::Chrome);
    assert!(!h1.contains("X-Evil"), "{h1}");
}
