//! E2E-1: навигация с телом — `HttpClient::fetch_page`/`fetch_page_streaming`
//! с `Some(NavigationBody)` должны отправить настоящий POST (метод, заголовок
//! `Content-Type`, байты тела) и провести его по той же цепочке редиректов,
//! что и GET-навигацию.
//!
//! Тесты живут здесь, а не в `mod tests` внутри `crates/network/src/lib.rs`:
//! проверяется публичная поверхность крейта, а сам `lib.rs` давно превысил
//! потолок `scripts/check_file_sizes.py` и расти не должен.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use lumen_core::url::Url;
use lumen_network::{HttpClient, NavigationBody};

/// `expect()`/`unwrap()` разрешены линтом только внутри тела `#[test]`
/// (`clippy.toml`, `allow-*-in-tests`), поэтому хелпер ниже пробрасывает
/// `Result`, а разворачивают его сами тесты — тот же приём, что и в
/// `bug785_extra_ca.rs`.
type BoxError = Box<dyn std::error::Error>;

/// Mock-сервер: обслуживает `accept_count` соединений подряд, складывает
/// полный текст каждого запроса (заголовки + тело по `Content-Length`) в
/// `captured` и отвечает `responder(i)` на i-й запрос.
///
/// Собственный, а не общий с `mod tests` в `lib.rs`: тот приватен для крейта
/// и интеграционному тесту не виден.
fn capturing_server<F>(
    accept_count: usize,
    captured: Arc<Mutex<Vec<String>>>,
    responder: F,
) -> Result<(u16, thread::JoinHandle<()>), BoxError>
where
    F: Fn(usize) -> Vec<u8> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = thread::spawn(move || {
        for i in 1..=accept_count {
            let Ok((mut sock, _)) = listener.accept() else { return };
            let Ok(clone) = sock.try_clone() else { return };
            let mut reader = BufReader::new(clone);
            let mut request = String::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let is_blank = line == "\r\n" || line == "\n";
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
                request.push_str(&line);
                if is_blank {
                    break;
                }
            }
            if content_length > 0 {
                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).is_ok() {
                    request.push_str(&String::from_utf8_lossy(&body));
                }
            }
            if let Ok(mut c) = captured.lock() {
                c.push(request);
            }
            let _ = sock.write_all(&responder(i));
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
    });
    Ok((port, handle))
}

/// Тело формы уезжает на сервер: request-line несёт `POST`, заголовок —
/// объявленный `Content-Type`, а за пустой строкой лежат ровно те байты,
/// которые закодировала оболочка. Именно этого не делал
/// `form_submit.rs`, печатавший тело в stderr вместо отправки.
#[test]
fn fetch_page_sends_post_body_and_content_type() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (port, server) = capturing_server(1, Arc::clone(&captured), |_| {
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
    })
    .expect("mock server");
    let client = HttpClient::new();
    let url = Url::parse(&format!("http://127.0.0.1:{port}/login")).expect("url");
    let body = NavigationBody::post(
        "application/x-www-form-urlencoded",
        b"user=admin&pass=secret".to_vec(),
    );
    let page = client.fetch_page(&url, Some(&body)).expect("post navigation");
    assert_eq!(page.body, b"ok");

    let seen = captured.lock().expect("captured");
    let req = seen.first().expect("one request");
    assert!(req.starts_with("POST /login HTTP/1.1"), "request line: {req}");
    assert!(
        req.to_ascii_lowercase().contains("content-type: application/x-www-form-urlencoded"),
        "no content-type: {req}"
    );
    assert!(req.ends_with("user=admin&pass=secret"), "body missing: {req}");
    server.join().expect("server thread");
}

/// Fetch §4.4 «HTTP-redirect fetch», шаг 11: 302 на POST превращает запрос в
/// GET без тела. Логика живёт в `fetch_with_redirect` и раньше была
/// недостижима для навигации — форма никуда не отправлялась вовсе; это
/// проверка того, что навигационный путь действительно в неё попадает.
///
/// PRG (POST → 302 → GET) — ровно то, как отвечает форма входа Keycloak,
/// ради которой дорожка E2E и заведена.
#[test]
fn post_navigation_becomes_get_after_302() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (port, server) = capturing_server(2, Arc::clone(&captured), |i| match i {
        1 => b"HTTP/1.1 302 Found\r\nLocation: /home\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        _ => b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nhome".to_vec(),
    })
    .expect("mock server");
    let client = HttpClient::new();
    let url = Url::parse(&format!("http://127.0.0.1:{port}/login")).expect("url");
    let body = NavigationBody::post("application/x-www-form-urlencoded", b"user=admin".to_vec());
    let mut streamed = Vec::new();
    let page = client
        .fetch_page_streaming(&url, &mut |c, _u| streamed.extend_from_slice(c), Some(&body))
        .expect("post navigation");
    assert_eq!(page.body, b"home");
    assert_eq!(streamed, b"home");
    // База документа — адрес, ответивший 200 (BUG-757), а не адрес формы.
    assert!(page.final_url.as_str().ends_with("/home"), "final_url: {}", page.final_url);

    let seen = captured.lock().expect("captured");
    assert!(seen[0].starts_with("POST /login HTTP/1.1"), "first hop: {}", seen[0]);
    assert!(seen[1].starts_with("GET /home HTTP/1.1"), "second hop: {}", seen[1]);
    assert!(!seen[1].contains("user=admin"), "тело пережило 302: {}", seen[1]);
    server.join().expect("server thread");
}
