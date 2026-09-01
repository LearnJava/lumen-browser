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
    eprintln!("Использование:");
    eprintln!("  lumen                                           — пустое окно");
    eprintln!("  lumen <path-or-url>                             — открыть страницу в окне");
    eprintln!("  lumen --dump-source <path-or-url>               — декодированный HTML в stdout");
    eprintln!("  lumen --dump-layout <path-or-url>               — layout-дерево в stdout");
    eprintln!("  lumen --dump-display-list <path-or-url>         — display list в stdout");
    eprintln!("  lumen --print-to-pdf <out.pdf> <path-or-url>   — сохранить страницу как PDF");
    eprintln!("  lumen --screenshot <out.png> <path-or-url>     — CPU-снимок страницы в PNG (без окна)");
    eprintln!("  lumen --trace-nav <out.json> <path-or-url>     — таймлайн одной навигации в Chrome-trace JSON");
    eprintln!("  [--devtools-port <N>]                           — DevTools WS сервер (любой режим)");
    eprintln!("  [--bidi-port <N>]                               — WebDriver BiDi WS сервер (любой режим)");
    eprintln!("  [--mcp-live-port <N>]                           — MCP-сервер (TCP) на живом окне (любой режим, SDC-2)");
    eprintln!("  [--viewport <W>x<H>]                            — фикс. CSS-размер окна (переопределяет --deterministic 1280×800)");
    eprintln!("  [--maximized]                                   — развернуть окно на весь экран (живой перф-аудит)");
    eprintln!("  [--proxy <url>]                                 — HTTP прокси (http://host:port или user:pass@host:port)");
    eprintln!("  [--tor [--tor-port <N>]]                        — Tor-режим: TorBrowser fingerprint + SOCKS5 9050 (или N)");
    eprintln!("  --import-session <file.lsession>                — восстановить сессию из файла");
    eprintln!("  --mcp [url]                                     — MCP-сервер (stdio) для AI-агентов");
    eprintln!("  --mcp-port <N> [url]                            — MCP-сервер (TCP) на порту N");
    eprintln!("  [--network-service]                             — вынести HTTP/TLS/DNS в отдельный процесс (PH1-4)");
    eprintln!("  --ipc-server                                    — headless IPC-сервер таб-команд: PNG-снимки через TCP (TAB-5)");
}

/// Извлечь `--print-to-pdf <output.pdf>` из аргументов.
///
/// Возвращает `(Some(output_path), остальные_аргументы)` или `(None, все_аргументы)`.
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

/// Извлечь `--screenshot <output.png>` из аргументов.
///
/// Возвращает `(Some(output_path), остальные_аргументы)` или `(None, все_аргументы)`.
/// Порядок аргументов зеркалит `--print-to-pdf`: путь вывода идёт сразу за флагом,
/// источник страницы — позиционный остаток (`--screenshot out.png <url>`).
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

/// Извлечь `--trace-nav <output.json>` из аргументов (PERF-1).
///
/// Возвращает `(Some(output_path), остальные_аргументы)` или `(None, все_аргументы)`.
/// Порядок аргументов зеркалит `--screenshot`: путь вывода идёт сразу за флагом,
/// источник страницы — позиционный остаток (`--trace-nav out.json <url>`).
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

/// Извлечь `--mcp` / `--mcp-port N` из аргументов.
///
/// Возвращает `(Some(McpMode), остальные_аргументы)` или `(None, все_аргументы)`.
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

/// Результат разбора `--import-session`: (source, (scroll_x, scroll_y)).
pub(crate) type ImportedSession = (PageSource, (f32, f32));

/// Извлечь `--import-session <file>` из аргументов.
///
/// Возвращает (Some((source, (scroll_x, scroll_y))), остальные аргументы)
/// или (None, аргументы) если флаг не указан.
pub(crate) fn extract_import_session(
    args: &[String],
) -> Result<(Option<ImportedSession>, Vec<String>), String> {
    let mut session: Option<(PageSource, (f32, f32))> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--import-session" {
            i += 1;
            let path = args.get(i).ok_or("--import-session требует путь к файлу")?;
            let json = std::fs::read_to_string(path)
                .map_err(|e| format!("не удалось прочитать {path}: {e}"))?;
            let file = session_export::from_json(&json)
                .map_err(|e| format!("ошибка разбора сессии {path}: {e}"))?;
            let tab = session_export::active_tab(&file)
                .ok_or_else(|| format!("сессия {path} не содержит вкладок"))?;
            let source = PageSource::from_arg(Some(&tab.url));
            session = Some((source, (tab.scroll_x, tab.scroll_y)));
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((session, rest))
}

