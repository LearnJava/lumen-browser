//! stdio-мост к живому окну (DEVX-18).
//!
//! Стандартные MCP-клиенты говорят по stdio, а живое окно (`lumen --mcp-live-port N`)
//! слушает TCP и требует токен в `initialize` (ADR-024 §Access model). Мост — это
//! построчный прокси stdio ⇄ TCP: единственное, что он меняет, — подставляет токен в
//! `params` запроса `initialize`. Токен берётся из файла (`--attach`) или из stderr
//! запущенного мостом окна (`--launch`) и в stdout не попадает никогда.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Режим моста, разобранный из аргументов командной строки.
#[derive(Debug, PartialEq, Eq)]
pub enum BridgeMode {
    /// Подключиться к уже запущенному окну.
    Attach {
        /// TCP-порт `--mcp-live-port` окна.
        port: u16,
        /// Файл с токеном (`[mcp] token: …` или голый токен).
        token_file: PathBuf,
    },
    /// Запустить `lumen --mcp-live-port` самим и читать токен из его stderr.
    Launch {
        /// Путь к `lumen` (иначе `$LUMEN_EXE`, иначе рядом с `lumen-mcp`).
        lumen: Option<PathBuf>,
        /// Страница для открытия (по умолчанию `about:blank`).
        url: Option<String>,
        /// Дополнительные флаги `lumen` после `--`.
        extra: Vec<String>,
    },
}

const TOKEN_MARKER: &str = "[mcp] token: ";
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// Разобрать аргументы. `Ok(None)` — мостовых флагов нет (обычный режим `lumen-mcp`).
pub fn parse_args(args: &[String]) -> Result<Option<BridgeMode>, String> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if !has("--attach") && !has("--launch") {
        return Ok(None);
    }
    if has("--attach") && has("--launch") {
        return Err("--attach и --launch взаимоисключающие".into());
    }
    let mut port = None;
    let mut token_file = None;
    let mut lumen = None;
    let mut url = None;
    let mut extra = Vec::new();
    let mut launch = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--attach" => {
                i += 1;
                let s = args.get(i).ok_or("--attach требует номер порта")?;
                port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
            }
            "--token-file" => {
                i += 1;
                token_file = Some(PathBuf::from(args.get(i).ok_or("--token-file требует путь")?));
            }
            "--launch" => launch = true,
            "--lumen" => {
                i += 1;
                lumen = Some(PathBuf::from(args.get(i).ok_or("--lumen требует путь")?));
            }
            "--" => {
                extra.extend(args[i + 1..].iter().cloned());
                break;
            }
            a if !a.starts_with("--") => url = Some(a.to_string()),
            a => return Err(format!("неизвестный флаг моста: {a}")),
        }
        i += 1;
    }
    if launch {
        return Ok(Some(BridgeMode::Launch { lumen, url, extra }));
    }
    Ok(Some(BridgeMode::Attach {
        port: port.ok_or("--attach требует номер порта")?,
        token_file: token_file.ok_or("--attach требует --token-file <path>")?,
    }))
}

/// Подставить токен в `params` запроса `initialize`; остальные строки — как есть.
pub fn inject_token(line: &str, token: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
        return line.to_string();
    };
    if v.get("method").and_then(|m| m.as_str()) != Some("initialize") {
        return line.to_string();
    }
    if !v.get("params").is_some_and(|p| p.is_object()) {
        v["params"] = serde_json::json!({});
    }
    v["params"]["token"] = serde_json::Value::String(token.to_string());
    v.to_string()
}

