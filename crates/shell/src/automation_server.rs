//! Headless automation servers: `--ipc-server` (TAB-5) and `--mcp` / `--mcp-port`.
//!
//! Both block the process on a socket instead of running the winit loop, and
//! both drive the engine through a session object rather than through a window,
//! so neither belongs with the one-shot render modes in [`crate::dump_mode`].
//! Moved out of `main.rs` by the SPLIT track (batch SH-3a); behaviour and
//! signatures are unchanged.

use crate::*;

/// Р—Р°РїСѓСЃС‚РёС‚СЊ headless IPC-СЃРµСЂРІРµСЂ С‚Р°Р±-РєРѕРјР°РЅРґ (TAB-5).
///
/// РЎР»СѓС€Р°РµС‚ TCP loopback (`lumen_ipc`), РїРµС‡Р°С‚Р°РµС‚ РІС‹Р±СЂР°РЅРЅС‹Р№ РїРѕСЂС‚ РІ stdout СЃС‚СЂРѕРєРѕР№
/// `LUMEN_IPC_PORT=<port>` (РєРѕРЅС‚СЂРѕР»Р»РµСЂ РµС‘ РїР°СЂСЃРёС‚), Р·Р°С‚РµРј РѕР±СЃР»СѓР¶РёРІР°РµС‚ РєРѕРјР°РЅРґС‹:
/// `CreateTab` / `NavigateTab` / `Screenshot` / `CloseTab` / `Shutdown`.
///
/// В«Р’РєР»Р°РґРєР°В» Р·РґРµСЃСЊ вЂ” headless-РєРѕРЅС‚РµРєСЃС‚ СЂРµРЅРґРµСЂР°: С…СЂР°РЅРёС‚ Р»РёС€СЊ СЃРІРѕР№ [`PageSource`],
/// Р° С„Р°РєС‚РёС‡РµСЃРєРёР№ load в†’ layout в†’ CPU-СЂР°СЃС‚РµСЂРёР·Р°С†РёСЏ РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ Р»РµРЅРёРІРѕ РЅР°
/// `Screenshot` С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ РїСѓС‚СЊ, С‡С‚Рѕ Рё `--screenshot` ([`render_source_to_png`]).
/// РћРєРЅРѕ/wgpu/winit РЅРµ СЃРѕР·РґР°СЋС‚СЃСЏ вЂ” СЃРЅРёРјРѕРє РїРёРєСЃРµР»СЊРЅРѕ РІРѕСЃРїСЂРѕРёР·РІРѕРґРёРј Рё СЂР°Р±РѕС‚Р°РµС‚ РІ CI.
/// РЎРѕСЃС‚РѕСЏРЅРёРµ РІРєР»Р°РґРѕРє РїРµСЂРµР¶РёРІР°РµС‚ РїРµСЂРµРїРѕРґРєР»СЋС‡РµРЅРёСЏ РєР»РёРµРЅС‚Р°; СЃРµСЂРІРµСЂ РІС‹С…РѕРґРёС‚ РїРѕ
/// `Shutdown`.
pub(crate) fn run_ipc_server(port: Option<u16>, event_sink: Arc<dyn EventSink>) -> ExitCode {
    use lumen_ipc::{IpcRequest, IpcResponse, IpcServer, TabId};
    use std::collections::HashMap;
    use std::io::Write as _;

    // `IpcServer::bind` РІСЃРµРіРґР° СЃР»СѓС€Р°РµС‚ 127.0.0.1:0 (OS РЅР°Р·РЅР°С‡Р°РµС‚ РїРѕСЂС‚). РЇРІРЅС‹Р№
    // `--ipc-port` РїРѕРєР° РЅРѕСЃРёС‚ РёРЅС„РѕСЂРјР°С†РёРѕРЅРЅС‹Р№ С…Р°СЂР°РєС‚РµСЂ вЂ” РєРѕРЅС‚СЂРѕР»Р»РµСЂ РІСЃС‘ СЂР°РІРЅРѕ
    // С‡РёС‚Р°РµС‚ С„Р°РєС‚РёС‡РµСЃРєРёР№ РїРѕСЂС‚ РёР· stdout-СЃС‚СЂРѕРєРё `LUMEN_IPC_PORT=`.
    let (server, bound_port) = match IpcServer::bind() {
        Ok(sp) => sp,
        Err(e) => {
            eprintln!("lumen --ipc-server: РЅРµ СѓРґР°Р»РѕСЃСЊ РѕС‚РєСЂС‹С‚СЊ РїРѕСЂС‚: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(requested) = port
        && requested != bound_port
    {
        eprintln!(
            "lumen --ipc-server: СЏРІРЅС‹Р№ РїРѕСЂС‚ {requested} РёРіРЅРѕСЂРёСЂСѓРµС‚СЃСЏ (bind РЅР°Р·РЅР°С‡Р°РµС‚ \
             РїРѕСЂС‚ Р°РІС‚РѕРјР°С‚РёС‡РµСЃРєРё); РёСЃРїРѕР»СЊР·СѓСЋ {bound_port}"
        );
    }

    // РљРѕРЅС‚СЂРѕР»Р»РµСЂ РїР°СЂСЃРёС‚ СЌС‚Рё СЃС‚СЂРѕРєРё РёР· stdout, С‡С‚РѕР±С‹ СѓР·РЅР°С‚СЊ РїРѕСЂС‚ РїРѕРґРєР»СЋС‡РµРЅРёСЏ Рё
    // С‚РѕРєРµРЅ Р°СѓС‚РµРЅС‚РёС„РёРєР°С†РёРё (ADR-024 В§Access model, DEVX-15).
    let token = lumen_core::auth::generate_token();
    println!("LUMEN_IPC_PORT={bound_port}");
    println!("LUMEN_IPC_TOKEN={token}");
    let _ = std::io::stdout().flush();
    eprintln!("lumen: IPC-СЃРµСЂРІРµСЂ С‚Р°Р±-РєРѕРјР°РЅРґ Р·Р°РїСѓС‰РµРЅ РЅР° 127.0.0.1:{bound_port} (TAB-5)");

    let mut tabs: HashMap<TabId, PageSource> = HashMap::new();
    let mut next_id: TabId = 1;

    // Р’РЅРµС€РЅРёР№ С†РёРєР»: РїСЂРёРЅРёРјР°РµРј РїРѕРґРєР»СЋС‡РµРЅРёСЏ (РєРѕРЅС‚СЂРѕР»Р»РµСЂ РјРѕР¶РµС‚ РїРµСЂРµРїРѕРґРєР»СЋС‡Р°С‚СЊСЃСЏ).
    loop {
        let mut channel = match server.accept() {
            Ok(ch) => ch,
            Err(e) => {
                eprintln!("lumen --ipc-server: РѕС€РёР±РєР° accept: {e}");
                return ExitCode::FAILURE;
            }
        };

        // РљР°Р¶РґРѕРµ РЅРѕРІРѕРµ СЃРѕРµРґРёРЅРµРЅРёРµ РЅР°С‡РёРЅР°РµС‚ РЅРµР°СѓС‚РµРЅС‚РёС„РёС†РёСЂРѕРІР°РЅРЅС‹Рј вЂ” `Auth` СЃ
        // РІРµСЂРЅС‹Рј С‚РѕРєРµРЅРѕРј РґРѕР»Р¶РµРЅ Р±С‹С‚СЊ РїРµСЂРІС‹Рј СЃРѕРѕР±С‰РµРЅРёРµРј.
        let mut authenticated = false;

        // Р’РЅСѓС‚СЂРµРЅРЅРёР№ С†РёРєР»: РѕР±СЃР»СѓР¶РёРІР°РµРј Р·Р°РїСЂРѕСЃС‹ С‚РµРєСѓС‰РµРіРѕ РїРѕРґРєР»СЋС‡РµРЅРёСЏ РґРѕ СЂР°Р·СЂС‹РІР°
        // (`recv` РІРµСЂРЅС‘С‚ Err РїСЂРё РѕС‚РєР»СЋС‡РµРЅРёРё РєР»РёРµРЅС‚Р° вЂ” Р¶РґС‘Рј СЃР»РµРґСѓСЋС‰РµРµ РїРѕРґРєР»СЋС‡РµРЅРёРµ).
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
                                message: "РЅРµРІРµСЂРЅС‹Р№ С‚РѕРєРµРЅ".to_owned(),
                            });
                            break;
                        }
                    }
                    _ => {
                        let _ = channel.send(&IpcResponse::AuthErr {
                            message: "РЅСѓР¶РЅР° Р°СѓС‚РµРЅС‚РёС„РёРєР°С†РёСЏ: СЃРЅР°С‡Р°Р»Р° РѕС‚РїСЂР°РІСЊС‚Рµ Auth".to_owned(),
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
                    eprintln!("lumen --ipc-server: РїРѕР»СѓС‡РµРЅ Shutdown, РІС‹С…РѕРґРёРј");
                    return ExitCode::SUCCESS;
                }
                IpcRequest::Fetch(fr) => IpcResponse::FetchErr(lumen_ipc::FetchErr {
                    id: fr.id,
                    error: "ipc-server: Fetch РЅРµ РїРѕРґРґРµСЂР¶РёРІР°РµС‚СЃСЏ РІ СЂРµР¶РёРјРµ С‚Р°Р±-РєРѕРјР°РЅРґ".to_owned(),
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
                            message: format!("РЅРµС‚ РІРєР»Р°РґРєРё СЃ id {tab_id}"),
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
                            message: format!("РЅРµС‚ РІРєР»Р°РґРєРё СЃ id {tab_id}"),
                        }
                    }
                }
                IpcRequest::Screenshot { tab_id } => match tabs.get(&tab_id) {
                    Some(source) => match render_source_to_png(source, event_sink.clone(), None) {
                        Ok((png, _w, _h)) => IpcResponse::Screenshot { tab_id, png },
                        Err(e) => IpcResponse::TabError {
                            tab_id,
                            message: format!("РѕС€РёР±РєР° СЂРµРЅРґРµСЂР°: {e}"),
                        },
                    },
                    None => IpcResponse::TabError {
                        tab_id,
                        message: format!("РЅРµС‚ РІРєР»Р°РґРєРё СЃ id {tab_id}"),
                    },
                },
            };

            if channel.send(&resp).is_err() {
                // РќРµ СЃРјРѕРіР»Рё РѕС‚РІРµС‚РёС‚СЊ вЂ” СЃРѕРµРґРёРЅРµРЅРёРµ РјРµСЂС‚РІРѕ, Р¶РґС‘Рј РїРµСЂРµРїРѕРґРєР»СЋС‡РµРЅРёСЏ.
                break;
            }
        }
    }
}