/// Извлечь `--viewport <W>x<H>` из аргументов (DEVX-1).
///
/// Overrides the window's CSS content viewport size (window height still adds
/// `toolbar::CHROME_H` — tab bar + toolbar — on top, same as the
/// non-deterministic default — see `resumed()`). Needed because
/// `--deterministic` forces a 1280×800 window,
/// which breaks `graphic_tests/run.py --live`'s magenta-marker crop calibration
/// (baked in at the pipeline's fixed 1024×720 viewport); this flag lets a caller
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

/// Извлечь `--maximized` из аргументов, вернуть (flag, остальные аргументы).
///
/// Разворачивает окно на весь экран при создании (перф-аудит: тестирование
/// в развёрнутом окне по решению пользователя 2026-07-17). `--viewport` при
/// этом игнорируется оконным менеджером — размер задаёт максимизация.
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

/// Извлечь `--no-scrollbar` из аргументов, вернуть (flag, остальные аргументы).
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

/// Извлечь `--network-service` из аргументов (PH1-4).
///
/// Когда флаг присутствует, шелл запускает `lumen-network-service` как дочерний процесс
/// и делегирует все HTTP/TLS/DNS запросы через IPC вместо встроенного `HttpClient`.
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

/// Извлечь `--ipc-server` (+ опционально `--ipc-port N`) из аргументов (TAB-5).
///
/// `--ipc-server` запускает шелл headless-сервером таб-команд (см.
/// [`run_ipc_server`]): внешний контроллер (`graphic_tests/run.py`) открывает
/// браузер один раз и тянет PNG-снимки через IPC вместо gdigrab/ffmpeg.
///
/// Возвращает `(Some(port), rest)` если флаг присутствует, где `port` — явный
/// порт из `--ipc-port N` или `None` (OS назначит порт, шелл напечатает его в
/// stdout строкой `LUMEN_IPC_PORT=<port>`).
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

/// Извлечь `--activity-log` (или `--click-log`) из аргументов.
/// Также активируется переменной окружения `LUMEN_ACTIVITY_LOG=1`.
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

/// Извлечь `--health-log` из аргументов (PERF-6, журнал здоровья сессии).
/// Также активируется переменной окружения `LUMEN_HEALTH_LOG=1`.
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

/// Извлечь `--devtools-port N` из аргументов, вернуть (port, остальные аргументы).
pub(crate) fn extract_devtools_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--devtools-port" {
            i += 1;
            let s = args.get(i).ok_or("--devtools-port требует номер порта")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// Извлечь `--bidi-port N` из аргументов, вернуть (port, остальные аргументы).
pub(crate) fn extract_bidi_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--bidi-port" {
            i += 1;
            let s = args.get(i).ok_or("--bidi-port требует номер порта")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// Извлечь `--mcp-live-port N` из аргументов, вернуть (port, остальные аргументы).
///
/// Отдельно от `--mcp`/`--mcp-port` (см. [`extract_mcp_mode`]): те выбирают
/// эксклюзивный headless `CliMode::Mcp` поверх `InProcessSession`. Этот флаг —
/// как `--bidi-port`/`--devtools-port` — поднимает MCP-фронт фоновым потоком
/// рядом с любым другим режимом, направленный на живое окно через SDC-2
/// (`lumen_mcp::spawn_live`), чтобы `screenshot`/`eval` работали по-настоящему.
pub(crate) fn extract_mcp_live_port(args: &[String]) -> Result<(Option<u16>, Vec<String>), String> {
    let mut port: Option<u16> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--mcp-live-port" {
            i += 1;
            let s = args.get(i).ok_or("--mcp-live-port требует номер порта")?;
            port = Some(s.parse::<u16>().map_err(|_| format!("неверный порт: {s}"))?);
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((port, rest))
}

/// Извлечь `--proxy http://host:port` из аргументов.
pub(crate) fn extract_proxy(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut proxy: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--proxy" {
            i += 1;
            let s = args.get(i).ok_or("--proxy требует адрес (http://host:port или https://host:port)")?;
            proxy = Some(s.clone());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((proxy, rest))
}

/// Извлечь `--tor` / `--tor-port N` из аргументов.
///
/// `--tor` активирует Tor-режим: профиль TorBrowser, SOCKS5 через локальный демон,
/// без персистентного хранилища. `--tor-port N` переопределяет порт SOCKS5
/// (по умолчанию 9050; Tor Browser bundle использует 9150).
///
/// Возвращает `(Some(port), остальные_аргументы)` или `(None, все_аргументы)`.
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

/// Проверить доступность Tor-демона: попытаться открыть TCP-соединение к SOCKS5-порту.
///
/// Возвращает `true` если порт принимает соединения (таймаут 2 с). Не выполняет
/// SOCKS5-хэндшейк — только проверяет что сокет слушает.
pub(crate) fn check_tor_connectivity(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}

