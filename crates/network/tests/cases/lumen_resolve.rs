//! WPT-LUMEN-RESOLVE: `LUMEN_RESOLVE=host=ip` routes a real `HttpClient`
//! navigation to an otherwise unresolvable name (`x.invalid`, RFC 6761 —
//! never resolves through DNS) to a local server.
//!
//! The variable is read once per process (`OnceLock`), and the other tests of
//! this binary may initialise it first, so setting it in-process would race.
//! The parent test instead re-runs this very test binary with the variable set
//! and only the `#[ignore]`d child test selected — a fresh process whose first
//! connection reads the override.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

use lumen_core::url::Url;
use lumen_network::{HttpClient, RESOLVE_ENV};

/// Port of the parent's server, passed to the child; its absence means the
/// child was started by hand (`cargo test -- --ignored`) and has nothing to do.
const PORT_ENV: &str = "LUMEN_RESOLVE_TEST_PORT";

/// One-shot server: answers 200 and hands back the request head.
fn one_shot_server() -> std::io::Result<(u16, thread::JoinHandle<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = thread::spawn(move || {
        let Ok((mut sock, _)) = listener.accept() else { return String::new() };
        let Ok(clone) = sock.try_clone() else { return String::new() };
        let mut reader = BufReader::new(clone);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nresolved");
        head
    });
    Ok((port, handle))
}

#[test]
fn env_override_reaches_local_server_by_name() {
    let (port, server) = one_shot_server().expect("server");
    let status = Command::new(std::env::current_exe().expect("test binary"))
        .args(["cases::lumen_resolve::child_fetch_overridden_host", "--exact", "--ignored", "--nocapture"])
        .env(RESOLVE_ENV, "bad-pair,X.Invalid=127.0.0.1")
        .env(PORT_ENV, port.to_string())
        .status()
        .expect("spawn child test");
    assert!(status.success(), "child test failed: {status}");
    let head = server.join().expect("server");
    // The name, not the IP, reaches the wire. The port is not checked:
    // BUG-1762 — `Host` omits a non-default port.
    assert!(head.to_ascii_lowercase().contains("\nhost: x.invalid"), "head: {head}");
}

/// Runs only inside the process spawned by
/// [`env_override_reaches_local_server_by_name`].
#[test]
#[ignore = "child process of env_override_reaches_local_server_by_name"]
fn child_fetch_overridden_host() {
    let Ok(port) = std::env::var(PORT_ENV) else { return };
    let url = Url::parse(&format!("http://x.invalid:{port}/")).expect("url");
    let page = HttpClient::new().fetch_page(&url, None, false).expect("fetch via LUMEN_RESOLVE");
    assert_eq!(page.body, b"resolved");
}
