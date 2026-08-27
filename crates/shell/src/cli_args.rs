//! Command-line argument extraction for `run_cli`, plus `--help` output.
//!
//! Every `extract_*` here has the same contract: scan `args` for one flag, return
//! its parsed value together with the remaining arguments, so `run_cli` can peel
//! the flags off one at a time and treat whatever is left as the page source.
//!
//! The launch mode the peeled arguments add up to lives here too ([`CliMode`],
//! [`McpMode`], [`DumpKind`] and [`parse_cli`], batch SH-3d): `parse_cli` reads
//! the arguments `run_cli` has left over, so its grammar and the `extract_*`
//! contract above are two halves of one answer.
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-3a, SH-3d); behaviour
//! and signatures are unchanged.

use crate::*;

pub(crate) fn print_usage() {
    eprintln!("РСЃРїРѕР»СЊР·РѕРІР°РЅРёРµ:");
    eprintln!("  lumen                                           вЂ” РїСѓСЃС‚РѕРµ РѕРєРЅРѕ");
    eprintln!("  lumen <path-or-url>                             вЂ” РѕС‚РєСЂС‹С‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РІ РѕРєРЅРµ");
    eprintln!("  lumen --dump-source <path-or-url>               вЂ” РґРµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Р№ HTML РІ stdout");
    eprintln!("  lumen --dump-layout <path-or-url>               вЂ” layout-РґРµСЂРµРІРѕ РІ stdout");
    eprintln!("  lumen --dump-display-list <path-or-url>         вЂ” display list РІ stdout");
    eprintln!("  lumen --print-to-pdf <out.pdf> <path-or-url>   вЂ” СЃРѕС…СЂР°РЅРёС‚СЊ СЃС‚СЂР°РЅРёС†Сѓ РєР°Рє PDF");
    eprintln!("  lumen --screenshot <out.png> <path-or-url>     вЂ” CPU-СЃРЅРёРјРѕРє СЃС‚СЂР°РЅРёС†С‹ РІ PNG (Р±РµР· РѕРєРЅР°)");
    eprintln!("  lumen --trace-nav <out.json> <path-or-url>     вЂ” С‚Р°Р№РјР»Р°Р№РЅ РѕРґРЅРѕР№ РЅР°РІРёРіР°С†РёРё РІ Chrome-trace JSON");
    eprintln!("  [--devtools-port <N>]                           вЂ” DevTools WS СЃРµСЂРІРµСЂ (Р»СЋР±РѕР№ СЂРµР¶РёРј)");
    eprintln!("  [--bidi-port <N>]                               вЂ” WebDriver BiDi WS СЃРµСЂРІРµСЂ (Р»СЋР±РѕР№ СЂРµР¶РёРј)");
    eprintln!("  [--mcp-live-port <N>]                           вЂ” MCP-СЃРµСЂРІРµСЂ (TCP) РЅР° Р¶РёРІРѕРј РѕРєРЅРµ (Р»СЋР±РѕР№ СЂРµР¶РёРј, SDC-2)");
    eprintln!("  [--viewport <W>x<H>]                            вЂ” С„РёРєСЃ. CSS-СЂР°Р·РјРµСЂ РѕРєРЅР° (РїРµСЂРµРѕРїСЂРµРґРµР»СЏРµС‚ --deterministic 1280Г—800)");
    eprintln!("  [--maximized]                                   вЂ” СЂР°Р·РІРµСЂРЅСѓС‚СЊ РѕРєРЅРѕ РЅР° РІРµСЃСЊ СЌРєСЂР°РЅ (Р¶РёРІРѕР№ РїРµСЂС„-Р°СѓРґРёС‚)");
    eprintln!("  [--proxy <url>]                                 вЂ” HTTP РїСЂРѕРєСЃРё (http://host:port РёР»Рё user:pass@host:port)");
    eprintln!("  [--tor [--tor-port <N>]]                        вЂ” Tor-СЂРµР¶РёРј: TorBrowser fingerprint + SOCKS5 9050 (РёР»Рё N)");
    eprintln!("  --import-session <file.lsession>                вЂ” РІРѕСЃСЃС‚Р°РЅРѕРІРёС‚СЊ СЃРµСЃСЃРёСЋ РёР· С„Р°Р№Р»Р°");
    eprintln!("  --mcp [url]                                     вЂ” MCP-СЃРµСЂРІРµСЂ (stdio) РґР»СЏ AI-Р°РіРµРЅС‚РѕРІ");
    eprintln!("  --mcp-port <N> [url]                            вЂ” MCP-СЃРµСЂРІРµСЂ (TCP) РЅР° РїРѕСЂС‚Сѓ N");
    eprintln!("  [--network-service]                             вЂ” РІС‹РЅРµСЃС‚Рё HTTP/TLS/DNS РІ РѕС‚РґРµР»СЊРЅС‹Р№ РїСЂРѕС†РµСЃСЃ (PH1-4)");
    eprintln!("  --ipc-server                                    вЂ” headless IPC-СЃРµСЂРІРµСЂ С‚Р°Р±-РєРѕРјР°РЅРґ: PNG-СЃРЅРёРјРєРё С‡РµСЂРµР· TCP (TAB-5)");
}