/// Режим запуска shell. Решается на основе CLI-аргументов в `parse_cli`.
#[derive(Debug, Clone)]
pub(crate) enum CliMode {
    /// Обычное окно — текущий source открывается в winit-окне.
    OpenWindow(PageSource),
    /// Headless: pipeline прогоняется до нужной фазы, результат идёт в stdout.
    Dump { source: PageSource, kind: DumpKind },
    /// Headless: страница рендерится постранично и сохраняется как PDF.
    PrintToPdf { source: PageSource, output: std::path::PathBuf },
    /// Headless: страница рендерится CPU-растеризатором и сохраняется как PNG.
    Screenshot { source: PageSource, output: std::path::PathBuf },
    /// Headless (PERF-1): одна навигация прогоняется через тот же CPU-путь, что и
    /// `--screenshot`, но с включённым трейсером; таймлайн сохраняется как
    /// Chrome-trace JSON для Perfetto / `chrome://tracing`.
    TraceNav { source: PageSource, output: std::path::PathBuf },
    /// Headless: MCP-сервер для AI-агентов (Claude, Browser Use…).
    Mcp(McpMode),
    /// Headless: IPC-сервер таб-команд (TAB-5). Контроллер драйвит вкладки и
    /// получает PNG-снимки через TCP. `Some(port)` — явный порт, `None` — OS.
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

/// Параметры MCP-режима.
#[derive(Debug, Clone)]
pub(crate) struct McpMode {
    /// Начальный URL (если указан).
    pub(crate) url: Option<String>,
    /// TCP-порт для `--mcp-port N`. None → stdio.
    pub(crate) port: Option<u16>,
}

/// Что именно печатать в dump-режиме.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum DumpKind {
    /// Декодированный HTML после `lumen_encoding::decode`.
    Source,
    /// `serialize_layout_tree` — детерминированный текстовый формат layout-дерева.
    Layout,
    /// `serialize_display_list` — текстовый формат paint-команд.
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

/// Разобрать аргументы (без `argv[0]`) в режим запуска.
///
/// Грамматика:
/// - `[]`           → OpenWindow(Empty)
/// - `[arg]`        → OpenWindow(from_arg(arg)) если arg не dump-флаг; иначе ошибка
/// - `[flag, tgt]`  → Dump если flag — dump-флаг; иначе ошибка
/// - `[…]` (>2)     → ошибка
///
/// Dump-флаги принимаются только в первой позиции — иначе пришлось бы парсить
/// аргументы произвольным порядком, что не нужно для текущего скоупа.
pub(crate) fn parse_cli(args: &[String]) -> Result<CliMode, String> {
    match args {
        [] => Ok(CliMode::OpenWindow(PageSource::Empty)),
        [arg] => {
            if DumpKind::from_flag(arg).is_some() {
                Err(format!("флаг {arg} требует путь или URL"))
            } else if arg.starts_with("--") {
                Err(format!("неизвестный флаг: {arg}"))
            } else {
                Ok(CliMode::OpenWindow(PageSource::from_arg(Some(arg))))
            }
        }
        [flag, target] => {
            let kind = DumpKind::from_flag(flag)
                .ok_or_else(|| format!("неизвестный флаг: {flag}"))?;
            if target.starts_with("--") {
                return Err(format!(
                    "ожидался путь или URL после {flag}, получен флаг {target}"
                ));
            }
            Ok(CliMode::Dump {
                source: PageSource::from_arg(Some(target)),
                kind,
            })
        }
        _ => Err(format!("слишком много аргументов: {}", args.len())),
    }
}

/// Parses the command line and dispatches to the chosen `CliMode`.
///
/// Split out of `main` so that every `return` inside it — including the early
/// argument-error paths — is followed by `diag_stderr::flush`.
pub(crate) fn run_cli() -> ExitCode {
    // Opt-in visual profiler (§14.3, BUG-284): `Client::start()` spawns the
    // background thread that connects to (or is discovered by) a running
    // Tracy GUI app — https://github.com/wolfpld/tracy. Must be started
    // before any `lumen_core::tracy_zone!` spans fire, so this is the very
    // first thing main() does. No-op unless built with `--features tracy`.
    #[cfg(feature = "tracy")]
    let _tracy_client = tracy_client::Client::start();
    #[cfg(feature = "tracy")]
    eprintln!("[tracy] профилировщик активен — открой Tracy GUI, чтобы увидеть таймлайн");

    // Anchor for launch->first-frame timing (§4 score table) — before any work.
    bench_frames::mark_process_start();
    // PERF-12: fixed-startup stopwatch. Must precede the config load and the
    // argument parse below, which are the phases it measures; it also switches
    // the tracer on for `--trace-nav`, so that startup lands on the timeline
    // instead of ahead of its origin.
    let startup = startup_trace::Startup::begin();
    let cfg_phase = startup.phase("config-load");
    // Load the fingerprint profile (9F.1) once, before any network or JS setup.
    // Absent config → engine defaults, so behaviour is unchanged out of the box.
    let mut startup_profile = config::load().unwrap_or_default();
    // BUG-295: automation sessions (BiDi / MCP) use an in-memory HTTP cache, never
    // the persistent on-disk one. The disk cache is keyed by URL and survives across
    // runs, so on the fixed ports an automation server reuses (e.g. wptserve's
    // 8000/8001) a resource fetched in one run is replayed stale in the next — even
    // after the served file changed on disk. That silently broke
    // `tests/wpt/run_smoke.py`: the first run (before the wptrunner `env_options` fix
    // served the right file) cached the wrong `testharnessreport.js` with its
    // `Cache-Control: max-age=3600`, and every later run kept serving that stale copy
    // from disk, setting the wrong result global forever, so the harness timed out no
    // matter what else was fixed. In-memory cache = fresh per process, deterministic.
    // This must be decided BEFORE `init_global` — the profile `OnceLock` is set-once,
    // so a later `init_global` is a no-op — hence a raw arg scan here rather than
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
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (bidi_port, rest_args) = match extract_bidi_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (mcp_live_port, rest_args) = match extract_mcp_live_port(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка аргументов: {err}");
            print_usage();
            return ExitCode::FAILURE;
        }
    };
    let (import_session, rest_args) = match extract_import_session(&rest_args) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Ошибка --import-session: {err}");
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
            eprintln!("Ошибка --proxy: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Если прокси передан в командной строке, переопределить конфиг.
    if let Some(proxy_str) = proxy {
        let mut cfg = config::global().clone();
        cfg.proxy = Some(proxy_str);
        config::init_global(cfg);
    }

    let (tor_port, rest_args) = extract_tor_mode(&rest_args);

    // --tor: переключить на профиль TorBrowser + SOCKS5 + без персистентного хранилища.
    if let Some(port) = tor_port {
        if !check_tor_connectivity(port) {
            eprintln!(
                "lumen --tor: Tor-демон недоступен на 127.0.0.1:{port} — \
                 запустите Tor перед запуском Lumen"
            );
            return ExitCode::FAILURE;
        }
        let mut cfg = config::global().clone();
        cfg.http_profile = lumen_network::HttpProfile::TorBrowser;
        cfg.socks5_proxy = Some(format!("socks5://127.0.0.1:{port}"));
        cfg.no_persistent_state = true;
        config::init_global(cfg);
        eprintln!(
            "lumen: Tor-режим активирован (socks5://127.0.0.1:{port}, \
             профиль TorBrowser, без персистентного хранилища)"
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
                eprintln!("Ошибка аргументов: {err}");
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
        eprintln!("Ошибка запуска DevTools на порту {port}: {e}");
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
        eprintln!("Ошибка запуска BiDi на порту {port}: {e}");
        return ExitCode::FAILURE;
    }

    if let Some(port) = mcp_live_port
        && let Err(e) = lumen_mcp::spawn_live(port, automation_handle.clone())
    {
        eprintln!("Ошибка запуска MCP (live) на порту {port}: {e}");
        return ExitCode::FAILURE;
    }

    let blocked_log = Arc::new(std::sync::Mutex::new(
        panels::shields_panel::BlockedLog::default(),
    ));
    let network_log = Arc::new(std::sync::Mutex::new(
        devtools::network_panel::NetworkLog::default(),
    ));
    // Sink chain: StdoutEventSink → NetworkLogSink → ResourceTimingSink →
    // ShieldCountSink. Each wrapper forwards to its inner sink, so all four
    // observe every event — the Resource Timing capture (BUG-839) is a tap, not
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

    // PH1-4: Запустить сетевой сервис как дочерний процесс (если --network-service).
    // Хендл живёт до конца main() — при дропе убивает дочерний процесс.
    // _transport хранит Arc, чтобы не дропнуть IPC-соединение до конца сессии.
    let (_network_svc, _transport) = if use_network_service {
        match network_service::NetworkServiceHandle::spawn() {
            Ok((handle, transport)) => {
                eprintln!("lumen: сетевой сервис запущен (PH1-4, --network-service)");
                (Some(handle), Some(transport))
            }
            Err(e) => {
                eprintln!("lumen: не удалось запустить сетевой сервис: {e}");
                eprintln!("lumen: продолжаю со встроенным HttpClient");
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    // --import-session переопределяет источник страницы и начальный scroll.
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