/// Прокачать stdio ⇄ TCP до EOF с любой стороны. Токен подставляется в `initialize`.
pub fn pump<R, W>(client_in: R, client_out: W, stream: TcpStream, token: &str) -> io::Result<()>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
{
    let mut upstream = stream.try_clone()?;
    let down = stream.try_clone()?;
    let (done_tx, done_rx) = mpsc::channel::<()>();
    // Ответы окна → клиенту.
    thread::spawn(move || {
        let mut out = client_out;
        for line in BufReader::new(down).lines() {
            let Ok(line) = line else { break };
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
        let _ = done_tx.send(());
    });
    // Запросы клиента → окну (EOF stdin завершает мост).
    let (line_tx, line_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in client_in.lines() {
            let Ok(line) = line else { break };
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });
    loop {
        match line_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let out = inject_token(&line, token);
                if writeln!(upstream, "{out}").and_then(|()| upstream.flush()).is_err() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if done_rx.try_recv().is_ok() {
                    break; // окно закрыло соединение
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // stdin закрыт: дать дочитать уже отправленные ответы.
                let _ = done_rx.recv_timeout(Duration::from_millis(1500));
                break;
            }
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn read_token_file(path: &Path) -> Result<String, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("не прочитан токен-файл {}: {e}", path.display()))?;
    let t = s.trim();
    let t = t.strip_prefix(TOKEN_MARKER).unwrap_or(t).trim();
    if t.is_empty() {
        return Err(format!("токен-файл {} пуст", path.display()));
    }
    Ok(t.to_string())
}

fn default_lumen_exe() -> PathBuf {
    let name = if cfg!(windows) { "lumen.exe" } else { "lumen" };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

struct Launched {
    child: Child,
    port: u16,
    token: String,
}

impl Drop for Launched {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn launch(lumen: Option<PathBuf>, url: Option<String>, extra: Vec<String>) -> Result<Launched, String> {
    let exe = lumen
        .or_else(|| std::env::var_os("LUMEN_EXE").map(PathBuf::from))
        .unwrap_or_else(default_lumen_exe);
    let port = free_port().map_err(|e| format!("нет свободного порта: {e}"))?;
    let mut child = Command::new(&exe)
        .arg("--maximized")
        .arg("--mcp-live-port")
        .arg(port.to_string())
        .args(&extra)
        .arg(url.unwrap_or_else(|| "about:blank".into()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("не запущен {}: {e}", exe.display()))?;
    let stderr = child.stderr.take().ok_or("нет stderr у lumen")?;
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut tx = Some(tx);
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if let Some(tok) = line.strip_prefix(TOKEN_MARKER) {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(tok.trim().to_string());
                }
                continue; // строку с токеном не пересылаем
            }
            eprintln!("{line}");
        }
    });
    let token = match rx.recv_timeout(START_TIMEOUT) {
        Ok(t) => t,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("lumen не напечатал `[mcp] token:` за 60 с".into());
        }
    };
    Ok(Launched { child, port, token })
}

fn connect_with_retry(port: u16) -> Result<TcpStream, String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => {
                return Err(format!("нет соединения с 127.0.0.1:{port}: {e}"));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Точка входа моста: возвращает код выхода процесса.
pub fn run(mode: BridgeMode) -> i32 {
    let result = match mode {
        BridgeMode::Attach { port, token_file } => read_token_file(&token_file).and_then(|token| {
            let stream = connect_with_retry(port)?;
            pump(BufReader::new(io::stdin()), io::stdout(), stream, &token)
                .map_err(|e| e.to_string())
        }),
        BridgeMode::Launch { lumen, url, extra } => launch(lumen, url, extra).and_then(|l| {
            let stream = connect_with_retry(l.port)?;
            // `l` умирает после pump — окно закрывается вместе с мостом.
            pump(BufReader::new(io::stdin()), io::stdout(), stream, &l.token)
                .map_err(|e| e.to_string())
        }),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("lumen-mcp: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_none_without_bridge_flags() {
        assert_eq!(parse_args(&s(&["--port", "1"])).unwrap(), None);
    }

    #[test]
    fn parse_attach() {
        let m = parse_args(&s(&["--attach", "9", "--token-file", "t.txt"])).unwrap();
        assert_eq!(m, Some(BridgeMode::Attach { port: 9, token_file: "t.txt".into() }));
    }

    #[test]
    fn parse_attach_requires_token_file() {
        assert!(parse_args(&s(&["--attach", "9"])).is_err());
    }

    #[test]
    fn parse_launch_with_url_and_extra() {
        let m = parse_args(&s(&["--launch", "http://x/", "--", "--no-scrollbar"])).unwrap();
        assert_eq!(
            m,
            Some(BridgeMode::Launch {
                lumen: None,
                url: Some("http://x/".into()),
                extra: s(&["--no-scrollbar"])
            })
        );
    }

    #[test]
    fn inject_token_only_into_initialize() {
        let out = inject_token(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"a":1}}"#, "T");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["params"]["token"], "T");
        assert_eq!(v["params"]["a"], 1);
        let other = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        assert_eq!(inject_token(other, "T"), other);
        assert_eq!(inject_token("garbage", "T"), "garbage");
    }

    #[test]
    fn inject_token_creates_missing_params() {
        let out = inject_token(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, "T");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["params"]["token"], "T");
    }

    #[derive(Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Сквозной тест: настоящий `McpServer::with_token` по TCP, клиент шлёт initialize без токена.
    #[test]
    fn pump_authenticates_against_token_gated_server() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let transport = crate::TcpTransport::from_stream(stream).unwrap();
            let session = lumen_driver::InProcessSession::new();
            let mut server = crate::McpServer::with_token(session, transport, "secret".into());
            let _ = server.run();
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let sink = Sink(Arc::default());
        pump(Cursor::new(input), sink.clone(), stream, "secret").unwrap();
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<serde_json::Value> =
            text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0]["error"].is_null(), "{text}");
        assert!(lines[1]["result"]["tools"].is_array(), "{text}");
        assert!(!text.contains("secret"));
        srv.join().unwrap();
    }
}