/// Р—Р°РїСѓСЃС‚РёС‚СЊ MCP-СЃРµСЂРІРµСЂ РІ headless-СЂРµР¶РёРјРµ.
///
/// РЎРѕР·РґР°С‘С‚ `InProcessSession`, РѕРїС†РёРѕРЅР°Р»СЊРЅРѕ Р·Р°РіСЂСѓР¶Р°РµС‚ URL, Р·Р°С‚РµРј Р·Р°РїСѓСЃРєР°РµС‚
/// `McpServer` РїРѕРІРµСЂС… stdio РёР»Рё TCP-С‚СЂР°РЅСЃРїРѕСЂС‚Р°. Р‘Р»РѕРєРёСЂСѓРµС‚ РґРѕ РѕС‚РєР»СЋС‡РµРЅРёСЏ РєР»РёРµРЅС‚Р°.
pub(crate) fn run_mcp_mode(mcp: McpMode) -> ExitCode {
    use lumen_driver::{BrowserSession, InProcessSession};
    use lumen_mcp::{McpServer, StdioTransport, TcpTransport};
    use std::net::TcpListener;

    let mut session = InProcessSession::new();
    if let Some(ref url) = mcp.url
        && let Err(e) = session.navigate(url)
    {
        eprintln!("MCP: РѕС€РёР±РєР° Р·Р°РіСЂСѓР·РєРё {url}: {e}");
    }

    if let Some(port) = mcp.port {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("MCP: РЅРµ СѓРґР°Р»РѕСЃСЊ РѕС‚РєСЂС‹С‚СЊ РїРѕСЂС‚ {port}: {e}");
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
                        eprintln!("MCP: РѕС€РёР±РєР° С‚СЂР°РЅСЃРїРѕСЂС‚Р°: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            Err(e) => {
                eprintln!("MCP: РѕС€РёР±РєР° accept: {e}");
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
