//! Headless automation servers: `--ipc-server` (TAB-5) and `--mcp` / `--mcp-port`.
//!
//! Both block the process on a socket instead of running the winit loop, and
//! both drive the engine through a session object rather than through a window,
//! so neither belongs with the one-shot render modes in [`crate::dump_mode`].
//! Moved out of `main.rs` by the SPLIT track (batch SH-3a); behaviour and
//! signatures are unchanged.

use crate::*;

/// Запустить headless IPC-сервер таб-команд (TAB-5).
///
/// Слушает TCP loopback (`lumen_ipc`), печатает выбранный порт в stdout строкой
/// `LUMEN_IPC_PORT=<port>` (контроллер её парсит), затем обслуживает команды:
/// `CreateTab` / `NavigateTab` / `Screenshot` / `CloseTab` / `Shutdown`.
///
/// «Вкладка» здесь — headless-контекст рендера: хранит лишь свой [`PageSource`],
/// а фактический load → layout → CPU-растеризация выполняется лениво на
/// `Screenshot` через тот же путь, что и `--screenshot` ([`render_source_to_png`]).
/// Окно/wgpu/winit не создаются — снимок пиксельно воспроизводим и работает в CI.
/// Состояние вкладок переживает переподключения клиента; сервер выходит по
/// `Shutdown`.
pub(crate) fn run_ipc_server(port: Option<u16>, event_sink: Arc<dyn EventSink>) -> ExitCode {
    use lumen_ipc::{IpcRequest, IpcResponse, IpcServer, TabId};
    use std::collections::HashMap;
    use std::io::Write as _;

    // `IpcServer::bind` всегда слушает 127.0.0.1:0 (OS назначает порт). Явный
    // `--ipc-port` пока носит информационный характер — контроллер всё равно
    // читает фактический порт из stdout-строки `LUMEN_IPC_PORT=`.
    let (server, bound_port) = match IpcServer::bind() {
        Ok(sp) => sp,
        Err(e) => {
            eprintln!("lumen --ipc-server: не удалось открыть порт: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(requested) = port
        && requested != bound_port
    {
        eprintln!(
            "lumen --ipc-server: явный порт {requested} игнорируется (bind назначает \
             порт автоматически); использую {bound_port}"
        );
    }

    // Контроллер парсит эти строки из stdout, чтобы узнать порт подключения и
    // токен аутентификации (ADR-024 §Access model, DEVX-15).
    let token = lumen_core::auth::generate_token();
    println!("LUMEN_IPC_PORT={bound_port}");
    println!("LUMEN_IPC_TOKEN={token}");
    let _ = std::io::stdout().flush();
    eprintln!("lumen: IPC-сервер таб-команд запущен на 127.0.0.1:{bound_port} (TAB-5)");

    let mut tabs: HashMap<TabId, PageSource> = HashMap::new();
    let mut next_id: TabId = 1;

    // Внешний цикл: принимаем подключения (контроллер может переподключаться).
    loop {
        let mut channel = match server.accept() {
            Ok(ch) => ch,
            Err(e) => {
                eprintln!("lumen --ipc-server: ошибка accept: {e}");
                return ExitCode::FAILURE;
            }
        };

        // Каждое новое соединение начинает неаутентифицированным — `Auth` с
        // верным токеном должен быть первым сообщением.
        let mut authenticated = false;

        // Внутренний цикл: обслуживаем запросы текущего подключения до разрыва
        // (`recv` вернёт Err при отключении клиента — ждём следующее подключение).
        while let Ok(req) = channel.recv::<IpcRequest>() {
            if !authenticated {
                match req {
                    IpcRequest::Auth { token: provided } => {
                        if lumen_core::auth::tokens_match(&token, &provided) {
                            authenticated = true;
                            if channel.send(&IpcResponse::AuthOk).is_err() {
                                break;
                            }
                        } else {
                            let _ = channel.send(&IpcResponse::AuthErr {
                                message: "неверный токен".to_owned(),
                            });
                            break;
                        }
                    }
                    _ => {
                        let _ = channel.send(&IpcResponse::AuthErr {
                            message: "нужна аутентификация: сначала отправьте Auth".to_owned(),
                        });
                        break;
                    }
                }
                continue;
            }

            let resp = match req {
                IpcRequest::Auth { .. } => IpcResponse::AuthOk,
                IpcRequest::Ping => IpcResponse::Pong,
                IpcRequest::Shutdown => {
                    let _ = channel.send(&IpcResponse::Shutdown);
                    eprintln!("lumen --ipc-server: получен Shutdown, выходим");
                    return ExitCode::SUCCESS;
                }
                IpcRequest::Fetch(fr) => IpcResponse::FetchErr(lumen_ipc::FetchErr {
                    id: fr.id,
                    error: "ipc-server: Fetch не поддерживается в режиме таб-команд".to_owned(),
                }),
                IpcRequest::CreateTab => {
                    let tab_id = next_id;
                    next_id = next_id.wrapping_add(1);
                    tabs.insert(tab_id, PageSource::Empty);
                    IpcResponse::TabCreated { tab_id }
                }
                IpcRequest::CloseTab { tab_id } => {
                    if tabs.remove(&tab_id).is_some() {
                        IpcResponse::TabClosed { tab_id }
                    } else {
                        IpcResponse::TabError {
                            tab_id,
                            message: format!("нет вкладки с id {tab_id}"),
                        }
                    }
                }
                IpcRequest::NavigateTab { tab_id, url } => {
                    if let Some(slot) = tabs.get_mut(&tab_id) {
                        *slot = PageSource::from_arg(Some(&url));
                        IpcResponse::Navigated { tab_id }
                    } else {
                        IpcResponse::TabError {
                            tab_id,
                            message: format!("нет вкладки с id {tab_id}"),
                        }
                    }
                }
                IpcRequest::Screenshot { tab_id } => match tabs.get(&tab_id) {
                    Some(source) => match render_source_to_png(source, event_sink.clone(), None) {
                        Ok((png, _w, _h)) => IpcResponse::Screenshot { tab_id, png },
                        Err(e) => IpcResponse::TabError {
                            tab_id,
                            message: format!("ошибка рендера: {e}"),
                        },
                    },
                    None => IpcResponse::TabError {
                        tab_id,
                        message: format!("нет вкладки с id {tab_id}"),
                    },
                },
            };

            if channel.send(&resp).is_err() {
                // Не смогли ответить — соединение мертво, ждём переподключения.
                break;
            }
        }
    }
}

/// Запустить MCP-сервер в headless-режиме.
///
/// Создаёт `InProcessSession`, опционально загружает URL, затем запускает
/// `McpServer` поверх stdio или TCP-транспорта. Блокирует до отключения клиента.
pub(crate) fn run_mcp_mode(mcp: McpMode) -> ExitCode {
    use lumen_driver::{BrowserSession, InProcessSession};
    use lumen_mcp::{McpServer, StdioTransport, TcpTransport};
    use std::net::TcpListener;

    let mut session = InProcessSession::new();
    if let Some(ref url) = mcp.url
        && let Err(e) = session.navigate(url)
    {
        eprintln!("MCP: ошибка загрузки {url}: {e}");
    }

    if let Some(port) = mcp.port {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("MCP: не удалось открыть порт {port}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let token = lumen_core::auth::generate_token();
        eprintln!("MCP listening on 127.0.0.1:{port}");
        eprintln!("MCP token: {token}");
        match listener.accept() {
            Ok((stream, addr)) => {
                eprintln!("MCP connection from {addr}");
                match TcpTransport::from_stream(stream) {
                    Ok(transport) => {
                        let mut server = McpServer::with_token(session, transport, token);
                        let _ = server.run();
                    }
                    Err(e) => {
                        eprintln!("MCP: ошибка транспорта: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Err(e) => {
                eprintln!("MCP: ошибка accept: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        eprintln!("MCP server ready (stdio)");
        let transport = StdioTransport::new();
        let mut server = McpServer::new(session, transport);
        let _ = server.run();
    }

    ExitCode::SUCCESS
}