/// РР·РІР»РµС‡СЊ `--print-to-pdf <output.pdf>` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(output_path), РѕСЃС‚Р°Р»СЊРЅС‹Рµ_Р°СЂРіСѓРјРµРЅС‚С‹)` РёР»Рё `(None, РІСЃРµ_Р°СЂРіСѓРјРµРЅС‚С‹)`.
pub(crate) fn extract_print_to_pdf(args: &[String]) -> (Option<std::path::PathBuf>, Vec<String>) {
    let mut i = 0;
    let mut output: Option<std::path::PathBuf> = None;
    let mut rest = Vec::new();

    while i < args.len() {
        if args[i] == "--print-to-pdf" && output.is_none() {
            i += 1;
            if let Some(path) = args.get(i) {
                output = Some(std::path::PathBuf::from(path));
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if output.is_some() {
        (output, rest)
    } else {
        (None, args.to_vec())
    }
}

/// РР·РІР»РµС‡СЊ `--screenshot <output.png>` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(output_path), РѕСЃС‚Р°Р»СЊРЅС‹Рµ_Р°СЂРіСѓРјРµРЅС‚С‹)` РёР»Рё `(None, РІСЃРµ_Р°СЂРіСѓРјРµРЅС‚С‹)`.
/// РџРѕСЂСЏРґРѕРє Р°СЂРіСѓРјРµРЅС‚РѕРІ Р·РµСЂРєР°Р»РёС‚ `--print-to-pdf`: РїСѓС‚СЊ РІС‹РІРѕРґР° РёРґС‘С‚ СЃСЂР°Р·Сѓ Р·Р° С„Р»Р°РіРѕРј,
/// РёСЃС‚РѕС‡РЅРёРє СЃС‚СЂР°РЅРёС†С‹ вЂ” РїРѕР·РёС†РёРѕРЅРЅС‹Р№ РѕСЃС‚Р°С‚РѕРє (`--screenshot out.png <url>`).
pub(crate) fn extract_screenshot(args: &[String]) -> (Option<std::path::PathBuf>, Vec<String>) {
    let mut i = 0;
    let mut output: Option<std::path::PathBuf> = None;
    let mut rest = Vec::new();

    while i < args.len() {
        if args[i] == "--screenshot" && output.is_none() {
            i += 1;
            if let Some(path) = args.get(i) {
                output = Some(std::path::PathBuf::from(path));
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if output.is_some() {
        (output, rest)
    } else {
        (None, args.to_vec())
    }
}

/// РР·РІР»РµС‡СЊ `--trace-nav <output.json>` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ (PERF-1).
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(output_path), РѕСЃС‚Р°Р»СЊРЅС‹Рµ_Р°СЂРіСѓРјРµРЅС‚С‹)` РёР»Рё `(None, РІСЃРµ_Р°СЂРіСѓРјРµРЅС‚С‹)`.
/// РџРѕСЂСЏРґРѕРє Р°СЂРіСѓРјРµРЅС‚РѕРІ Р·РµСЂРєР°Р»РёС‚ `--screenshot`: РїСѓС‚СЊ РІС‹РІРѕРґР° РёРґС‘С‚ СЃСЂР°Р·Сѓ Р·Р° С„Р»Р°РіРѕРј,
/// РёСЃС‚РѕС‡РЅРёРє СЃС‚СЂР°РЅРёС†С‹ вЂ” РїРѕР·РёС†РёРѕРЅРЅС‹Р№ РѕСЃС‚Р°С‚РѕРє (`--trace-nav out.json <url>`).
pub(crate) fn extract_trace_nav(args: &[String]) -> (Option<std::path::PathBuf>, Vec<String>) {
    let mut i = 0;
    let mut output: Option<std::path::PathBuf> = None;
    let mut rest = Vec::new();

    while i < args.len() {
        if args[i] == "--trace-nav" && output.is_none() {
            i += 1;
            if let Some(path) = args.get(i) {
                output = Some(std::path::PathBuf::from(path));
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if output.is_some() {
        (output, rest)
    } else {
        (None, args.to_vec())
    }
}

/// РР·РІР»РµС‡СЊ `--mcp` / `--mcp-port N` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(McpMode), РѕСЃС‚Р°Р»СЊРЅС‹Рµ_Р°СЂРіСѓРјРµРЅС‚С‹)` РёР»Рё `(None, РІСЃРµ_Р°СЂРіСѓРјРµРЅС‚С‹)`.
pub(crate) fn extract_mcp_mode(args: &[String]) -> (Option<McpMode>, Vec<String>) {
    let mut port: Option<u16> = None;
    let mut url: Option<String> = None;
    let mut mcp_found = false;
    let mut rest = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "--mcp" {
            mcp_found = true;
        } else if args[i] == "--mcp-port" {
            mcp_found = true;
            i += 1;
            if let Some(p) = args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                port = Some(p);
            }
        } else if mcp_found && !args[i].starts_with("--") && url.is_none() {
            url = Some(args[i].clone());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }

    if mcp_found {
        (Some(McpMode { url, port }), rest)
    } else {
        (None, args.to_vec())
    }
}

/// Р РµР·СѓР»СЊС‚Р°С‚ СЂР°Р·Р±РѕСЂР° `--import-session`: (source, (scroll_x, scroll_y)).
pub(crate) type ImportedSession = (PageSource, (f32, f32));

/// РР·РІР»РµС‡СЊ `--import-session <file>` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ (Some((source, (scroll_x, scroll_y))), РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹)
/// РёР»Рё (None, Р°СЂРіСѓРјРµРЅС‚С‹) РµСЃР»Рё С„Р»Р°Рі РЅРµ СѓРєР°Р·Р°РЅ.
pub(crate) fn extract_import_session(
    args: &[String],
) -> Result<(Option<ImportedSession>, Vec<String>), String> {
    let mut session: Option<(PageSource, (f32, f32))> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--import-session" {
            i += 1;
            let path = args.get(i).ok_or("--import-session С‚СЂРµР±СѓРµС‚ РїСѓС‚СЊ Рє С„Р°Р№Р»Сѓ")?;
            let json = std::fs::read_to_string(path)
                .map_err(|e| format!("РЅРµ СѓРґР°Р»РѕСЃСЊ РїСЂРѕС‡РёС‚Р°С‚СЊ {path}: {e}"))?;
            let file = session_export::from_json(&json)
                .map_err(|e| format!("РѕС€РёР±РєР° СЂР°Р·Р±РѕСЂР° СЃРµСЃСЃРёРё {path}: {e}"))?;
            let tab = session_export::active_tab(&file)
                .ok_or_else(|| format!("СЃРµСЃСЃРёСЏ {path} РЅРµ СЃРѕРґРµСЂР¶РёС‚ РІРєР»Р°РґРѕРє"))?;
            let source = PageSource::from_arg(Some(&tab.url));
            session = Some((source, (tab.scroll_x, tab.scroll_y)));
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((session, rest))
}

/// РР·РІР»РµС‡СЊ `--viewport <W>x<H>` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ (DEVX-1).
///
/// Overrides the window's CSS content viewport size (window height still adds
/// `toolbar::CHROME_H` вЂ” tab bar + toolbar вЂ” on top, same as the
/// non-deterministic default вЂ” see `resumed()`). Needed because
/// `--deterministic` forces a 1280Г—800 window,
/// which breaks `graphic_tests/run.py --live`'s magenta-marker crop calibration
/// (baked in at the pipeline's fixed 1024Г—720 viewport); this flag lets a caller
/// combine `--deterministic` (freeze Date.now/Math.random/rAF) with the exact
/// viewport graphic_tests expects. Malformed values (missing `x`, non-numeric)
/// are left in `rest` untouched rather than silently ignored.
pub(crate) fn extract_viewport_override(args: &[String]) -> (Option<(f32, f32)>, Vec<String>) {
    let mut size: Option<(f32, f32)> = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--viewport" && size.is_none() {
            match iter.next() {
                Some(val) => match val.split_once('x') {
                    Some((w, h)) => match (w.parse::<f32>(), h.parse::<f32>()) {
                        (Ok(w), Ok(h)) => size = Some((w, h)),
                        _ => {
                            rest.push(arg.clone());
                            rest.push(val.clone());
                        }
                    },
                    None => {
                        rest.push(arg.clone());
                        rest.push(val.clone());
                    }
                },
                None => rest.push(arg.clone()),
            }
        } else {
            rest.push(arg.clone());
        }
    }
    (size, rest)
}

/// РР·РІР»РµС‡СЊ `--maximized` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ, РІРµСЂРЅСѓС‚СЊ (flag, РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹).
///
/// Р Р°Р·РІРѕСЂР°С‡РёРІР°РµС‚ РѕРєРЅРѕ РЅР° РІРµСЃСЊ СЌРєСЂР°РЅ РїСЂРё СЃРѕР·РґР°РЅРёРё (РїРµСЂС„-Р°СѓРґРёС‚: С‚РµСЃС‚РёСЂРѕРІР°РЅРёРµ
/// РІ СЂР°Р·РІС‘СЂРЅСѓС‚РѕРј РѕРєРЅРµ РїРѕ СЂРµС€РµРЅРёСЋ РїРѕР»СЊР·РѕРІР°С‚РµР»СЏ 2026-07-17). `--viewport` РїСЂРё
/// СЌС‚РѕРј РёРіРЅРѕСЂРёСЂСѓРµС‚СЃСЏ РѕРєРѕРЅРЅС‹Рј РјРµРЅРµРґР¶РµСЂРѕРј вЂ” СЂР°Р·РјРµСЂ Р·Р°РґР°С‘С‚ РјР°РєСЃРёРјРёР·Р°С†РёСЏ.
pub(crate) fn extract_maximized(args: &[String]) -> (bool, Vec<String>) {
    let mut found = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--maximized" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// РР·РІР»РµС‡СЊ `--no-scrollbar` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ, РІРµСЂРЅСѓС‚СЊ (flag, РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹).
pub(crate) fn extract_no_scrollbar(args: &[String]) -> (bool, Vec<String>) {
    let mut found = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--no-scrollbar" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// РР·РІР»РµС‡СЊ `--network-service` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ (PH1-4).
///
/// РљРѕРіРґР° С„Р»Р°Рі РїСЂРёСЃСѓС‚СЃС‚РІСѓРµС‚, С€РµР»Р» Р·Р°РїСѓСЃРєР°РµС‚ `lumen-network-service` РєР°Рє РґРѕС‡РµСЂРЅРёР№ РїСЂРѕС†РµСЃСЃ
/// Рё РґРµР»РµРіРёСЂСѓРµС‚ РІСЃРµ HTTP/TLS/DNS Р·Р°РїСЂРѕСЃС‹ С‡РµСЂРµР· IPC РІРјРµСЃС‚Рѕ РІСЃС‚СЂРѕРµРЅРЅРѕРіРѕ `HttpClient`.
pub(crate) fn extract_network_service(args: &[String]) -> (bool, Vec<String>) {
    let mut found = false;
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--network-service" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// РР·РІР»РµС‡СЊ `--ipc-server` (+ РѕРїС†РёРѕРЅР°Р»СЊРЅРѕ `--ipc-port N`) РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ (TAB-5).
///
/// `--ipc-server` Р·Р°РїСѓСЃРєР°РµС‚ С€РµР»Р» headless-СЃРµСЂРІРµСЂРѕРј С‚Р°Р±-РєРѕРјР°РЅРґ (СЃРј.
/// [`run_ipc_server`]): РІРЅРµС€РЅРёР№ РєРѕРЅС‚СЂРѕР»Р»РµСЂ (`graphic_tests/run.py`) РѕС‚РєСЂС‹РІР°РµС‚
/// Р±СЂР°СѓР·РµСЂ РѕРґРёРЅ СЂР°Р· Рё С‚СЏРЅРµС‚ PNG-СЃРЅРёРјРєРё С‡РµСЂРµР· IPC РІРјРµСЃС‚Рѕ gdigrab/ffmpeg.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(port), rest)` РµСЃР»Рё С„Р»Р°Рі РїСЂРёСЃСѓС‚СЃС‚РІСѓРµС‚, РіРґРµ `port` вЂ” СЏРІРЅС‹Р№
/// РїРѕСЂС‚ РёР· `--ipc-port N` РёР»Рё `None` (OS РЅР°Р·РЅР°С‡РёС‚ РїРѕСЂС‚, С€РµР»Р» РЅР°РїРµС‡Р°С‚Р°РµС‚ РµРіРѕ РІ
/// stdout СЃС‚СЂРѕРєРѕР№ `LUMEN_IPC_PORT=<port>`).
pub(crate) fn extract_ipc_server(args: &[String]) -> (Option<Option<u16>>, Vec<String>) {
    let mut enabled = false;
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--ipc-server" => enabled = true,
            "--ipc-port" => {
                if let Some(n) = args.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                    port = Some(n);
                    i += 1; // skip the value
                }
            }
            other => rest.push(other.to_owned()),
        }
        i += 1;
    }
    (if enabled { Some(port) } else { None }, rest)
}

/// РР·РІР»РµС‡СЊ `--activity-log` (РёР»Рё `--click-log`) РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
/// РўР°РєР¶Рµ Р°РєС‚РёРІРёСЂСѓРµС‚СЃСЏ РїРµСЂРµРјРµРЅРЅРѕР№ РѕРєСЂСѓР¶РµРЅРёСЏ `LUMEN_ACTIVITY_LOG=1`.
pub(crate) fn extract_click_log(args: &[String]) -> (bool, Vec<String>) {
    let mut found = std::env::var("LUMEN_ACTIVITY_LOG").is_ok_and(|v| v == "1")
        || std::env::var("LUMEN_CLICK_LOG").is_ok_and(|v| v == "1");
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--activity-log" || arg == "--click-log" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// РР·РІР»РµС‡СЊ `--health-log` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ (PERF-6, Р¶СѓСЂРЅР°Р» Р·РґРѕСЂРѕРІСЊСЏ СЃРµСЃСЃРёРё).
/// РўР°РєР¶Рµ Р°РєС‚РёРІРёСЂСѓРµС‚СЃСЏ РїРµСЂРµРјРµРЅРЅРѕР№ РѕРєСЂСѓР¶РµРЅРёСЏ `LUMEN_HEALTH_LOG=1`.
pub(crate) fn extract_health_log(args: &[String]) -> (bool, Vec<String>) {
    let mut found = std::env::var("LUMEN_HEALTH_LOG").is_ok_and(|v| v == "1");
    let mut rest = Vec::new();
    for arg in args {
        if arg == "--health-log" {
            found = true;
        } else {
            rest.push(arg.clone());
        }
    }
    (found, rest)
}

/// РР·РІР»РµС‡СЊ `--devtools-port N` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ, РІРµСЂРЅСѓС‚СЊ (port, РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹).
pub(crate) fn extract_devtools_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--devtools-port" {
            i += 1;
            let s = args.get(i).ok_or("--devtools-port С‚СЂРµР±СѓРµС‚ РЅРѕРјРµСЂ РїРѕСЂС‚Р°")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("РЅРµРІРµСЂРЅС‹Р№ РїРѕСЂС‚: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// РР·РІР»РµС‡СЊ `--bidi-port N` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ, РІРµСЂРЅСѓС‚СЊ (port, РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹).
pub(crate) fn extract_bidi_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--bidi-port" {
            i += 1;
            let s = args.get(i).ok_or("--bidi-port С‚СЂРµР±СѓРµС‚ РЅРѕРјРµСЂ РїРѕСЂС‚Р°")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("РЅРµРІРµСЂРЅС‹Р№ РїРѕСЂС‚: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// РР·РІР»РµС‡СЊ `--mcp-live-port N` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ, РІРµСЂРЅСѓС‚СЊ (port, РѕСЃС‚Р°Р»СЊРЅС‹Рµ Р°СЂРіСѓРјРµРЅС‚С‹).
///
/// РћС‚РґРµР»СЊРЅРѕ РѕС‚ `--mcp`/`--mcp-port` (СЃРј. [`extract_mcp_mode`]): С‚Рµ РІС‹Р±РёСЂР°СЋС‚
/// СЌРєСЃРєР»СЋР·РёРІРЅС‹Р№ headless `CliMode::Mcp` РїРѕРІРµСЂС… `InProcessSession`. Р­С‚РѕС‚ С„Р»Р°Рі вЂ”
/// РєР°Рє `--bidi-port`/`--devtools-port` вЂ” РїРѕРґРЅРёРјР°РµС‚ MCP-С„СЂРѕРЅС‚ С„РѕРЅРѕРІС‹Рј РїРѕС‚РѕРєРѕРј
/// СЂСЏРґРѕРј СЃ Р»СЋР±С‹Рј РґСЂСѓРіРёРј СЂРµР¶РёРјРѕРј, РЅР°РїСЂР°РІР»РµРЅРЅС‹Р№ РЅР° Р¶РёРІРѕРµ РѕРєРЅРѕ С‡РµСЂРµР· SDC-2
/// (`lumen_mcp::spawn_live`), С‡С‚РѕР±С‹ `screenshot`/`eval` СЂР°Р±РѕС‚Р°Р»Рё РїРѕ-РЅР°СЃС‚РѕСЏС‰РµРјСѓ.
pub(crate) fn extract_mcp_live_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--mcp-live-port" {
            i += 1;
            let s = args.get(i).ok_or("--mcp-live-port С‚СЂРµР±СѓРµС‚ РЅРѕРјРµСЂ РїРѕСЂС‚Р°")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("РЅРµРІРµСЂРЅС‹Р№ РїРѕСЂС‚: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// РР·РІР»РµС‡СЊ `--proxy http://host:port` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
pub(crate) fn extract_proxy(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut proxy: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--proxy" {
            i += 1;
            let s = args.get(i).ok_or("--proxy С‚СЂРµР±СѓРµС‚ Р°РґСЂРµСЃ (http://host:port РёР»Рё https://host:port)")?;
            proxy = Some(s.clone());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((proxy, rest))
}

/// РР·РІР»РµС‡СЊ `--tor` / `--tor-port N` РёР· Р°СЂРіСѓРјРµРЅС‚РѕРІ.
///
/// `--tor` Р°РєС‚РёРІРёСЂСѓРµС‚ Tor-СЂРµР¶РёРј: РїСЂРѕС„РёР»СЊ TorBrowser, SOCKS5 С‡РµСЂРµР· Р»РѕРєР°Р»СЊРЅС‹Р№ РґРµРјРѕРЅ,
/// Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ С…СЂР°РЅРёР»РёС‰Р°. `--tor-port N` РїРµСЂРµРѕРїСЂРµРґРµР»СЏРµС‚ РїРѕСЂС‚ SOCKS5
/// (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ 9050; Tor Browser bundle РёСЃРїРѕР»СЊР·СѓРµС‚ 9150).
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(Some(port), РѕСЃС‚Р°Р»СЊРЅС‹Рµ_Р°СЂРіСѓРјРµРЅС‚С‹)` РёР»Рё `(None, РІСЃРµ_Р°СЂРіСѓРјРµРЅС‚С‹)`.
pub(crate) fn extract_tor_mode(args: &[String]) -> (Option<u16>, Vec<String>) {
    let mut port: u16 = 9050;
    let mut tor_found = false;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--tor" {
            tor_found = true;
        } else if args[i] == "--tor-port" {
            i += 1;
            if let Some(p) = args.get(i).and_then(|s| s.parse().ok()) {
                port = p;
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    if tor_found {
        (Some(port), rest)
    } else {
        (None, rest)
    }
}

/// РџСЂРѕРІРµСЂРёС‚СЊ РґРѕСЃС‚СѓРїРЅРѕСЃС‚СЊ Tor-РґРµРјРѕРЅР°: РїРѕРїС‹С‚Р°С‚СЊСЃСЏ РѕС‚РєСЂС‹С‚СЊ TCP-СЃРѕРµРґРёРЅРµРЅРёРµ Рє SOCKS5-РїРѕСЂС‚Сѓ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `true` РµСЃР»Рё РїРѕСЂС‚ РїСЂРёРЅРёРјР°РµС‚ СЃРѕРµРґРёРЅРµРЅРёСЏ (С‚Р°Р№РјР°СѓС‚ 2 СЃ). РќРµ РІС‹РїРѕР»РЅСЏРµС‚
/// SOCKS5-С…СЌРЅРґС€РµР№Рє вЂ” С‚РѕР»СЊРєРѕ РїСЂРѕРІРµСЂСЏРµС‚ С‡С‚Рѕ СЃРѕРєРµС‚ СЃР»СѓС€Р°РµС‚.
pub(crate) fn check_tor_connectivity(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

/// Р РµР¶РёРј Р·Р°РїСѓСЃРєР° shell. Р РµС€Р°РµС‚СЃСЏ РЅР° РѕСЃРЅРѕРІРµ CLI-Р°СЂРіСѓРјРµРЅС‚РѕРІ РІ `parse_cli`.
#[derive(Debug, Clone)]
pub(crate) enum CliMode {
    /// РћР±С‹С‡РЅРѕРµ РѕРєРЅРѕ вЂ” С‚РµРєСѓС‰РёР№ source РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РІ winit-РѕРєРЅРµ.
    OpenWindow(PageSource),
    /// Headless: pipeline РїСЂРѕРіРѕРЅСЏРµС‚СЃСЏ РґРѕ РЅСѓР¶РЅРѕР№ С„Р°Р·С‹, СЂРµР·СѓР»СЊС‚Р°С‚ РёРґС‘С‚ РІ stdout.
    Dump { source: PageSource, kind: DumpKind },
    /// Headless: СЃС‚СЂР°РЅРёС†Р° СЂРµРЅРґРµСЂРёС‚СЃСЏ РїРѕСЃС‚СЂР°РЅРёС‡РЅРѕ Рё СЃРѕС…СЂР°РЅСЏРµС‚СЃСЏ РєР°Рє PDF.
    PrintToPdf { source: PageSource, output: std::path::PathBuf },
    /// Headless: СЃС‚СЂР°РЅРёС†Р° СЂРµРЅРґРµСЂРёС‚СЃСЏ CPU-СЂР°СЃС‚РµСЂРёР·Р°С‚РѕСЂРѕРј Рё СЃРѕС…СЂР°РЅСЏРµС‚СЃСЏ РєР°Рє PNG.
    Screenshot { source: PageSource, output: std::path::PathBuf },
    /// Headless (PERF-1): РѕРґРЅР° РЅР°РІРёРіР°С†РёСЏ РїСЂРѕРіРѕРЅСЏРµС‚СЃСЏ С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ CPU-РїСѓС‚СЊ, С‡С‚Рѕ Рё
    /// `--screenshot`, РЅРѕ СЃ РІРєР»СЋС‡С‘РЅРЅС‹Рј С‚СЂРµР№СЃРµСЂРѕРј; С‚Р°Р№РјР»Р°Р№РЅ СЃРѕС…СЂР°РЅСЏРµС‚СЃСЏ РєР°Рє
    /// Chrome-trace JSON РґР»СЏ Perfetto / `chrome://tracing`.
    TraceNav { source: PageSource, output: std::path::PathBuf },
    /// Headless: MCP-СЃРµСЂРІРµСЂ РґР»СЏ AI-Р°РіРµРЅС‚РѕРІ (Claude, Browser UseвЂ¦).
    Mcp(McpMode),
    /// Headless: IPC-СЃРµСЂРІРµСЂ С‚Р°Р±-РєРѕРјР°РЅРґ (TAB-5). РљРѕРЅС‚СЂРѕР»Р»РµСЂ РґСЂР°Р№РІРёС‚ РІРєР»Р°РґРєРё Рё
    /// РїРѕР»СѓС‡Р°РµС‚ PNG-СЃРЅРёРјРєРё С‡РµСЂРµР· TCP. `Some(port)` вЂ” СЏРІРЅС‹Р№ РїРѕСЂС‚, `None` вЂ” OS.
    IpcServer { port: Option<u16> },
}

impl CliMode {
    /// Short stable label for this mode, used by the PERF-12 startup accounting
    /// (`startup_trace::Startup::dispatch`) to name the point where fixed
    /// startup ends and page work begins.
    pub(crate) fn mode_name(&self) -> &'static str {
        match self {
            Self::OpenWindow(_) => "window",
            Self::Dump { kind, .. } => match kind {
                DumpKind::Source => "dump-source",
                DumpKind::Layout => "dump-layout",
                DumpKind::DisplayList => "dump-display-list",
            },
            Self::PrintToPdf { .. } => "print-to-pdf",
            Self::Screenshot { .. } => "screenshot",
            Self::TraceNav { .. } => "trace-nav",
            Self::Mcp(_) => "mcp",
            Self::IpcServer { .. } => "ipc-server",
        }
    }
}

/// РџР°СЂР°РјРµС‚СЂС‹ MCP-СЂРµР¶РёРјР°.
#[derive(Debug, Clone)]
pub(crate) struct McpMode {
    /// РќР°С‡Р°Р»СЊРЅС‹Р№ URL (РµСЃР»Рё СѓРєР°Р·Р°РЅ).
    pub(crate) url: Option<String>,
    /// TCP-РїРѕСЂС‚ РґР»СЏ `--mcp-port N`. None в†’ stdio.
    pub(crate) port: Option<u16>,
}

/// Р§С‚Рѕ РёРјРµРЅРЅРѕ РїРµС‡Р°С‚Р°С‚СЊ РІ dump-СЂРµР¶РёРјРµ.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum DumpKind {
    /// Р”РµРєРѕРґРёСЂРѕРІР°РЅРЅС‹Р№ HTML РїРѕСЃР»Рµ `lumen_encoding::decode`.
    Source,
    /// `serialize_layout_tree` вЂ” РґРµС‚РµСЂРјРёРЅРёСЂРѕРІР°РЅРЅС‹Р№ С‚РµРєСЃС‚РѕРІС‹Р№ С„РѕСЂРјР°С‚ layout-РґРµСЂРµРІР°.
    Layout,
    /// `serialize_display_list` вЂ” С‚РµРєСЃС‚РѕРІС‹Р№ С„РѕСЂРјР°С‚ paint-РєРѕРјР°РЅРґ.
    DisplayList,
}

impl DumpKind {
    pub(crate) fn from_flag(s: &str) -> Option<Self> {
        match s {
            "--dump-source" => Some(DumpKind::Source),
            "--dump-layout" => Some(DumpKind::Layout),
            "--dump-display-list" => Some(DumpKind::DisplayList),
            _ => None,
        }
    }
}

/// Р Р°Р·РѕР±СЂР°С‚СЊ Р°СЂРіСѓРјРµРЅС‚С‹ (Р±РµР· `argv[0]`) РІ СЂРµР¶РёРј Р·Р°РїСѓСЃРєР°.
///
/// Р“СЂР°РјРјР°С‚РёРєР°:
/// - `[]`           в†’ OpenWindow(Empty)
/// - `[arg]`        в†’ OpenWindow(from_arg(arg)) РµСЃР»Рё arg РЅРµ dump-С„Р»Р°Рі; РёРЅР°С‡Рµ РѕС€РёР±РєР°
/// - `[flag, tgt]`  в†’ Dump РµСЃР»Рё flag вЂ” dump-С„Р»Р°Рі; РёРЅР°С‡Рµ РѕС€РёР±РєР°
/// - `[вЂ¦]` (>2)     в†’ РѕС€РёР±РєР°
///
/// Dump-С„Р»Р°РіРё РїСЂРёРЅРёРјР°СЋС‚СЃСЏ С‚РѕР»СЊРєРѕ РІ РїРµСЂРІРѕР№ РїРѕР·РёС†РёРё вЂ” РёРЅР°С‡Рµ РїСЂРёС€Р»РѕСЃСЊ Р±С‹ РїР°СЂСЃРёС‚СЊ
/// Р°СЂРіСѓРјРµРЅС‚С‹ РїСЂРѕРёР·РІРѕР»СЊРЅС‹Рј РїРѕСЂСЏРґРєРѕРј, С‡С‚Рѕ РЅРµ РЅСѓР¶РЅРѕ РґР»СЏ С‚РµРєСѓС‰РµРіРѕ СЃРєРѕСѓРїР°.
pub(crate) fn parse_cli(args: &[String]) -> Result<CliMode, String> {
    match args {
        [] => Ok(CliMode::OpenWindow(PageSource::Empty)),
        [arg] => {
            if DumpKind::from_flag(arg).is_some() {
                Err(format!("С„Р»Р°Рі {arg} С‚СЂРµР±СѓРµС‚ РїСѓС‚СЊ РёР»Рё URL"))
            } else if arg.starts_with("--") {
                Err(format!("РЅРµРёР·РІРµСЃС‚РЅС‹Р№ С„Р»Р°Рі: {arg}"))
            } else {
                Ok(CliMode::OpenWindow(PageSource::from_arg(Some(arg))))
            }
        }
        [flag, target] => {
            let kind = DumpKind::from_flag(flag)
                .ok_or_else(|| format!("РЅРµРёР·РІРµСЃС‚РЅС‹Р№ С„Р»Р°Рі: {flag}"))?;
            if target.starts_with("--") {
                return Err(format!(
                    "РѕР¶РёРґР°Р»СЃСЏ РїСѓС‚СЊ РёР»Рё URL РїРѕСЃР»Рµ {flag}, РїРѕР»СѓС‡РµРЅ С„Р»Р°Рі {target}"
                ));
            }
            Ok(CliMode::Dump {
                source: PageSource::from_arg(Some(target)),
                kind,
            })
        }
        _ => Err(format!("СЃР»РёС€РєРѕРј РјРЅРѕРіРѕ Р°СЂРіСѓРјРµРЅС‚РѕРІ: {}", args.len())),
    }
}

/// Parses the command line and dispatches to the chosen `CliMode`.
///
/// Split out of `main` so that every `return` inside it вЂ” including the early
/// argument-error paths вЂ” is followed by `diag_stderr::flush`.
pub(crate) fn run_cli() -> ExitCode {
    // Opt-in visual profiler (В§14.3, BUG-284): `Client::start()` spawns the
    // background thread that connects to (or is discovered by) a running
    // Tracy GUI app вЂ” https://github.com/wolfpld/tracy. Must be started
    // before any `lumen_core::tracy_zone!` spans fire, so this is the very
    // first thing main() does. No-op unless built with `--features tracy`.
    #[cfg(feature = "tracy")]
    let _tracy_client = tracy_client::Client::start();
    #[cfg(feature = "tracy")]
    eprintln!("[tracy] РїСЂРѕС„РёР»РёСЂРѕРІС‰РёРє Р°РєС‚РёРІРµРЅ вЂ” РѕС‚РєСЂРѕР№ Tracy GUI, С‡С‚РѕР±С‹ СѓРІРёРґРµС‚СЊ С‚Р°Р№РјР»Р°Р№РЅ");

    // Anchor for launch->first-frame timing (В§4 score table) вЂ” before any work.
    bench_frames::mark_process_start();
    // PERF-12: fixed-startup stopwatch. Must precede the config load and the
    // argument parse below, which are the phases it measures; it also switches
    // the tracer on for `--trace-nav`, so that startup lands on the timeline
    // instead of ahead of its origin.
    let startup = startup_trace::Startup::begin();
    let cfg_phase = startup.phase("config-load");
    // Load the fingerprint profile (9F.1) once, before any network or JS setup.
    // Absent config в†’ engine defaults, so behaviour is unchanged out of the box.
    let mut startup_profile = config::load().unwrap_or_default();
    // BUG-295: automation sessions (BiDi / MCP) use an in-memory HTTP cache, never
    // the persistent on-disk one. The disk cache is keyed by URL and survives across
    // runs, so on the fixed ports an automation server reuses (e.g. wptserve's
    // 8000/8001) a resource fetched in one run is replayed stale in the next вЂ” even
    // after the served file changed on disk. That silently broke
    // `tests/wpt/run_smoke.py`: the first run (before the wptrunner `env_options` fix
    // served the right file) cached the wrong `testharnessreport.js` with its
    // `Cache-Control: max-age=3600`, and every later run kept serving that stale copy
    // from disk, setting the wrong result global forever, so the harness timed out no
    // matter what else was fixed. In-memory cache = fresh per process, deterministic.
    // This must be decided BEFORE `init_global` вЂ” the profile `OnceLock` is set-once,
    // so a later `init_global` is a no-op вЂ” hence a raw arg scan here rather than
    // reusing the `extract_*` parsers below.
    if std::env::args()
        .any(|a| matches!(a.as_str(), "--bidi-port" | "--mcp-live-port" | "--mcp" | "--mcp-port"))
    {
        startup_profile.no_persistent_state = true;
    }
    config::init_global(startup_profile);
    drop(cfg_phase);

    let arg_phase = startup.phase("arg-parse");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (devtools_port, rest_args) = match extract_devtools_port(&args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (bidi_port, rest_args) = match extract_bidi_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (mcp_live_port, rest_args) = match extract_mcp_live_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (import_session, rest_args) = match extract_import_session(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --import-session: {err}");
            return ExitCode::FAILURE;
        }
    };
    let (no_scrollbar, rest_args) = extract_no_scrollbar(&rest_args);
    let (maximized, rest_args) = extract_maximized(&rest_args);
    let (click_log_flag, rest_args) = extract_click_log(&rest_args);
    click_log::init(click_log_flag);
    // PERF-6: session health journal. Turned on by `--activity-log`/`--click-log`
    // (shared surface), the dedicated `--health-log`, or `LUMEN_HEALTH_LOG=1`.
    let (health_log_flag, rest_args) = extract_health_log(&rest_args);
    health_log::init(click_log_flag || health_log_flag);
    let (det_cfg, rest_args) = deterministic::extract_deterministic(&rest_args);
    let (viewport_override, rest_args) = extract_viewport_override(&rest_args);
    let (pdf_output, rest_args) = extract_print_to_pdf(&rest_args);
    let (screenshot_output, rest_args) = extract_screenshot(&rest_args);
    let (trace_nav_output, rest_args) = extract_trace_nav(&rest_args);
    let (mcp_mode, rest_args) = extract_mcp_mode(&rest_args);
    let (use_network_service, rest_args) = extract_network_service(&rest_args);
    let (ipc_server, rest_args) = extract_ipc_server(&rest_args);
    let (proxy, rest_args) = match extract_proxy(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --proxy: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Р•СЃР»Рё РїСЂРѕРєСЃРё РїРµСЂРµРґР°РЅ РІ РєРѕРјР°РЅРґРЅРѕР№ СЃС‚СЂРѕРєРµ, РїРµСЂРµРѕРїСЂРµРґРµР»РёС‚СЊ РєРѕРЅС„РёРі.
    if let Some(proxy_str) = proxy {
        let mut cfg = config::global().clone();
        cfg.proxy = Some(proxy_str);
        config::init_global(cfg);
    }

    let (tor_port, rest_args) = extract_tor_mode(&rest_args);

    // --tor: РїРµСЂРµРєР»СЋС‡РёС‚СЊ РЅР° РїСЂРѕС„РёР»СЊ TorBrowser + SOCKS5 + Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ С…СЂР°РЅРёР»РёС‰Р°.
    if let Some(port) = tor_port {
        if !check_tor_connectivity(port) {
            eprintln!(
                "lumen --tor: Tor-РґРµРјРѕРЅ РЅРµРґРѕСЃС‚СѓРїРµРЅ РЅР° 127.0.0.1:{port} вЂ” \
                 Р·Р°РїСѓСЃС‚РёС‚Рµ Tor РїРµСЂРµРґ Р·Р°РїСѓСЃРєРѕРј Lumen"
            );
            return ExitCode::FAILURE;
        }
        let mut cfg = config::global().clone();
        cfg.http_profile = lumen_network::HttpProfile::TorBrowser;
        cfg.socks5_proxy = Some(format!("socks5://127.0.0.1:{port}"));
        cfg.no_persistent_state = true;
        config::init_global(cfg);
        eprintln!(
            "lumen: Tor-СЂРµР¶РёРј Р°РєС‚РёРІРёСЂРѕРІР°РЅ (socks5://127.0.0.1:{port}, \
             РїСЂРѕС„РёР»СЊ TorBrowser, Р±РµР· РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ С…СЂР°РЅРёР»РёС‰Р°)"
        );
    }

    let cli = if let Some(output) = pdf_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::PrintToPdf { source, output }
    } else if let Some(output) = screenshot_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::Screenshot { source, output }
    } else if let Some(output) = trace_nav_output {
        let source = PageSource::from_arg(rest_args.first().map(|s| s.as_str()));
        CliMode::TraceNav { source, output }
    } else if let Some(port) = ipc_server {
        CliMode::IpcServer { port }
    } else if let Some(mcp) = mcp_mode {
        CliMode::Mcp(mcp)
    } else {
        match parse_cli(&rest_args) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("РћС€РёР±РєР° Р°СЂРіСѓРјРµРЅС‚РѕРІ: {err}");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    };

    drop(arg_phase);
    let svc_phase = startup.phase("services-init");

    if let Some(port) = devtools_port
        && let Err(e) = DevToolsServer::spawn(port)
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° DevTools РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    // SDC-2: automation channel created here (not inside run_window_mode) so
    // BiDi/MCP front-ends spawned below get a handle that stays valid once the
    // live window's event loop starts draining `automation_rx`. Without an
    // open window (e.g. --dump/--screenshot/--mcp combined with --bidi-port),
    // the receiver is simply never drained and calls through the handle time out.
    let (automation_cmd_tx, automation_rx) =
        std::sync::mpsc::channel::<AutomationRequest>();
    let automation_handle = AutomationHandle::new(automation_cmd_tx.clone());

    if let Some(port) = bidi_port
        && let Err(e) = bidi_spawn(port, automation_handle.clone())
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° BiDi РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    if let Some(port) = mcp_live_port
        && let Err(e) = lumen_mcp::spawn_live(port, automation_handle.clone())
    {
        eprintln!("РћС€РёР±РєР° Р·Р°РїСѓСЃРєР° MCP (live) РЅР° РїРѕСЂС‚Сѓ {port}: {e}");
        return ExitCode::FAILURE;
    }

    let blocked_log = Arc::new(std::sync::Mutex::new(
        panels::shields_panel::BlockedLog::default(),
    ));
    let network_log = Arc::new(std::sync::Mutex::new(
        devtools::network_panel::NetworkLog::default(),
    ));
    // Sink chain: StdoutEventSink в†’ NetworkLogSink в†’ ResourceTimingSink в†’
    // ShieldCountSink. Each wrapper forwards to its inner sink, so all four
    // observe every event вЂ” the Resource Timing capture (BUG-839) is a tap, not
    // a filter.
    let event_sink: Arc<dyn EventSink> = Arc::new(panels::shields_panel::ShieldCountSink {
        inner: Arc::new(resource_timing::ResourceTimingSink {
            inner: Arc::new(devtools::network_panel::NetworkLogSink {
                inner: Arc::new(StdoutEventSink),
                log: Arc::clone(&network_log),
            }),
        }),
        log: Arc::clone(&blocked_log),
    });

    // PH1-4: Р—Р°РїСѓСЃС‚РёС‚СЊ СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ РєР°Рє РґРѕС‡РµСЂРЅРёР№ РїСЂРѕС†РµСЃСЃ (РµСЃР»Рё --network-service).
    // РҐРµРЅРґР» Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° main() вЂ” РїСЂРё РґСЂРѕРїРµ СѓР±РёРІР°РµС‚ РґРѕС‡РµСЂРЅРёР№ РїСЂРѕС†РµСЃСЃ.
    // _transport С…СЂР°РЅРёС‚ Arc, С‡С‚РѕР±С‹ РЅРµ РґСЂРѕРїРЅСѓС‚СЊ IPC-СЃРѕРµРґРёРЅРµРЅРёРµ РґРѕ РєРѕРЅС†Р° СЃРµСЃСЃРёРё.
    let (_network_svc, _transport) = if use_network_service {
        match network_service::NetworkServiceHandle::spawn() {
            Ok((handle, transport)) => {
                eprintln!("lumen: СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ Р·Р°РїСѓС‰РµРЅ (PH1-4, --network-service)");
                (Some(handle), Some(transport))
            }
            Err(e) => {
                eprintln!("lumen: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ СЃРµС‚РµРІРѕР№ СЃРµСЂРІРёСЃ: {e}");
                eprintln!("lumen: РїСЂРѕРґРѕР»Р¶Р°СЋ СЃРѕ РІСЃС‚СЂРѕРµРЅРЅС‹Рј HttpClient");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // --import-session РїРµСЂРµРѕРїСЂРµРґРµР»СЏРµС‚ РёСЃС‚РѕС‡РЅРёРє СЃС‚СЂР°РЅРёС†С‹ Рё РЅР°С‡Р°Р»СЊРЅС‹Р№ scroll.
    let (cli, initial_scroll) = match import_session {
        Some((session_source, scroll)) => (CliMode::OpenWindow(session_source), scroll),
        None => (cli, (0.0_f32, 0.0_f32)),
    };

    drop(svc_phase);
    startup.dispatch(cli.mode_name());

    match cli {
        CliMode::Dump { source, kind } => {
            run_dump_mode(&source, kind, event_sink, viewport_override)
        }
        CliMode::OpenWindow(source) => run_window_mode(source, event_sink, blocked_log, network_log, initial_scroll, no_scrollbar, maximized, det_cfg, viewport_override, automation_handle, automation_cmd_tx, automation_rx, bidi_port.is_some() || mcp_live_port.is_some()),
        CliMode::PrintToPdf { source, output } => run_print_to_pdf(&source, &output, event_sink),
        CliMode::Screenshot { source, output } => {
            run_screenshot(&source, &output, event_sink, viewport_override)
        }
        CliMode::TraceNav { source, output } => run_trace_nav(&source, &output, event_sink),
        CliMode::Mcp(mcp) => run_mcp_mode(mcp),
        CliMode::IpcServer { port } => run_ipc_server(port, event_sink),
    }
}
